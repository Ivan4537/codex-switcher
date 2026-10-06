use crate::oauth;
use base64::{engine::general_purpose, Engine as _};
use rand::{rng, RngCore};
use std::sync::Mutex;
use std::sync::OnceLock;
use tauri::{AppHandle, Emitter};
use tauri_plugin_opener::OpenerExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::Duration;
use url::Url;

/// 使用 OnceLock 代替 lazy_static 存储 OAuth 流程中的临时数据
static PENDING_LOGIN: OnceLock<Mutex<Option<PendingLogin>>> = OnceLock::new();
static CALLBACK_TASK: OnceLock<Mutex<Option<tokio::task::JoinHandle<()>>>> = OnceLock::new();
static LOGIN_LIFECYCLE: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

fn get_login_lifecycle() -> &'static tokio::sync::Mutex<()> {
    LOGIN_LIFECYCLE.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn get_pending_login() -> &'static Mutex<Option<PendingLogin>> {
    PENDING_LOGIN.get_or_init(|| Mutex::new(None))
}

fn get_callback_task() -> &'static Mutex<Option<tokio::task::JoinHandle<()>>> {
    CALLBACK_TASK.get_or_init(|| Mutex::new(None))
}

struct PendingLogin {
    pkce: oauth::PkceCodes,
    port: u16,
    state: String,
}

