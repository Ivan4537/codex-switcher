//! 构造与官方 codex CLI 完全同形态的 `originator` / `User-Agent`。
//!
//! 背景：我们几个**自建**的、直连 OpenAI / ChatGPT 的请求（quota 查询、OAuth
//! token 兑换 / 刷新）必须看起来像真 codex，否则就是指纹。主代理转发链路不需要
//! 这里——它逐字透传 codex 自己的 header（见 proxy.rs `build_upstream_headers`）。
//!
//! 官方实现见 `codex-rs/login/src/auth/default_client.rs` 的 `get_codex_user_agent()`：
//!   `{originator}/{version} ({os_type} {os_version}; {arch}) {terminal}`
//! 例：`codex_cli_rs/0.137.0 (Mac OS 15.5; arm64) iTerm.app`
//! - version  = codex 自己的 CARGO_PKG_VERSION（这里改成探测本机安装的 codex 版本）
//! - os_type  = os_info::os_type()，macOS 上是 "Mac OS"
//! - arch     = os_info::architecture()，Apple Silicon 是 "arm64"（不是 std 的 aarch64）
//! - terminal = codex_terminal_detection::user_agent()，终端程序名，缺省 "unknown"

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// 官方默认 originator（codex-rs `DEFAULT_ORIGINATOR`）。
pub const CODEX_ORIGINATOR: &str = "codex_cli_rs";

/// 探测不到本机 codex 版本时的兜底。
///
/// 这个值会随官方发布而过期；环境变量和本机 npm manifest 探测链才是正解。
const DEFAULT_CODEX_VERSION: &str = "0.160.0";

/// 完整 User-Agent，进程内只算一次。
pub fn codex_user_agent() -> &'static str {
    static UA: OnceLock<String> = OnceLock::new();
    UA.get_or_init(|| {
        let raw = format!(
            "{}/{} ({} {}; {}) {}",
            CODEX_ORIGINATOR,
            codex_version(),
            os_type(),
            os_version(),
            arch(),
            terminal_token(),
        );
        let user_agent = sanitize_header(raw);
        println!("[CodexUA] {}", user_agent);
        user_agent
    })
    .as_str()
}

/// 本机安装的 codex 版本：环境变量 → npm manifest → `codex --version` → 常量。
fn codex_version() -> String {
    static VER: OnceLock<String> = OnceLock::new();
    VER.get_or_init(detect_codex_version_or_default).clone()
}

#[cfg(windows)]
fn windows_version_command() -> Option<Command> {
    use std::os::windows::process::CommandExt;
    let root = std::env::var_os("SystemRoot")?;
    let mut command = Command::new(std::path::PathBuf::from(root).join("System32/cmd.exe"));
    // Fixed arguments only; cmd resolves both npm's codex.cmd and native codex.exe.
    command.args(["/D", "/V:OFF", "/C", "codex", "--version"]);
    command.creation_flags(0x08000000);
    Some(command)
}

fn detect_codex_version_or_default() -> String {
    detect_codex_version().unwrap_or_else(|| DEFAULT_CODEX_VERSION.to_string())
}

fn detect_codex_version() -> Option<String> {
    let env_version = std::env::var("CODEX_SWITCHER_CLIENT_VERSION").ok();
    let binaries = codex_binary_candidates();
    let npm_roots = npm_global_roots();
    detect_codex_version_from_sources(
        env_version.as_deref(),
        &binaries,
        &npm_roots,
        run_codex_version,
    )
}

fn detect_codex_version_from_sources<F>(
    env_version: Option<&str>,
    binaries: &[PathBuf],
    npm_roots: &[PathBuf],
    run_version: F,
) -> Option<String>
where
    F: Fn(&Path) -> Option<String>,
{
    if let Some(version) = env_version.map(str::trim).filter(|value| !value.is_empty()) {
        return Some(version.to_string());
    }

    for binary in binaries {
        if let Some(version) = version_from_binary_manifest(binary) {
            return Some(version);
        }
    }
    for root in npm_roots {
        if let Some(version) = version_from_manifest(&root.join("@openai/codex/package.json")) {
            return Some(version);
        }
    }
    binaries.iter().find_map(|binary| run_version(binary))
}

fn version_from_binary_manifest(binary: &Path) -> Option<String> {
    let resolved = binary.canonicalize().ok()?;
    resolved
        .ancestors()
        .find_map(|parent| version_from_manifest(&parent.join("package.json")))
}

fn version_from_manifest(path: &Path) -> Option<String> {
    let data: serde_json::Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    if data.get("name").and_then(|value| value.as_str()) != Some("@openai/codex") {
        return None;
    }
    data.get("version")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn augmented_path() -> std::ffi::OsString {
    let mut paths: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    for extra in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"] {
        paths.push(PathBuf::from(extra));
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        for sub in [".local/bin", ".bun/bin", ".npm-global/bin", ".volta/bin"] {
            paths.push(home.join(sub));
        }
    }
    std::env::join_paths(paths).unwrap_or_default()
}

fn codex_binary_candidates() -> Vec<PathBuf> {
    let mut candidates = vec![
        PathBuf::from("/opt/homebrew/bin/codex"),
        PathBuf::from("/usr/local/bin/codex"),
    ];
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(
            std::env::split_paths(&path)
                .map(|dir| dir.join(if cfg!(windows) { "codex.cmd" } else { "codex" })),
        );
    }
    candidates.push(PathBuf::from(if cfg!(windows) {
        "codex.cmd"
    } else {
        "codex"
    }));
    dedupe_paths(candidates)
}

