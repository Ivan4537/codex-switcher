//! Read-only observations of the upstream `X-Codex-Turn-State` response header.
//!
//! This module deliberately does not retain or expose the header value.  The
//! envelope shape and the 292/312/332 length buckets are empirical heuristics,
//! not proof of account health or model quality.  The first implementation is
//! passive-only: it never probes, injects, retries, or changes routing.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

const HEADER: &str = "X-Codex-Turn-State";
const MAX_RECENT: usize = 100;
const HEURISTIC_TTL_SECONDS: i64 = 3600;

#[derive(Debug, Clone, Serialize)]
pub struct TurnStateObservation {
    pub observed_at: String,
    pub account_id: String,
    pub model: String,
    pub session_key_hash: Option<String>,
    pub source: String,
    pub status: u16,
    pub length: usize,
    pub classification: String,
    pub time_status: String,
    pub envelope_ok: bool,
    pub blocks: Option<usize>,
    pub issued_at: Option<String>,
    pub expires_at: Option<String>,
    pub heuristic_usable: bool,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TurnStateStatus {
    pub header: &'static str,
    pub total_observations: u64,
    pub by_classification: BTreeMap<String, u64>,
    pub last: Option<TurnStateObservation>,
    pub recent: Vec<TurnStateObservation>,
}

struct Inner {
    total_observations: u64,
    by_classification: BTreeMap<String, u64>,
    last: Option<TurnStateObservation>,
    recent: VecDeque<TurnStateObservation>,
}

pub struct TurnStateMonitor {
    inner: Mutex<Inner>,
}

impl Default for TurnStateMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl TurnStateMonitor {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                total_observations: 0,
                by_classification: BTreeMap::new(),
                last: None,
                recent: VecDeque::with_capacity(MAX_RECENT),
            }),
        }
    }

    pub fn observe(
        &self,
        value: &str,
        account_id: Option<&str>,
        model: Option<&str>,
        session_key: Option<&str>,
        source: &str,
        status: u16,
    ) {
        let value = value.trim();
        if value.is_empty() {
            return;
        }

        let observation = inspect(value, account_id, model, session_key, source, status);
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        inner.total_observations += 1;
        *inner
            .by_classification
            .entry(observation.classification.clone())
            .or_default() += 1;
        inner.last = Some(observation.clone());
        inner.recent.push_front(observation);
        while inner.recent.len() > MAX_RECENT {
            inner.recent.pop_back();
        }
    }

    pub fn status(&self) -> TurnStateStatus {
        let Ok(inner) = self.inner.lock() else {
            return TurnStateStatus {
                header: HEADER,
                total_observations: 0,
                by_classification: BTreeMap::new(),
                last: None,
                recent: Vec::new(),
            };
        };
        TurnStateStatus {
            header: HEADER,
            total_observations: inner.total_observations,
            by_classification: inner.by_classification.clone(),
            last: inner.last.clone(),
            recent: inner.recent.iter().cloned().collect(),
        }
    }
}

