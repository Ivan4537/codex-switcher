use crate::{account::AccountStore, usage::UsageDisplay, AppState};
use tauri::{Emitter, State};

fn validate_recovery(
    store: &AccountStore,
    expected_anchor: &str,
    target: &str,
    usage: &UsageDisplay,
) -> Result<(), String> {
    if store.session_anchor_id().as_deref() != Some(expected_anchor) {
        return Err("Phone anchor changed. Refresh the recovery panel and try again.".into());
    }
    let account = store
        .accounts
        .get(target)
        .ok_or("Target account no longer exists.")?;
    if target == expected_anchor
        || !account.is_chatgpt_oauth()
        || account.is_banned
        || account.is_logged_out
        || account.is_token_invalid
        || AccountStore::extract_access_token(&account.auth_json).is_none()
    {
        return Err("Choose a different healthy ChatGPT OAuth account.".into());
    }
    if !usage.is_valid_for_cli
        || !usage.has_usable_quota()
        || !usage
            .desktop_gate
            .as_ref()
            .is_some_and(|gate| gate.confirmed_available())
    {
        return Err("Target workspace is blocked or its availability could not be confirmed. No anchor was changed.".into());
    }
    Ok(())
}

/// Rebind only after the user confirms the exact target and a fresh quota check.
/// Inference current stays independent. Desktop must reload its shell identity.
#[tauri::command]
pub async fn recover_session_anchor(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    expected_anchor: String,
    target: String,
) -> Result<(), String> {
    let target_identity = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        if store.session_anchor_id().as_deref() != Some(expected_anchor.as_str()) {
            return Err("Phone anchor changed. Refresh the recovery panel and try again.".into());
        }
        store
            .accounts
            .get(&target)
            .ok_or("Target account no longer exists.")?
            .auth_json
            .clone()
    };
    let usage = crate::get_quota_by_id(state.clone(), app.clone(), target.clone()).await?;
    {
        let mut store = state.store.lock().map_err(|e| e.to_string())?;
        validate_recovery(&store, &expected_anchor, &target, &usage)?;
        if !AccountStore::auth_identity_matches(
            &target_identity,
            &store.accounts.get(&target).unwrap().auth_json,
        ) {
            return Err(
                "Target identity changed during verification. Check it again before migrating."
                    .into(),
            );
        }
        let old_auth = AccountStore::read_codex_auth()?;
        let new_auth = store.accounts.get(&target).unwrap().to_codex_auth_value();
        let previous = store.clone();
        AccountStore::write_codex_auth(&new_auth)?;
        if let Err(error) = store
            .set_session_anchor(&target, true)
            .and_then(|_| store.save())
        {
            *store = previous;
            return match AccountStore::write_codex_auth(&old_auth) {
                Ok(()) => Err(error),
                Err(rollback) => Err(format!("Anchor save failed: {error}; disk rollback failed: {rollback}. Restore the previous anchor before continuing.")),
            };
        }
    }
    crate::proxy::invalidate_remote_token_cache();
    let _ = app.emit("accounts-updated", ());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_usage() -> UsageDisplay {
        serde_json::from_value(serde_json::json!({
            "plan_type":"plus", "five_hour_used":0, "five_hour_left":100,
            "five_hour_label":"5H", "five_hour_reset":"", "five_hour_reset_at":null,
            "primary_window_seconds":18000, "weekly_used":0,"weekly_left":100,
            "weekly_label":"7D","weekly_reset":"","weekly_reset_at":null,
            "secondary_window_seconds":604800,"credits_balance":null,"has_credits":false,
            "reset_credits":null,"is_valid_for_cli":true,
            "desktop_gate":{"allowed":true,"limit_reached":false,"reason":null}
        }))
        .unwrap()
    }

    fn test_store() -> AccountStore {
        let mut store = AccountStore::default();
        for id in ["anchor", "worker"] {
            let account = serde_json::from_value(serde_json::json!({
                "id":id,"name":id,"kind":"chatgpt_oauth",
                "auth_json":{"tokens":{"access_token":"eyJ.test-token","account_id":id}},
                "created_at":"2026-10-06T00:00:00Z","is_session_anchor":id == "anchor"
            }))
            .unwrap();
            store.accounts.insert(id.into(), account);
        }
        store.current = Some("worker".into());
        store
    }

    #[test]
    fn healthy_target_accepted_without_changing_current_or_anchor() {
        let store = test_store();
        assert!(validate_recovery(&store, "anchor", "worker", &test_usage()).is_ok());
        assert_eq!(store.current.as_deref(), Some("worker"));
        assert_eq!(store.session_anchor_id().as_deref(), Some("anchor"));
    }

    #[test]
    fn blocked_unknown_deleted_or_same_target_rejected() {
        let mut store = test_store();
        let mut usage = test_usage();
        usage.desktop_gate = None;
        assert!(validate_recovery(&store, "anchor", "worker", &usage).is_err());
        usage = test_usage();
        usage.desktop_gate.as_mut().unwrap().reason =
            Some("workspace_owner_credits_depleted".into());
        assert!(validate_recovery(&store, "anchor", "worker", &usage).is_err());
        assert!(validate_recovery(&store, "anchor", "anchor", &test_usage()).is_err());
        store.accounts.remove("worker");
        assert!(validate_recovery(&store, "anchor", "worker", &test_usage()).is_err());
    }

    #[test]
    fn changed_anchor_is_rejected_before_any_write() {
        let store = AccountStore::default();
        let usage: UsageDisplay = serde_json::from_value(serde_json::json!({
            "plan_type":"plus", "five_hour_used":0, "five_hour_left":100,
            "five_hour_label":"5H", "five_hour_reset":"", "five_hour_reset_at":null,
            "primary_window_seconds":18000, "weekly_used":0,"weekly_left":100,
            "weekly_label":"7D","weekly_reset":"","weekly_reset_at":null,
            "secondary_window_seconds":604800,"credits_balance":null,"has_credits":false,
            "reset_credits":null,"is_valid_for_cli":true
        }))
        .unwrap();
        assert!(validate_recovery(&store, "old", "target", &usage)
            .unwrap_err()
            .contains("anchor changed"));
    }
}