/// 生成与官方一致的 state (Base64 编码的32字节随机数)
fn generate_state() -> String {
    let mut bytes = [0u8; 32];
    rng().fill_bytes(&mut bytes);
    general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// 官方固定端口
const DEFAULT_PORT: u16 = 1455;
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(180);

fn clear_pending_login(expected_state: Option<&str>) -> bool {
    if let Ok(mut pending) = get_pending_login().lock() {
        let should_clear = expected_state
            .is_none_or(|state| pending.as_ref().is_some_and(|login| login.state == state));
        if should_clear {
            return pending.take().is_some();
        }
    }
    false
}

async fn stop_callback_task(
    task_slot: &Mutex<Option<tokio::task::JoinHandle<()>>>,
) -> Result<(), String> {
    let task = task_slot.lock().map_err(|_| "OAuth 回调任务锁异常")?.take();
    if let Some(task) = task {
        task.abort();
        let _ = task.await;
    }
    Ok(())
}

async fn cancel_pending_login(
    lifecycle: &tokio::sync::Mutex<()>,
    pending: &Mutex<Option<PendingLogin>>,
    task_slot: &Mutex<Option<tokio::task::JoinHandle<()>>>,
) -> Result<(), String> {
    // Hold the lifecycle lock across abort/join and state cleanup. A new start
    // cannot install its PKCE or listener until the previous cancel is done.
    let _lifecycle = lifecycle.lock().await;
    stop_callback_task(task_slot).await?;
    pending.lock().map_err(|_| "登录流程状态锁异常")?.take();
    Ok(())
}

fn notify_login_failed(app_handle: &AppHandle, message: &str) {
    if let Err(error) = app_handle.emit("oauth-login-failed", message) {
        eprintln!("[OAuth] 发送登录失败事件失败: {}", error);
    }
}

/// 准备 OAuth 流程并返回授权 URL
///
/// `open_browser=Some(false)` 时不调用系统默认浏览器，前端可以把返回的 URL 拷贝到剪贴板，
/// 用户自行粘贴到目标浏览器里完成授权。回调监听仍然启动，所以授权完成后流程与"直接点击登录"一致。
#[tauri::command]
pub async fn start_oauth_login(
    app_handle: AppHandle,
    open_browser: Option<bool>,
) -> Result<String, String> {
    let _lifecycle = get_login_lifecycle().lock().await;
    // Await actual listener release instead of guessing with a fixed sleep.
    stop_callback_task(get_callback_task()).await?;
    clear_pending_login(None);

    let listener = TcpListener::bind(format!("127.0.0.1:{}", DEFAULT_PORT))
        .await
        .map_err(|e| {
            format!(
                "无法绑定本地端口 {}: {}。请关闭占用该端口的进程后重试。",
                DEFAULT_PORT, e
            )
        })?;
    let port = DEFAULT_PORT;

    // 2. 生成 PKCE 和 State (与官方一致)
    let pkce = oauth::generate_pkce();
    let state = generate_state();
    let redirect_uri = format!("http://localhost:{}/auth/callback", port);

    // 3. 构造授权 URL (与官方完全一致: 手动拼接, 不对特殊字符编码)
    let qs = format!(
        "response_type=code&client_id={}&redirect_uri={}&scope={}&code_challenge={}&code_challenge_method=S256&id_token_add_organizations=true&codex_cli_simplified_flow=true&state={}&originator=codex_cli_rs",
        oauth::CLIENT_ID,
        redirect_uri,
        "openid profile email offline_access",
        pkce.code_challenge,
        state
    );

    let auth_url = format!("{}?{}", oauth::AUTH_URL, qs);

    // 4. 保存状态，开启监听任务
    {
        let mut pending = get_pending_login()
            .lock()
            .map_err(|_| "登录流程状态锁异常")?;
        *pending = Some(PendingLogin {
            pkce: pkce.clone(),
            port,
            state: state.clone(),
        });
    }

    // 5. 启动异步监听
    let app_handle_clone = app_handle.clone();
    let handle = tokio::spawn(async move {
        handle_callback(listener, app_handle_clone, state).await;
    });
    if let Ok(mut task_slot) = get_callback_task().lock() {
        *task_slot = Some(handle);
    }

    // 6. 打开浏览器（除非前端显式要求"只拿 URL 不开浏览器"）
    if open_browser.unwrap_or(true) {
        let _ = app_handle.opener().open_url(&auth_url, None::<String>);
    }

    Ok(auth_url)
}

/// 监听回调
async fn handle_callback(listener: TcpListener, app_handle: AppHandle, expected_state: String) {
    match wait_for_callback(listener, &expected_state, CALLBACK_TIMEOUT).await {
        Ok(code) => {
            if let Err(e) = app_handle.emit("oauth-callback-received", code) {
                eprintln!("发送 oauth-callback-received 事件失败: {}", e);
            }
        }
        Err(message) => {
            if clear_pending_login(Some(&expected_state)) {
                notify_login_failed(&app_handle, &message);
            }
        }
    }
}

/// The deadline covers accept, request reads and response writes. A TCP client
/// that connects without sending a callback must not keep OAuth alive forever.
async fn wait_for_callback(
    listener: TcpListener,
    expected_state: &str,
    timeout: Duration,
) -> Result<String, String> {
    tokio::time::timeout(timeout, receive_callback(&listener, expected_state))
        .await
        .map_err(|_| "OpenAI 授权已超时，请重新发起登录。".to_string())?
}

async fn receive_callback(listener: &TcpListener, expected_state: &str) -> Result<String, String> {
    loop {
        let (mut socket, _) = match listener.accept().await {
            Ok(sock) => sock,
            Err(e) => {
                return Err(format!("无法监听 OpenAI 授权回调: {}", e));
            }
        };

        let mut buffer = [0; 4096];
        let request_read = async {
            let mut n = 0;
            while n < buffer.len() {
                let read = socket.read(&mut buffer[n..]).await?;
                if read == 0 {
                    break;
                }
                n += read;
                if buffer[..n].windows(4).any(|part| part == b"\r\n\r\n") {
                    break;
                }
            }
            Ok::<usize, std::io::Error>(n)
        };
        let n = match tokio::time::timeout(Duration::from_secs(5), request_read).await {
            Ok(Ok(n)) => n,
            Ok(Err(e)) => {
                eprintln!("[OAuth] 读取回调请求失败: {}", e);
                continue;
            }
            Err(_) => continue,
        };
        if n == 0 {
            continue;
        }
        let request = String::from_utf8_lossy(&buffer[..n]);

        if let Some(code) = extract_oauth_code_from_request(&request, expected_state) {
            // 发送成功 HTML 并通知前端
            let response = crate::i18n::oauth_success_html();
            let _ = tokio::time::timeout(
                Duration::from_secs(5),
                socket.write_all(response.as_bytes()),
            )
            .await;
            return Ok(code);
        }

        let response = crate::i18n::oauth_failure_response();
        let _ = tokio::time::timeout(
            Duration::from_secs(5),
            socket.write_all(response.as_bytes()),
        )
        .await;
    }
}

/// 取消仍在等待浏览器回调的 OAuth 流程，并立即释放固定监听端口。
#[tauri::command]
pub async fn cancel_oauth_login() -> Result<(), String> {
    cancel_pending_login(
        get_login_lifecycle(),
        get_pending_login(),
        get_callback_task(),
    )
    .await
}

fn extract_oauth_code_from_request(request: &str, expected_state: &str) -> Option<String> {
    let first_line = request.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() <= 1 {
        return None;
    }

    let callback_url = format!("http://localhost{}", parts[1]);
    let url = Url::parse(&callback_url).ok()?;
    let params: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();

    let code = params.get("code")?;
    let state = params.get("state")?;
    if state != expected_state {
        return None;
    }

    Some(code.to_string())
}

/// 手动粘贴回调链接或裸 code 提交。适用于浏览器没能跳回本机监听端口（走了代理、被防火墙拦截等）。
/// 接受的 input 形式：
/// - 完整 URL: `http://localhost:1455/auth/callback?code=XXX&state=YYY`
/// - query 串:  `?code=XXX&state=YYY` 或 `code=XXX&state=YYY`
/// - 裸 code（不推荐，不做 state 校验）
#[tauri::command]
pub async fn submit_oauth_callback(app_handle: AppHandle, input: String) -> Result<(), String> {
    let _lifecycle = get_login_lifecycle().lock().await;
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("回调链接不能为空".to_string());
    }

    // 尝试按 URL 解析；失败则按 query 串处理；都失败就当作裸 code
    let (code_opt, state_opt) = parse_callback_input(trimmed);

    let Some(code) = code_opt else {
        return Err("未能从输入中解析出 code 参数".to_string());
    };

    // 有 state 就校验；没 state 的裸 code 也接受（用户自己承担风险）
    if let Some(ref provided_state) = state_opt {
        let expected = {
            let guard = get_pending_login()
                .lock()
                .map_err(|_| "登录流程状态锁异常")?;
            guard.as_ref().map(|p| p.state.clone())
        };
        match expected {
            Some(expected) if expected != *provided_state => {
                return Err("state 校验不通过：这个回调链接不属于本次登录流程".to_string());
            }
            None => {
                return Err("登录流程已过期或未启动，请先点击『立即登录 OpenAI』".to_string());
            }
            _ => {}
        }
    }

    // 停掉后端 HTTP 监听，避免它再接收一个回调
    stop_callback_task(get_callback_task()).await?;

    // 走跟 HTTP 监听完全相同的路径：把 code 丢到前端
    app_handle
        .emit("oauth-callback-received", code)
        .map_err(|e| format!("派发 oauth-callback-received 失败: {}", e))?;
    Ok(())
}