fn npm_global_roots() -> Vec<PathBuf> {
    let path = augmented_path();
    let mut roots = Vec::new();
    for npm in ["/opt/homebrew/bin/npm", "/usr/local/bin/npm", "npm"] {
        let Ok(out) = Command::new(npm)
            .args(["root", "-g"])
            .env("PATH", &path)
            .output()
        else {
            continue;
        };
        if out.status.success() {
            let root = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !root.is_empty() {
                roots.push(PathBuf::from(root));
            }
        }
    }
    dedupe_paths(roots)
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

fn run_codex_version(binary: &Path) -> Option<String> {
    #[cfg(windows)]
    let out = if binary.components().count() == 1 {
        windows_version_command()?.output().ok()?
    } else {
        Command::new(binary).arg("--version").output().ok()?
    };
    #[cfg(not(windows))]
    let out = Command::new(binary)
        .arg("--version")
        .env("PATH", augmented_path())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    // 输出形如 "codex-cli 0.137.0" / "codex 0.137.0"；抓第一个 x.y.z。
    parse_semver(&String::from_utf8_lossy(&out.stdout))
}

fn parse_semver(s: &str) -> Option<String> {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r"\d+\.\d+\.\d+[A-Za-z0-9.\-+]*").unwrap());
    re.find(s).map(|m| m.as_str().to_string())
}

pub(crate) fn os_type() -> &'static str {
    // 对齐 os_info::os_type() 的取值。
    if cfg!(target_os = "macos") {
        "Mac OS"
    } else if cfg!(target_os = "windows") {
        "Windows"
    } else {
        "Linux"
    }
}

pub(crate) fn os_version() -> String {
    static V: OnceLock<String> = OnceLock::new();
    V.get_or_init(|| detect_os_version().unwrap_or_else(|| "unknown".to_string()))
        .clone()
}

#[cfg(target_os = "macos")]
fn detect_os_version() -> Option<String> {
    let out = Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!v.is_empty()).then_some(v)
}

#[cfg(not(target_os = "macos"))]
fn detect_os_version() -> Option<String> {
    None
}

pub(crate) fn arch() -> &'static str {
    // os_info 的命名：Apple Silicon = "arm64"（std 的 ARCH 是 "aarch64"）。
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        other => other,
    }
}

/// 复刻 `codex_terminal_detection::user_agent()` 的主路径（覆盖常见终端，
/// 兜底 "unknown"）。GUI 进程一般无 TERM_PROGRAM → 返回 "unknown"，与真
/// codex 在非终端上下文一致。
fn terminal_token() -> String {
    if let Ok(tp) = std::env::var("TERM_PROGRAM") {
        let tp = tp.trim();
        if !tp.is_empty() && !tp.eq_ignore_ascii_case("tmux") {
            return match std::env::var("TERM_PROGRAM_VERSION") {
                Ok(v) if !v.trim().is_empty() => format!("{tp}/{}", v.trim()),
                _ => tp.to_string(),
            };
        }
    }
    if std::env::var("WEZTERM_VERSION").is_ok() {
        return "WezTerm".to_string();
    }
    if std::env::var("ITERM_SESSION_ID").is_ok() || std::env::var("ITERM_PROFILE").is_ok() {
        return "iTerm.app".to_string();
    }
    if std::env::var("KITTY_WINDOW_ID").is_ok() {
        return "kitty".to_string();
    }
    if std::env::var("ALACRITTY_SOCKET").is_ok() {
        return "Alacritty".to_string();
    }
    "unknown".to_string()
}

/// 把非可打印 ASCII 替换成 '_'，保证能塞进 HeaderValue（对齐 codex 的 sanitize）。
fn sanitize_header(s: String) -> String {
    if s.bytes().all(|b| (b' '..=b'~').contains(&b)) {
        s
    } else {
        s.chars()
            .map(|c| if (' '..='~').contains(&c) { c } else { '_' })
            .collect()
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    #[test]
    fn version_probe_preserves_windows_path_and_uses_fixed_batch_arguments() {
        let command = super::windows_version_command().unwrap();
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy())
            .collect();
        assert_eq!(args, ["/D", "/V:OFF", "/C", "codex", "--version"]);
        assert_eq!(command.get_envs().count(), 0);
        assert!(command.get_program().to_string_lossy().ends_with("cmd.exe"));
    }
}

#[cfg(test)]
mod version_tests {
    use super::*;

    #[test]
    fn environment_version_has_highest_priority() {
        let version = detect_codex_version_from_sources(
            Some(" 9.8.7 "),
            &[PathBuf::from("/does/not/exist")],
            &[],
            |_| Some("1.2.3".to_string()),
        );
        assert_eq!(version.as_deref(), Some("9.8.7"));
    }

    #[cfg(unix)]
    #[test]
    fn reads_version_from_symlinked_npm_manifest_without_running_launcher() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!("codex-ua-test-{}", uuid::Uuid::new_v4()));
        let package = root.join("lib/node_modules/@openai/codex");
        let launcher = package.join("bin/codex.js");
        let link = root.join("bin/codex");
        std::fs::create_dir_all(launcher.parent().unwrap()).unwrap();
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::fs::write(&launcher, "#!/usr/bin/env node\n").unwrap();
        std::fs::write(
            package.join("package.json"),
            r#"{"name":"@openai/codex","version":"7.6.5"}"#,
        )
        .unwrap();
        symlink(&launcher, &link).unwrap();

        let version = detect_codex_version_from_sources(None, &[link], &[], |_| {
            panic!("manifest detection must not execute the Node launcher")
        });
        assert_eq!(version.as_deref(), Some("7.6.5"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn falls_back_to_pinned_version_when_all_detection_fails() {
        let detected = detect_codex_version_from_sources(None, &[], &[], |_| None);
        assert_eq!(
            detected.unwrap_or_else(|| DEFAULT_CODEX_VERSION.to_string()),
            DEFAULT_CODEX_VERSION
        );
    }
}
