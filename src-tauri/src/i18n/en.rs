use super::{BackendLocale, APP_NAME};

pub struct EnglishLocale;

impl BackendLocale for EnglishLocale {
    fn oauth_success_html(&self) -> &'static str {
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\r\n\
         <html lang=\"en\"><body><h1>Authorization successful</h1><p>OpenAI is connected. Close this window and return to the app.</p>\
         <script>setTimeout(() => window.close(), 3000)</script></body></html>"
    }
    fn oauth_failure_response(&self) -> &'static str {
        "HTTP/1.1 400 Bad Request\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nAuthorization failed: invalid state or missing parameters"
    }
    fn tray_tooltip_logged_out(&self) -> String {
        format!("{APP_NAME} - Not signed in")
    }
    fn tray_tooltip_account(&self, name: &str, five_hour_left: f64, weekly_left: f64) -> String {
        format!("{APP_NAME} - {name} | 5H: {five_hour_left:.0}%  Weekly: {weekly_left:.0}%")
    }
    fn tray_show_main(&self) -> &'static str {
        "Open main window"
    }
    fn tray_next_account(&self) -> &'static str {
        "Switch to next account"
    }
    fn tray_quit(&self) -> &'static str {
        "Quit"
    }
    fn notification_account_banned_subtitle(&self) -> &'static str {
        "Account ban detected"
    }
    fn notification_auto_switch_subtitle(&self) -> &'static str {
        "Automatic account switch"
    }
    fn injected_switch_message(&self, account_name: &str) -> String {
        format!("⚡ [Codex Switcher] Switched to {account_name}")
    }
    fn referral_unsupported_program(&self) -> &'static str {
        "Unsupported invitation campaign"
    }
    fn referral_uncertain_send_suffix(&self) -> &'static str {
        "; send result unconfirmed. Check invitation records before retrying"
    }
    fn referral_network_error(&self, error: &reqwest::Error, uncertain: &str) -> String {
        format!("Invitation API network error: {error}{uncertain}")
    }
    fn referral_non_json_response(&self, status: u16, uncertain: &str) -> String {
        format!("Invitation API returned HTTP {status} with non-JSON data. An official desktop login may be required; eligibility cannot be confirmed{uncertain}")
    }
    fn referral_upstream_rejected(&self) -> &'static str {
        "Request rejected by upstream"
    }
    fn referral_failed_emails(&self, emails: &serde_json::Value) -> String {
        format!("; emails: {emails}")
    }
    fn referral_unrecognized_response(&self, uncertain: &str) -> String {
        format!("Unrecognized invitation API response{uncertain}")
    }
    fn referral_missing_items(&self) -> &'static str {
        "Invitation response lacks items; records cannot be confirmed"
    }
    fn referral_no_available_campaign(&self) -> &'static str {
        "No invitation campaign available for this account. Refresh eligibility"
    }
}