fn parse_callback_input(input: &str) -> (Option<String>, Option<String>) {
    // 1) 完整 URL
    if let Ok(url) = Url::parse(input) {
        let params: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        if let Some(code) = params.get("code") {
            return (Some(code.clone()), params.get("state").cloned());
        }
    }
    // 2) 以 ? 开头或形如 code=xxx&state=yyy 的 query 串
    let stripped = input.trim_start_matches('?');
    if stripped.contains('=') {
        let fake = format!("http://x/?{}", stripped);
        if let Ok(url) = Url::parse(&fake) {
            let params: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
            if let Some(code) = params.get("code") {
                return (Some(code.clone()), params.get("state").cloned());
            }
        }
    }
    // 3) 裸 code：只要不含空白就认作 code
    if !input.chars().any(char::is_whitespace) {
        return (Some(input.to_string()), None);
    }
    (None, None)
}

#[cfg(windows)]
#[tauri::command]
pub async fn copy_to_clipboard(text: String) -> Result<(), String> {
    crate::windows_clipboard::write_text(&text).await
}

/// macOS 端剪贴板写入：webview 的 `navigator.clipboard.writeText` 在跨过 await
/// 后会丢失 user-gesture，触发 NotAllowedError；改走 pbcopy 通过 Tauri IPC 写入，
/// 不依赖 user gesture，也避开 webview 权限提示。
#[cfg(not(windows))]
#[tauri::command]
pub async fn copy_to_clipboard(text: String) -> Result<(), String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new("pbcopy")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("无法启动 pbcopy: {}", e))?;

    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| "pbcopy stdin 不可写".to_string())?;
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| format!("写入 pbcopy 失败: {}", e))?;
    }

    let status = child
        .wait()
        .map_err(|e| format!("等待 pbcopy 退出失败: {}", e))?;
    if !status.success() {
        return Err(format!("pbcopy 返回非零: {:?}", status.code()));
    }
    Ok(())
}

