//! Read-only candidate audit. Simulation modifies memory, never accounts.json or auth.json.
use codex_switcher_lib::{account::AccountStore, score_candidate_accounts};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bytes = std::fs::read(AccountStore::config_path())?;
    let mut store: AccountStore = serde_json::from_slice(&bytes)?;
    let simulate = std::env::args().any(|a| a == "--simulate-plus-exhausted");
    if simulate {
        let id = store.accounts.values().find(|a| {
            !a.is_session_anchor && !a.is_banned && !a.is_logged_out && !a.is_token_invalid
                && a.cached_quota.as_ref().is_some_and(|q| q.plan_type.eq_ignore_ascii_case("plus"))
        }).map(|a| a.id.clone()).ok_or("No healthy non-anchor Plus account to simulate")?;
        store.current = Some(id.clone());
        let q = store.accounts.get_mut(&id).unwrap().cached_quota.as_mut().unwrap();
        q.five_hour_left = 0.0;
        q.five_hour_reset_at = Some(chrono::Utc::now().timestamp() + 3600);
        q.has_credits = false;
        q.credits_balance = None;
    }
    let candidates: Vec<_> = score_candidate_accounts(&store).iter().map(|(id, _, _)| {
        let a = &store.accounts[id];
        serde_json::json!({
            "plan": a.cached_quota.as_ref().map(|q| &q.plan_type),
            "phone_anchor": a.is_session_anchor,
            "has_access_token": AccountStore::extract_access_token(&a.auth_json).is_some()
        })
    }).collect();
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "simulated_plus_exhaustion": simulate,
        "anchor_last": store.settings.protect_session_anchor,
        "candidates": candidates,
        "writes_performed": false
    }))?);
    Ok(())
}
