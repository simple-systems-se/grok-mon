use super::secrets::AuthError;
use chrono::{DateTime, Utc};
use serde_json::Value;

#[cfg_attr(not(test), allow(dead_code))]
const NO_LIMIT_SENTINEL_CENTS: f64 = 2_147_483_647.0;

#[derive(Debug, Clone)]
pub struct BotSnapshot {
    pub percent: f32,
    pub enterprise: bool,
    pub used_cents: Option<f64>,
    pub limit_cents: Option<f64>,
    pub resets_at: Option<DateTime<Utc>>,
    pub plan: Option<String>,
    pub email: Option<String>,
    pub trial_expires_at: Option<DateTime<Utc>>,
    pub fetched_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub enum FetchError {
    Auth(AuthError),
    #[cfg_attr(not(test), allow(dead_code))]
    Parse(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auth(e) => write!(f, "{e}"),
            Self::Parse(e) => write!(f, "{e}"),
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn parse_usage_json(
    status_bytes: &[u8],
    period_bytes: Option<&[u8]>,
) -> Result<BotSnapshot, FetchError> {
    let status: Value =
        serde_json::from_slice(status_bytes).map_err(|e| FetchError::Parse(e.to_string()))?;
    let obj = status
        .as_object()
        .ok_or_else(|| FetchError::Parse("status not an object".into()))?;

    let enterprise = field(
        obj,
        "usesPooledEnterpriseAllowance",
        "uses_pooled_enterprise_allowance",
    )
    .and_then(Value::as_bool)
    .unwrap_or(false);

    let resets_at =
        field(obj, "nextResetTimestampUtc", "next_reset_timestamp_utc").and_then(parse_timestamp);
    let trial_expires_at =
        field(obj, "sandTrialExpiresAt", "sand_trial_expires_at").and_then(parse_timestamp);

    let percent = field(obj, "usagePercent", "usage_percent").and_then(json_f64);

    let (percent, enterprise) = if enterprise {
        (0.0, true)
    } else if let Some(p) = percent {
        (clamp_percent(p as f32), false)
    } else if resets_at.is_some() {
        (0.0, false)
    } else {
        return Err(FetchError::Parse("no usage fields".into()));
    };

    let mut used_cents = None;
    let mut limit_cents = None;
    if let Some(period) = period_bytes
        && let Ok(period_val) = serde_json::from_slice::<Value>(period)
        && let Some((used, limit)) = parse_ondemand(&period_val)
    {
        used_cents = Some(used);
        limit_cents = Some(limit);
    }

    let plan = field(obj, "grokPlanLabel", "grok_plan_label")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    Ok(BotSnapshot {
        percent,
        enterprise,
        used_cents,
        limit_cents,
        resets_at,
        plan,
        email: None,
        trial_expires_at,
        fetched_at: Utc::now(),
    })
}

#[cfg_attr(not(test), allow(dead_code))]
fn parse_ondemand(period: &Value) -> Option<(f64, f64)> {
    let spend = field_value(period, "spendLimitUsage", "spend_limit_usage")?;
    let limit = field_value(spend, "individualLimit", "individual_limit").and_then(json_f64)?;
    if !limit.is_finite() || limit <= 0.0 || limit >= NO_LIMIT_SENTINEL_CENTS {
        return None;
    }
    let used = field_value(spend, "individualUsed", "individual_used")
        .and_then(json_f64)
        .unwrap_or(0.0);
    Some((used, limit))
}

#[cfg_attr(not(test), allow(dead_code))]
fn field<'a>(
    obj: &'a serde_json::Map<String, Value>,
    camel: &str,
    snake: &str,
) -> Option<&'a Value> {
    obj.get(camel).or_else(|| obj.get(snake))
}

#[cfg_attr(not(test), allow(dead_code))]
fn field_value<'a>(value: &'a Value, camel: &str, snake: &str) -> Option<&'a Value> {
    value.as_object().and_then(|o| field(o, camel, snake))
}

