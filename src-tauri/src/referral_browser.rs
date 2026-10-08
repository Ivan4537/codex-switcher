//! User-operated, read-only browser query. No CAPTCHA solving or invitation POST.
use crate::{account::AccountStore, AppState};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tauri::{State, WebviewUrl, WebviewWindowBuilder};

pub(crate) fn isolated_window(label: &str) -> bool {
    label.starts_with("referral-browser-")
}

fn readback(
    url: &url::Url,
    nonce: &str,
    account: &str,
    user: &str,
) -> Option<Result<Value, String>> {
    if url.scheme() != "referral-result" || url.host_str() != Some("result") {
        return None;
    }
    let pairs: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    if pairs.get("nonce").map(String::as_str) != Some(nonce) {
        return None;
    }
    let payload = pairs.get("data")?;
    if payload.len() > 65536 {
        return Some(Err("Browser eligibility response is too large.".into()));
    }
    Some((|| {
        let data: Value =
            serde_json::from_str(payload).map_err(|_| "Invalid browser eligibility response.")?;
        if data["account_id"].as_str() != Some(account) || data["user_id"].as_str() != Some(user) {
            return Err("Browser account identity does not match the selected account.".into());
        }
        let mut offer = crate::referrals::validate_eligibility(data["offer"].clone())?;
        // Only eligibility fields return to the application, never tokens/cookies.
        let allowed = [
            "should_show",
            "offer_id",
            "grants",
            "remaining_send_capacity",
            "remaining_reward_capacity",
            "requires_explicit_confirmation",
        ];
        offer
            .as_object_mut()
            .unwrap()
            .retain(|key, _| allowed.contains(&key.as_str()));
        if let Some(grants) = offer.get_mut("grants").and_then(Value::as_array_mut) {
            for grant in grants {
                grant
                    .as_object_mut()
                    .unwrap()
                    .retain(|key, _| ["recipient", "grant_type", "amount"].contains(&key.as_str()));
            }
        }
        offer["query_source"] = json!("verified_browser");
        if let Some(entrypoint) = data["query_entrypoint"].as_str() {
            if !matches!(entrypoint, "persistent" | "rate_limit") {
                return Err("Unsupported browser referral entrypoint.".into());
            }
            offer["query_entrypoint"] = json!(entrypoint);
        }
        if let Some(checked) = data["checked_entrypoints"].as_array() {
            if checked.is_empty()
                || checked.len() > 2
                || checked
                    .iter()
                    .any(|item| !matches!(item.as_str(), Some("persistent" | "rate_limit")))
            {
                return Err("Invalid browser referral query context.".into());
            }
            offer["checked_entrypoints"] = json!(checked);
        }
        Ok(offer)
    })())
}