fn inspect(
    value: &str,
    account_id: Option<&str>,
    model: Option<&str>,
    session_key: Option<&str>,
    source: &str,
    status: u16,
) -> TurnStateObservation {
    let now = Utc::now();
    let fingerprint = short_hash(value);
    let mut result = TurnStateObservation {
        observed_at: now.to_rfc3339(),
        account_id: account_id.unwrap_or_default().to_string(),
        model: model.unwrap_or_default().to_string(),
        session_key_hash: session_key.map(short_hash),
        source: source.to_string(),
        status,
        length: value.len(),
        classification: classify_length(value.len()),
        time_status: "unparsed".to_string(),
        envelope_ok: false,
        blocks: None,
        issued_at: None,
        expires_at: None,
        heuristic_usable: false,
        fingerprint,
    };

    if value.len() > 2048 || value.chars().any(|c| c.is_ascii_whitespace()) {
        result.classification = "malformed".to_string();
        return result;
    }

    let core = value.trim_end_matches('=');
    if value.len() - core.len() > 2 {
        result.classification = "malformed".to_string();
        return result;
    }
    let Ok(raw) = URL_SAFE_NO_PAD.decode(core) else {
        result.classification = "malformed".to_string();
        return result;
    };
    if raw.len() < 73 || raw[0] != 0x80 || (raw.len() - 57) % 16 != 0 {
        result.classification = "unknown_envelope".to_string();
        return result;
    }

    let blocks = (raw.len() - 57) / 16;
    result.envelope_ok = true;
    result.blocks = Some(blocks);
    result.classification = classify_blocks_and_length(blocks, value.len());

    let issued_seconds = u64::from_be_bytes(raw[1..9].try_into().unwrap());
    let Some(issued) = DateTime::<Utc>::from_timestamp(issued_seconds as i64, 0) else {
        result.time_status = "invalid_timestamp".to_string();
        return result;
    };
    result.issued_at = Some(issued.to_rfc3339());
    let expires = issued + Duration::seconds(HEURISTIC_TTL_SECONDS);
    result.expires_at = Some(expires.to_rfc3339());
    if issued > now + Duration::seconds(30) {
        result.time_status = "future".to_string();
    } else if now >= expires - Duration::seconds(30) {
        result.time_status = "expired_or_near_expiry".to_string();
    } else {
        result.time_status = "current".to_string();
    }
    result.heuristic_usable = matches!(
        result.classification.as_str(),
        "personal_normal" | "team_normal"
    ) && result.time_status == "current";
    result
}

fn classify_length(length: usize) -> String {
    match length {
        292 => "personal_normal".to_string(),
        312 | 356 => "limited_or_degraded".to_string(),
        332 => "team_normal".to_string(),
        _ => "other_length".to_string(),
    }
}

fn classify_blocks_and_length(blocks: usize, length: usize) -> String {
    match (blocks, length) {
        (10, 292) => "personal_normal".to_string(),
        (12, 332) => "team_normal".to_string(),
        (11, 312) | (13, 356) => "limited_or_degraded".to_string(),
        _ => classify_length(length),
    }
}

fn short_hash(value: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(value.as_bytes());
    digest
        .finalize()
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::URL_SAFE, Engine as _};

    fn sample(blocks: usize, issued: u64) -> String {
        let mut raw = vec![0x80];
        raw.extend_from_slice(&issued.to_be_bytes());
        raw.extend(std::iter::repeat_n(0u8, 48 + blocks * 16));
        URL_SAFE.encode(raw)
    }

    #[test]
    fn classifies_known_envelopes_without_retaining_value() {
        let value = sample(10, Utc::now().timestamp() as u64);
        let observation = inspect(&value, Some("account"), Some("model"), None, "http", 200);
        assert_eq!(observation.classification, "personal_normal");
        assert!(observation.envelope_ok);
        assert_eq!(observation.length, 292);
        assert!(observation.heuristic_usable);
        assert!(!observation.fingerprint.is_empty());
    }

    #[test]
    fn malformed_values_are_classified_without_panicking() {
        let observation = inspect("not a state", None, None, None, "http", 200);
        assert_eq!(observation.classification, "malformed");
        assert!(!observation.envelope_ok);
    }

    #[test]
    fn monitor_is_bounded_and_counts_classes() {
        let monitor = TurnStateMonitor::new();
        for _ in 0..(MAX_RECENT + 5) {
            monitor.observe("bad", Some("account"), Some("model"), None, "http", 200);
        }
        let status = monitor.status();
        assert_eq!(status.total_observations, (MAX_RECENT + 5) as u64);
        assert_eq!(status.recent.len(), MAX_RECENT);
        assert_eq!(
            status.by_classification.get("malformed"),
            Some(&((MAX_RECENT + 5) as u64))
        );
    }
}