#[cfg_attr(not(test), allow(dead_code))]
fn json_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn parse_timestamp(value: &Value) -> Option<DateTime<Utc>> {
    match value {
        Value::String(s) => DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|d| d.with_timezone(&Utc)),
        Value::Object(m) => {
            let seconds = field(m, "seconds", "seconds").and_then(json_f64)? as i64;
            let nanos = field(m, "nanos", "nanos").and_then(json_f64).unwrap_or(0.0) as u32;
            DateTime::from_timestamp(seconds, nanos)
        }
        Value::Number(n) => {
            let n = n.as_f64()?;
            if n > 1_000_000_000_000.0 {
                DateTime::from_timestamp_millis(n as i64)
            } else {
                DateTime::from_timestamp(n as i64, 0)
            }
        }
        _ => None,
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn clamp_percent(p: f32) -> f32 {
    if !p.is_finite() {
        0.0
    } else {
        p.clamp(0.0, 100.0)
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn create_cursor_checksum(machine_id: &str, now_ms: u128) -> String {
    let unix_kilo_seconds = now_ms / 1_000_000;
    let mut bytes = [
        ((unix_kilo_seconds >> 40) & 0xff) as u8,
        ((unix_kilo_seconds >> 32) & 0xff) as u8,
        ((unix_kilo_seconds >> 24) & 0xff) as u8,
        ((unix_kilo_seconds >> 16) & 0xff) as u8,
        ((unix_kilo_seconds >> 8) & 0xff) as u8,
        (unix_kilo_seconds & 0xff) as u8,
    ];
    let mut last = 165u8;
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = (*byte ^ last).wrapping_add((i % 256) as u8);
        last = *byte;
    }
    format!(
        "{}{machine_id}",
        data_encoding::BASE64URL_NOPAD.encode(&bytes)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_weekly_percent() {
        let json = include_bytes!("../../tests/fixtures/bot_usage.json");
        let snap = parse_usage_json(json, None).unwrap();
        assert!((snap.percent - 23.0).abs() < f32::EPSILON);
        assert!(!snap.enterprise);
        assert!(snap.resets_at.is_some());
        assert!(snap.trial_expires_at.is_none());
        assert_eq!(snap.plan.as_deref(), Some("Grok Bot Plan"));
    }

    #[test]
    fn parse_omitted_percent_is_zero() {
        let json = include_bytes!("../../tests/fixtures/bot_usage_new_period.json");
        let snap = parse_usage_json(json, None).unwrap();
        assert_eq!(snap.percent, 0.0);
        assert!(snap.resets_at.is_some());
    }

    #[test]
    fn parse_enterprise_is_na() {
        let json = include_bytes!("../../tests/fixtures/bot_usage_enterprise.json");
        let snap = parse_usage_json(json, None).unwrap();
        assert!(snap.enterprise);
        assert_eq!(snap.percent, 0.0);
    }

    #[test]
    fn parse_ondemand_cents() {
        let status = include_bytes!("../../tests/fixtures/bot_usage.json");
        let period = include_bytes!("../../tests/fixtures/bot_ondemand.json");
        let snap = parse_usage_json(status, Some(period)).unwrap();
        assert_eq!(snap.used_cents, Some(250.0));
        assert_eq!(snap.limit_cents, Some(1000.0));
    }

    #[test]
    fn checksum_is_stable_for_fixed_time() {
        let sum = create_cursor_checksum("machine-id", 1_000_000_000_000);
        assert!(sum.ends_with("machine-id"));
        assert_eq!(sum, create_cursor_checksum("machine-id", 1_000_000_000_000));
        assert_ne!(sum, create_cursor_checksum("machine-id", 2_000_000_000_000));
    }

    #[test]
    fn timestamp_object_and_rfc3339() {
        let rfc = serde_json::json!("2026-08-27T19:18:19Z");
        assert!(parse_timestamp(&rfc).is_some());
        let obj = serde_json::json!({"seconds": "1787843899", "nanos": 0});
        assert!(parse_timestamp(&obj).is_some());
    }

    #[test]
    fn weekly_pace_from_reset() {
        use crate::pace::maybe_weekly_pace;
        let json = include_bytes!("../../tests/fixtures/bot_usage.json");
        let snap = parse_usage_json(json, None).unwrap();
        let end = snap.resets_at.unwrap();
        let mid = end - chrono::Duration::days(3) - chrono::Duration::hours(12);
        let pace = maybe_weekly_pace(80.0, mid, None, snap.resets_at, Some("WEEKLY")).unwrap();
        assert_eq!(pace.popup_line(), "Will run out in 21h");
    }
}