#[tauri::command]
pub async fn get_desktop_referral_eligibility_browser(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    id: String,
    program: String,
) -> Result<Value, String> {
    if !matches!(
        program.as_str(),
        "codex_referral_consumer" | "codex_referral_workspace"
    ) {
        return Err("Unsupported referral program.".into());
    }
    let identity = {
        let store = state.store.lock().map_err(|error| error.to_string())?;
        store
            .accounts
            .get(&id)
            .ok_or("Selected account no longer exists.")?
            .auth_json
            .clone()
    };
    let (token, account_id) = crate::resolve_account_access_token(
        &state,
        &id,
        "Only ChatGPT accounts support browser invitation queries.",
    )
    .await?;
    let account_id = account_id.ok_or("Selected account has no workspace identity.")?;
    if AccountStore::extract_account_id(&identity).as_deref() != Some(account_id.as_str()) {
        return Err("Selected account changed before browser verification.".into());
    }
    // A successful ordinary usage GET anchors browser results to the selected identity.
    let user_agent = crate::desktop_ua::user_agent();
    let baseline = crate::usage::usage_client()
        .get("https://chatgpt.com/backend-api/wham/usage")
        .bearer_auth(&token)
        .header("ChatGPT-Account-Id", &account_id)
        .header("User-Agent", &user_agent)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .map_err(|_| "Cannot verify selected account before opening the browser.")?;
    if !baseline.status().is_success() {
        return Err("Cannot verify selected account before opening the browser.".into());
    }
    let baseline: Value = baseline
        .json()
        .await
        .map_err(|_| "Cannot verify selected account identity.")?;
    if baseline["account_id"].as_str() != Some(account_id.as_str()) {
        return Err("Workspace identity mismatch.".into());
    }
    let user_id = baseline["user_id"]
        .as_str()
        .ok_or("Missing user identity.")?
        .to_string();
    let nonce = uuid::Uuid::new_v4().to_string();
    let label = format!("referral-browser-{nonce}");
    let config = json!({"nonce":nonce,"token":token,"account_id":account_id,"user_id":user_id,"program":program});
    let script = format!(
        "(() => {{ const CONFIG = {}; {} }})();",
        config,
        include_str!("referral_browser.js")
    );
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let sender = Arc::new(Mutex::new(Some(sender)));
    let navigation_sender = sender.clone();
    let nonce_copy = nonce.clone();
    let account_copy = account_id.clone();
    let user_copy = user_id.clone();
    let window = WebviewWindowBuilder::new(
        &app,
        &label,
        WebviewUrl::External("https://chatgpt.com/".parse().unwrap()),
    )
    .title("只读邀请查询 / Read-only invitation query")
    .user_agent(&user_agent)
    .inner_size(1050.0, 760.0)
    .initialization_script(script)
    .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
    .on_navigation(move |url| {
        if let Some(result) = readback(url, &nonce_copy, &account_copy, &user_copy) {
            if let Ok(mut slot) = navigation_sender.lock() {
                if let Some(sender) = slot.take() {
                    let _ = sender.send(result);
                }
            }
            return false;
        }
        url.scheme() == "https" && url.host_str() == Some("chatgpt.com")
    })
    .build()
    .map_err(|_| "Cannot open the official browser verification window.")?;
    let close_sender = sender.clone();
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            if let Ok(mut slot) = close_sender.lock() {
                if let Some(sender) = slot.take() {
                    let _ = sender.send(Err(
                        "Browser verification was closed; eligibility remains unconfirmed.".into(),
                    ));
                }
            }
        }
    });
    let result = tokio::time::timeout(std::time::Duration::from_secs(600), receiver).await;
    let _ = window.destroy();
    let offer = result
        .map_err(|_| "Browser verification timed out; eligibility remains unconfirmed.")?
        .map_err(|_| "Browser verification was interrupted.")??;
    let store = state.store.lock().map_err(|error| error.to_string())?;
    let current = store
        .accounts
        .get(&id)
        .ok_or("Selected account no longer exists.")?;
    if !AccountStore::auth_identity_matches(&identity, &current.auth_json) {
        return Err("Selected account changed during browser verification.".into());
    }
    Ok(offer)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn callback_requires_nonce_and_exact_workspace_and_user() {
        let mut url = url::Url::parse("referral-result://result").unwrap();
        let payload = json!({"account_id":"A","user_id":"U","offer":{"should_show":true,"remaining_send_capacity":3,"token":"must-not-return"},"query_entrypoint":"rate_limit","checked_entrypoints":["persistent","rate_limit"]});
        url.query_pairs_mut()
            .append_pair("nonce", "n")
            .append_pair("data", &payload.to_string());
        assert!(readback(&url, "wrong", "A", "U").is_none());
        assert!(readback(&url, "n", "B", "U").unwrap().is_err());
        assert!(readback(&url, "n", "A", "wrong").unwrap().is_err());
        let offer = readback(&url, "n", "A", "U").unwrap().unwrap();
        assert!(offer.get("token").is_none());
        assert_eq!(offer["query_source"], "verified_browser");
        assert_eq!(offer["query_entrypoint"], "rate_limit");
        assert_eq!(
            offer["checked_entrypoints"],
            json!(["persistent", "rate_limit"])
        );
    }
}
