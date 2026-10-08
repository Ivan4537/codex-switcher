//! ChatGPT Desktop referrals (2026-09 client contract).
//! This API is separate from banked wham/rate-limit-reset-credits.
use serde_json::{json, Value};
use std::time::Duration;
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
pub fn open_official_referral_client(app: tauri::AppHandle) -> Result<(), String> {
    app.opener()
        .open_url("codex://", None::<String>)
        .map_err(|error| error.to_string())
}

fn parse_response(
    status: u16,
    challenged: bool,
    body: &str,
    sending: bool,
) -> Result<Value, String> {
    let uncertain = if sending {
        crate::i18n::referral_uncertain_send_suffix()
    } else {
        ""
    };
    if challenged
        || (status == 403 && (body.contains("challenge-platform") || body.contains("cf-chl-")))
    {
        return Err(format!("邀请查询被网页防护拦截，资格和剩余次数未确认；这不代表没有活动。可在官方 ChatGPT Desktop 查看并邀请。{uncertain}"));
    }
    let data: Value = serde_json::from_str(body)
        .map_err(|_| crate::i18n::referral_non_json_response(status, uncertain))?;
    if !(200..300).contains(&status) {
        let detail = data.get("detail").unwrap_or(&data);
        let message = detail
            .as_str()
            .or_else(|| detail.get("message").and_then(Value::as_str))
            .unwrap_or(crate::i18n::referral_upstream_rejected());
        let failed = detail
            .get("failed_emails")
            .or_else(|| data.get("failed_emails"));
        return Err(format!(
            "HTTP {status}: {message}{}{}",
            failed
                .map(crate::i18n::referral_failed_emails)
                .unwrap_or_default(),
            uncertain
        ));
    }
    if !data.is_object() {
        return Err(crate::i18n::referral_unrecognized_response(uncertain));
    }
    Ok(data)
}

pub(crate) fn validate_eligibility(data: Value) -> Result<Value, String> {
    if data
        .get("grants")
        .filter(|value| !value.is_null())
        .is_some_and(|value| {
            !value
                .as_array()
                .is_some_and(|grants| grants.iter().all(Value::is_object))
        })
        || data
            .get("remaining_reward_capacity")
            .filter(|value| !value.is_null())
            .is_some_and(|value| value.as_u64().is_none())
        || data
            .get("offer_id")
            .filter(|value| !value.is_null())
            .is_some_and(|value| !value.is_string())
    {
        return Err(
            "邀请资格响应不完整，活动和剩余次数未确认，请重试或在官方 Desktop 查看。".into(),
        );
    }
    match data.get("should_show").and_then(Value::as_bool) {
        Some(false) => Ok(data),
        Some(true)
            if data
                .get("remaining_send_capacity")
                .and_then(Value::as_u64)
                .is_some() =>
        {
            Ok(data)
        }
        _ => Err("邀请资格响应不完整，活动和剩余次数未确认，请重试或在官方 Desktop 查看。".into()),
    }
}

fn context(program: &str) -> Result<Value, String> {
    match program {
        "codex_referral_consumer" | "codex_referral_workspace" => {
            Ok(json!({"program_id": program, "entrypoint": "persistent"}))
        }
        _ => Err(crate::i18n::referral_unsupported_program().into()),
    }
}

fn request(
    token: &str,
    account_id: Option<&str>,
    method: reqwest::Method,
    path: &str,
) -> reqwest::RequestBuilder {
    let mut req = crate::usage::usage_client()
        .request(
            method,
            format!("https://chatgpt.com/backend-api/referrals/invite{path}"),
        )
        .bearer_auth(token)
        .header("User-Agent", crate::desktop_ua::user_agent())
        // Referrals belong to the Desktop product, not the CLI surface. The
        // same OAuth token returns should_show=false with codex_cli_rs.
        .header("originator", "Codex Desktop")
        .header("OAI-Product-Sku", "CODEX")
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(30));
    if let Some(id) = account_id {
        req = req.header("ChatGPT-Account-Id", id);
    }
    req
}

async fn response(req: reqwest::RequestBuilder, sending: bool) -> Result<Value, String> {
    let uncertain = if sending {
        crate::i18n::referral_uncertain_send_suffix()
    } else {
        ""
    };
    let resp = req
        .send()
        .await
        .map_err(|e| crate::i18n::referral_network_error(&e, uncertain))?;
    let status = resp.status().as_u16();
    let challenged = resp
        .headers()
        .get("cf-mitigated")
        .and_then(|value| value.to_str().ok())
        == Some("challenge");
    let body = resp
        .text()
        .await
        .map_err(|e| crate::i18n::referral_network_error(&e, uncertain))?;
    parse_response(status, challenged, &body, sending)
}

pub async fn eligibility(token: &str, aid: Option<&str>, program: &str) -> Result<Value, String> {
    let ctx = context(program)?;
    let data = response(
        request(token, aid, reqwest::Method::GET, "/eligibility").query(&[
            ("program_id", ctx["program_id"].as_str().unwrap()),
            ("entrypoint", "persistent"),
        ]),
        false,
    )
    .await?;
    validate_eligibility(data)
}

pub async fn tracking(
    token: &str,
    aid: Option<&str>,
    program: &str,
    cursor: Option<&str>,
) -> Result<Value, String> {
    context(program)?;
    let mut req = request(token, aid, reqwest::Method::GET, "/tracking").query(&[
        ("program_id", program),
        ("period", "past_90_days"),
        ("limit", "100"),
    ]);
    if let Some(cursor) = cursor {
        req = req.query(&[("cursor", cursor)]);
    }
    let data = response(req, false).await?;
    if !data["items"].is_array() {
        return Err(crate::i18n::referral_missing_items().into());
    }
    Ok(data)
}