/// 最后一步：使用捕获到的 Code 交换 Token (由前端触发)
#[tauri::command]
pub async fn complete_oauth_login(code: String) -> Result<oauth::TokenResponse, String> {
    // 提取所需数据并立即释放锁，避免跨 await 持有 MutexGuard
    let (code_verifier, port) = {
        let _lifecycle = get_login_lifecycle().lock().await;
        let mut pending_lock = get_pending_login().lock().map_err(|_| "锁被污染")?;
        let pending = pending_lock.take().ok_or("登录流程已过期或未启动")?;
        (pending.pkce.code_verifier, pending.port)
    };

    let redirect_uri = format!("http://localhost:{}/auth/callback", port);

    oauth::exchange_code(&code, &redirect_uri, &code_verifier).await
}

#[cfg(test)]
mod tests {
    use super::{
        cancel_pending_login, extract_oauth_code_from_request, wait_for_callback, PendingLogin,
    };
    use std::future::Future;
    use std::sync::{Arc, Mutex};
    use std::task::Poll;
    use tokio::io::AsyncWriteExt;
    use tokio::net::{TcpListener, TcpStream};
    use tokio::time::Duration;

    #[tokio::test]
    async fn restart_waits_for_cancel_join_and_keeps_new_pkce() {
        let lifecycle = Arc::new(tokio::sync::Mutex::new(()));
        let pending = Arc::new(Mutex::new(Some(PendingLogin {
            pkce: crate::oauth::generate_pkce(),
            port: 1455,
            state: "old".into(),
        })));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task_slot = Mutex::new(Some(tokio::spawn(async move {
            let _listener = listener;
            std::future::pending::<()>().await;
        })));

        let mut cancellation = Box::pin(cancel_pending_login(&lifecycle, &pending, &task_slot));
        // Poll cancel until it is waiting on the aborted task's join. This is
        // precisely the gap where the old implementation allowed a new start.
        std::future::poll_fn(|cx| {
            assert!(matches!(cancellation.as_mut().poll(cx), Poll::Pending));
            Poll::Ready(())
        })
        .await;
        assert!(lifecycle.try_lock().is_err());

        let restart_lifecycle = lifecycle.clone();
        let restart_pending = pending.clone();
        let restart = tokio::spawn(async move {
            let _guard = restart_lifecycle.lock().await;
            let _rebound = TcpListener::bind(address).await.unwrap();
            let pkce = crate::oauth::generate_pkce();
            let verifier = pkce.code_verifier.clone();
            *restart_pending.lock().unwrap() = Some(PendingLogin {
                pkce,
                port: address.port(),
                state: "new".into(),
            });
            verifier
        });
        tokio::task::yield_now().await;
        assert_eq!(pending.lock().unwrap().as_ref().unwrap().state, "old");
        cancellation.await.unwrap();
        let verifier = restart.await.unwrap();
        let new_pending = pending.lock().unwrap();
        let new_pending = new_pending.as_ref().unwrap();
        assert_eq!(new_pending.state, "new");
        assert_eq!(new_pending.pkce.code_verifier, verifier);
        assert!(task_slot.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn silent_callback_connection_times_out_and_releases_listener_port() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let _silent_client = TcpStream::connect(address).await.unwrap();
        let result = wait_for_callback(listener, "s1", Duration::from_millis(50)).await;
        assert!(result.unwrap_err().contains("超时"));
        let _rebound = TcpListener::bind(address).await.unwrap();
    }

    #[tokio::test]
    async fn fragmented_callback_headers_are_received_before_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task =
            tokio::spawn(
                async move { wait_for_callback(listener, "s1", Duration::from_secs(2)).await },
            );
        let mut client = TcpStream::connect(address).await.unwrap();
        client
            .write_all(b"GET /auth/callback?code=abc")
            .await
            .unwrap();
        tokio::task::yield_now().await;
        client
            .write_all(b"123&state=s1 HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        assert_eq!(task.await.unwrap().unwrap(), "abc123");
    }

    #[test]
    fn extract_code_success_when_state_matches() {
        let req = "GET /auth/callback?code=abc123&state=s1 HTTP/1.1\r\nHost: localhost\r\n\r\n";
        assert_eq!(
            extract_oauth_code_from_request(req, "s1"),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn extract_code_returns_none_when_state_mismatch() {
        let req = "GET /auth/callback?code=abc123&state=s2 HTTP/1.1\r\nHost: localhost\r\n\r\n";
        assert_eq!(extract_oauth_code_from_request(req, "s1"), None);
    }

    #[test]
    fn extract_code_returns_none_when_invalid_request_line() {
        let req = "INVALID\r\nHost: localhost\r\n\r\n";
        assert_eq!(extract_oauth_code_from_request(req, "s1"), None);
    }
}