fn send_body(
    program: &str,
    emails: Vec<String>,
    offer: &Value,
    expected: &Value,
) -> Result<Value, String> {
    let mut body = context(program)?;
    if offer["should_show"] != true {
        return Err(crate::i18n::referral_no_available_campaign().into());
    }
    // Detect an offer change between review and submission, including grant amounts.
    for key in ["offer_id", "grants", "requires_explicit_confirmation"] {
        if offer[key] != expected[key] {
            return Err("邀请奖励或活动条件已变化，请刷新资格后确认".into());
        }
    }
    let mut seen = std::collections::HashSet::new();
    let emails: Vec<String> = emails
        .into_iter()
        .map(|e| e.trim().to_string())
        .filter(|e| !e.is_empty() && seen.insert(e.to_lowercase()))
        .collect();
    let mut cap = offer["remaining_send_capacity"]
        .as_u64()
        .unwrap_or(0)
        .min(5);
    // Match the official client: reward capacity limits offers with grants or legacy offer_id.
    let grants = offer["grants"]
        .as_array()
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    let has_offer = offer["offer_id"].as_str().is_some_and(|v| v != "none");
    if grants || has_offer {
        cap = cap.min(offer["remaining_reward_capacity"].as_u64().unwrap_or(0));
    }
    if emails.is_empty() || emails.len() as u64 > cap {
        return Err(format!("本次最多可邀请 {cap} 个邮箱"));
    }
    let email_re = regex::Regex::new(r"^[^\s@]+@[^\s@]+\.[^\s@]+$").unwrap();
    if emails.iter().any(|e| !email_re.is_match(e)) {
        return Err("邮箱格式不正确".into());
    }
    body["emails"] = json!(emails);
    Ok(body)
}

pub async fn send(
    token: &str,
    aid: Option<&str>,
    program: &str,
    emails: Vec<String>,
    expected: Value,
) -> Result<Value, String> {
    let offer = eligibility(token, aid, program).await?;
    let body = send_body(program, emails, &offer, &expected)?;
    // Never retry a POST or fall back to the old referral_key campaign.
    let data = response(
        request(token, aid, reqwest::Method::POST, "").json(&body),
        true,
    )
    .await?;
    if !data["invites"].is_array() {
        return Err("发送结果缺少 invites，请先查询邀请记录，勿直接重发".into());
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invitation_requests_use_desktop_product_context() {
        for method in [reqwest::Method::GET, reqwest::Method::POST] {
            let req = request("test-token", Some("test-account"), method, "/eligibility")
                .build()
                .unwrap();
            assert_eq!(req.headers()["originator"], "Codex Desktop");
            assert_eq!(req.headers()["OAI-Product-Sku"], "CODEX");
            assert_eq!(req.headers()["ChatGPT-Account-Id"], "test-account");
        }
    }
    fn offer() -> Value {
        json!({"should_show":true,"offer_id":"credits_1000","remaining_send_capacity":5,"remaining_reward_capacity":2,"grants":[{"recipient":"referrer","grant_type":"personal_credits","amount":1000}]})
    }
    #[test]
    fn new_payload_deduplicates_and_never_uses_old_key() {
        let o = offer();
        let b = send_body(
            "codex_referral_consumer",
            vec![" A@example.com ".into(), "a@example.com".into()],
            &o,
            &o,
        )
        .unwrap();
        assert_eq!(
            b,
            json!({"program_id":"codex_referral_consumer","entrypoint":"persistent","emails":["A@example.com"]})
        );
    }
    #[test]
    fn rejects_capacity_ineligibility_and_changed_rewards() {
        let o = offer();
        let emails = vec!["a@example.com".into()];
        let mut n = o.clone();
        n["grants"][0]["amount"] = json!(500);
        assert!(send_body("codex_referral_consumer", emails.clone(), &n, &o).is_err());
        n = o.clone();
        n["should_show"] = json!(false);
        assert!(send_body("codex_referral_consumer", emails.clone(), &n, &o).is_err());
        n = o.clone();
        n["remaining_send_capacity"] = json!(0);
        assert!(send_body("codex_referral_consumer", emails, &n, &o).is_err());
        n = o.clone();
        n["remaining_reward_capacity"] = json!(0);
        assert!(send_body(
            "codex_referral_consumer",
            vec!["a@example.com".into()],
            &n,
            &n,
        )
        .is_err());
    }

    #[test]
    fn challenge_and_incomplete_response_never_mean_ineligible() {
        let error = parse_response(403, true, "<html>challenge</html>", false).unwrap_err();
        assert!(error.contains("未确认"));
        assert!(validate_eligibility(json!({})).is_err());
        assert!(validate_eligibility(json!({"should_show":true})).is_err());
        assert!(validate_eligibility(
            json!({"should_show":true,"remaining_send_capacity":3,"grants":{}})
        )
        .is_err());
        assert!(validate_eligibility(json!({"should_show":false})).is_ok());
        let error = parse_response(403, true, "<html>challenge</html>", true).unwrap_err();
        assert!(error.contains("勿直接重发") || error.contains("retrying"));
    }

    #[test]
    fn rewardless_invites_keep_official_send_capacity() {
        let offer = json!({"should_show":true,"remaining_send_capacity":3,"remaining_reward_capacity":0,"grants":[],"offer_id":"none"});
        let body = send_body(
            "codex_referral_consumer",
            vec!["a@example.com".into()],
            &offer,
            &offer,
        )
        .unwrap();
        assert_eq!(body["emails"], json!(["a@example.com"]));
    }
}
