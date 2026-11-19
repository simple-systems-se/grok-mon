use crate::auth::{AuthError, AuthIdentity, Bearer, load_bearer_from};
use chrono::{DateTime, Utc};
use futures_util::future::join_all;
use serde::Deserialize;
use std::path::PathBuf;

const BILLING_URL: &str = "https://cli-chat-proxy.grok.com/v1/billing?format=credits";
const SETTINGS_URL: &str = "https://cli-chat-proxy.grok.com/v1/settings";
const CLIENT_ID: &str = "grok-shell";
const TOKEN_AUTH: &str = "xai-grok-cli";
const USER_AGENT: &str = "cosmic-ext-applet-grok-monitor";

#[derive(Debug, Clone)]
pub struct UsageSnapshot {
    pub account_id: String,
    pub percent: f32,
    pub used: Option<f64>,
    /// On-demand cap, when the payload has one. The popup does not print it.
    #[allow(dead_code)]
    pub limit: Option<f64>,
    pub resets_at: Option<DateTime<Utc>>,
    pub starts_at: Option<DateTime<Utc>>,
    /// Billing window kind from the payload. The popup does not print it.
    #[allow(dead_code)]
    pub period_type: Option<String>,
    pub plan: Option<String>,
    pub email: Option<String>,
    /// When this reading was parsed. The popup does not print it.
    #[allow(dead_code)]
    pub fetched_at: DateTime<Utc>,
    /// `creditUsagePercent` came from a unified weekly pool. Product rows are
    /// a breakdown of that same pool and were not added.
    pub unified: bool,
}

#[derive(Debug, Clone)]
pub struct IconTarget {
    pub id: String,
    pub label: String,
    pub cli_home: PathBuf,
}

#[derive(Debug, Clone)]
pub struct IconUsage {
    pub id: String,
    pub result: Result<UsageSnapshot, FetchError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    Auth(AuthError),
    Http(String),
    Parse(String),
    Config(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auth(e) => write!(f, "{e}"),
            Self::Http(e) | Self::Parse(e) | Self::Config(e) => write!(f, "{e}"),
        }
    }
}

#[derive(Deserialize)]
struct CreditsResponse {
    config: Option<CreditsConfig>,
    #[serde(rename = "subscriptionTier")]
    subscription_tier: Option<String>,
}

#[derive(Deserialize)]
struct CreditsConfig {
    #[serde(rename = "creditUsagePercent")]
    credit_usage_percent: Option<f64>,
    #[serde(rename = "currentPeriod")]
    current_period: Option<CurrentPeriod>,
    #[serde(rename = "billingPeriodStart")]
    billing_period_start: Option<String>,
    #[serde(rename = "billingPeriodEnd")]
    billing_period_end: Option<String>,
    #[serde(rename = "onDemandCap")]
    on_demand_cap: Option<CreditsAmount>,
    #[serde(rename = "onDemandUsed")]
    on_demand_used: Option<CreditsAmount>,
    #[serde(rename = "subscriptionTier")]
    subscription_tier: Option<String>,
    #[serde(rename = "productUsage")]
    product_usage: Option<Vec<ProductUsage>>,
    /// Shared weekly/monthly pool across Grok products, including Grok Bot
    /// after account unification. Absent on the legacy Build-only shape.
    #[serde(default, rename = "isUnifiedBillingUser")]
    is_unified_billing_user: Option<bool>,
}

#[derive(Deserialize)]
struct CurrentPeriod {
    #[serde(rename = "type")]
    period_type: Option<String>,
    start: Option<String>,
    end: Option<String>,
}

#[derive(Deserialize)]
struct CreditsAmount {
    val: Option<f64>,
}

#[derive(Deserialize)]
struct ProductUsage {
    product: Option<String>,
    #[serde(rename = "usagePercent")]
    usage_percent: Option<f64>,
}

#[derive(Deserialize)]
struct SettingsResponse {
    subscription_tier_display: Option<String>,
}

pub fn parse_credits_json(bytes: &[u8]) -> Result<UsageSnapshot, FetchError> {
    let response: CreditsResponse =
        serde_json::from_slice(bytes).map_err(|e| FetchError::Parse(e.to_string()))?;
    let config = response
        .config
        .ok_or_else(|| FetchError::Parse("missing config".into()))?;

    let resets_at = config
        .current_period
        .as_ref()
        .and_then(|p| p.end.as_deref())
        .or(config.billing_period_end.as_deref())
        .and_then(parse_rfc3339);

    let starts_at = config
        .current_period
        .as_ref()
        .and_then(|p| p.start.as_deref())
        .or(config.billing_period_start.as_deref())
        .and_then(parse_rfc3339);

    let period_type = config
        .current_period
        .as_ref()
        .and_then(|p| p.period_type.clone());

    let build_percent = config.product_usage.as_ref().and_then(|items| {
        items.iter().find_map(|item| {
            if item.product.as_deref() == Some("GrokBuild") {
                item.usage_percent
            } else {
                None
            }
        })
    });
    let on_demand = config.on_demand_cap.as_ref().and_then(|cap| {
        let cap = cap.val?;
        let used = config
            .on_demand_used
            .as_ref()
            .and_then(|amount| amount.val)?;
        Some((used, cap))
    });
    // `creditUsagePercent` is the shared pool. `productUsage` repeats slices of
    // that same pool (Build, Chat, and the rest). Those slices are not added
    // to the headline, and the old Grok Bot ledger is not consulted here.
    let unified = config.is_unified_billing_user.unwrap_or(false);
    let (percent, used, limit) = select_percent(
        unified,
        config.credit_usage_percent,
        build_percent,
        on_demand,
        resets_at.is_some(),
    )?;

    let plan = config
        .subscription_tier
        .or(response.subscription_tier)
        .filter(|s| !s.is_empty());

    Ok(UsageSnapshot {
        account_id: String::new(),
        percent,
        used,
        limit,
        resets_at,
        starts_at,
        period_type,
        plan,
        email: None,
        fetched_at: Utc::now(),
        unified,
    })
}

fn select_percent(
    unified: bool,
    pool: Option<f64>,
    build_percent: Option<f64>,
    on_demand: Option<(f64, f64)>,
    has_period: bool,
) -> Result<(f32, Option<f64>, Option<f64>), FetchError> {
    if let Some(percent) = pool {
        return Ok((clamp_percent(percent as f32), None, None));
    }
    if unified {
        // Headline omitted. Do not sum productUsage, and do not substitute one
        // product slice for the pool.
        if has_period {
            return Ok((0.0, None, None));
        }
        return Err(FetchError::Parse("no usage fields".into()));
    }
    if let Some(percent) = build_percent {
        return Ok((clamp_percent(percent as f32), None, None));
    }
    if let Some((used_val, cap)) = on_demand {
        if cap > 0.0 {
            return Ok((
                clamp_percent((used_val / cap * 100.0) as f32),
                Some(used_val),
                Some(cap),
            ));
        }
        if has_period {
            return Ok((0.0, None, None));
        }
        return Err(FetchError::Parse("no usage fields".into()));
    }
    if has_period {
        return Ok((0.0, None, None));
    }
    Err(FetchError::Parse("no usage fields".into()))
}

fn clamp_percent(p: f32) -> f32 {
    if !p.is_finite() {
        0.0
    } else {
        p.clamp(0.0, 100.0)
    }
}

fn parse_rfc3339(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

fn http_client() -> Result<reqwest::Client, FetchError> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .user_agent(USER_AGENT)
        .https_only(true)
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
        .map_err(|e| FetchError::Http(e.to_string()))
}

fn auth_headers(token: &str) -> Result<reqwest::header::HeaderMap, FetchError> {
    use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue};
    let mut headers = HeaderMap::new();
    let mut authorization = HeaderValue::from_str(&format!("Bearer {token}"))
        .map_err(|_| FetchError::Auth(AuthError::Invalid))?;
    authorization.set_sensitive(true);
    headers.insert(AUTHORIZATION, authorization);
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert(
        "x-grok-client-identifier",
        HeaderValue::from_static(CLIENT_ID),
    );
    headers.insert("x-xai-token-auth", HeaderValue::from_static(TOKEN_AUTH));
    Ok(headers)
}

/// One usage fetch per configured icon, in config order.
///
/// Each icon reads only its own CLI home. A missing session is that icon's
/// result. Icons are not invented from other homes, and product rows are not
/// added onto the shared-pool percent.
pub async fn fetch_configured_usage(icons: Vec<IconTarget>) -> Vec<IconUsage> {
    let mut bearers = Vec::with_capacity(icons.len());
    for icon in &icons {
        bearers.push(load_bearer_from(&icon.cli_home));
    }
    let client = if bearers.iter().any(|bearer| bearer.is_ok()) {
        match http_client() {
            Ok(client) => Some(client),
            Err(err) => {
                let message = err.to_string();
                return icons
                    .into_iter()
                    .zip(bearers)
                    .map(|(icon, bearer)| IconUsage {
                        id: icon.id,
                        result: match bearer {
                            Ok(_) => Err(FetchError::Http(message.clone())),
                            Err(auth) => Err(FetchError::Auth(auth)),
                        },
                    })
                    .collect();
            }
        }
    } else {
        None
    };

    let fetches = icons.into_iter().zip(bearers).map(|(icon, bearer)| {
        let client = client.clone();
        async move {
            let result = match (client, bearer) {
                (Some(client), Ok(bearer)) => fetch_bearer(&client, bearer, &icon.label).await,
                (_, Err(auth)) => Err(FetchError::Auth(auth)),
                (None, Ok(_)) => Err(FetchError::Http("billing client unavailable".into())),
            };
            IconUsage {
                id: icon.id,
                result,
            }
        }
    });
    join_all(fetches).await
}

async fn fetch_bearer(
    client: &reqwest::Client,
    bearer: Bearer,
    label: &str,
) -> Result<UsageSnapshot, FetchError> {
    let identity_email = bearer.identity.email.clone();
    let result = match tokio::time::timeout(
        std::time::Duration::from_secs(20),
        fetch_usage_for(client, &bearer),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err(FetchError::Http("billing timed out".into())),
    };
    result.map(|mut snapshot| {
        snapshot.account_id = label.to_string();
        if snapshot.email.is_none() {
            snapshot.email = identity_email;
        }
        tracing::info!(
            percent = snapshot.percent,
            unified = snapshot.unified,
            "fetched shared Grok usage"
        );
        snapshot
    })
}

async fn fetch_usage_for(
    client: &reqwest::Client,
    bearer: &Bearer,
) -> Result<UsageSnapshot, FetchError> {
    let headers = auth_headers(&bearer.token)?;

    let response = client
        .get(BILLING_URL)
        .headers(headers.clone())
        .send()
        .await
        .map_err(|e| FetchError::Http(e.to_string()))?;
    let status = response.status();
    let body = response
        .bytes()
        .await
        .map_err(|e| FetchError::Http(e.to_string()))?;
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(FetchError::Auth(AuthError::Expired));
    }
    if !status.is_success() {
        return Err(FetchError::Http(format!("billing HTTP {status}")));
    }

    let mut snapshot = parse_credits_json(&body)?;
    snapshot.email = bearer.identity.email.clone();
    apply_team_guard(&bearer.identity, &mut snapshot);

    if let Ok(plan) = fetch_plan(client, headers).await
        && snapshot.plan.is_none()
    {
        snapshot.plan = plan;
    }

    Ok(snapshot)
}

fn apply_team_guard(identity: &AuthIdentity, snapshot: &mut UsageSnapshot) {
    if identity
        .principal_type
        .as_deref()
        .is_some_and(|p| p.eq_ignore_ascii_case("team"))
        && snapshot.percent == 0.0
        && snapshot.used.is_none()
    {
        snapshot.plan = Some("Team usage unavailable".into());
    }
}

async fn fetch_plan(
    client: &reqwest::Client,
    headers: reqwest::header::HeaderMap,
) -> Result<Option<String>, FetchError> {
    let response = client
        .get(SETTINGS_URL)
        .headers(headers)
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
        .map_err(|e| FetchError::Http(e.to_string()))?;
    if !response.status().is_success() {
        return Ok(None);
    }
    let parsed: SettingsResponse = response
        .json()
        .await
        .map_err(|e| FetchError::Parse(e.to_string()))?;
    Ok(parsed.subscription_tier_display)
}

pub fn format_percent(percent: f32) -> String {
    format!("{:.0}%", percent.round())
}

/// Percent of the quota still unused. Non-finite `used` is `0.0` so a bad
/// reading does not draw a full bar. Values outside 0..=100 clamp.
pub fn remaining_percent(used: f32) -> f32 {
    if !used.is_finite() {
        return 0.0;
    }
    (100.0 - used).clamp(0.0, 100.0)
}

/// Whole-number label for the usage bar, e.g. `72% remaining`.
pub fn format_remaining_percent(used: f32) -> String {
    let shown = remaining_percent(used).round();
    format!("{}% remaining", shown as i32)
}

/// Compact time until reset. Day-scale values keep leftover hours and drop
/// minutes. Hour-scale values keep leftover minutes. Under a minute the
/// seconds are shown, so a short remainder is not rounded up to `1m`.
/// Zero or a past reset is `0m`.
pub fn format_reset_duration(delta: chrono::Duration) -> String {
    let secs = delta.num_seconds();
    if secs <= 0 {
        return "0m".into();
    }
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let mins = (secs % 3_600) / 60;
    if days > 0 {
        if hours > 0 {
            format!("{days}d {hours}h")
        } else {
            format!("{days}d")
        }
    } else if hours > 0 {
        if mins > 0 {
            format!("{hours}h {mins}m")
        } else {
            format!("{hours}h")
        }
    } else if mins > 0 {
        format!("{mins}m")
    } else {
        format!("{secs}s")
    }
}

/// How full the time-remaining bar is, and the duration label beside it.
///
/// `fill_percent` is `(resets_at - now) / (resets_at - starts_at)` as a
/// percent, clamped to 0..=100. It is `None` when start and end do not form
/// a positive window (no start, or start that is not before end). The popup
/// draws `None` as an empty bar. A 7-day week is not assumed when start is
/// missing. The label is only the compact duration, or `—` when there is no
/// reset time to measure.
#[derive(Debug, Clone, PartialEq)]
pub struct ResetWindow {
    pub fill_percent: Option<f32>,
    pub label: String,
}

pub fn reset_window(
    now: DateTime<Utc>,
    starts_at: Option<DateTime<Utc>>,
    resets_at: Option<DateTime<Utc>>,
) -> ResetWindow {
    let Some(end) = resets_at else {
        return ResetWindow {
            fill_percent: None,
            label: "—".into(),
        };
    };
    let label = format_reset_duration(end - now);
    let fill_percent = starts_at.and_then(|start| {
        let window_ms = (end - start).num_milliseconds();
        if window_ms <= 0 {
            return None;
        }
        let left_ms = (end - now).num_milliseconds();
        let fraction = (left_ms as f64 / window_ms as f64).clamp(0.0, 1.0);
        Some((fraction * 100.0) as f32)
    });
    ResetWindow {
        fill_percent,
        label,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_weekly_percent() {
        let json = include_bytes!("../tests/fixtures/billing.json");
        let snap = parse_credits_json(json).unwrap();
        assert!((snap.percent - 2.0).abs() < f32::EPSILON);
        assert!(snap.unified);
        assert!(snap.resets_at.is_some());
        assert!(snap.starts_at.is_some());
        assert!(snap.starts_at < snap.resets_at);
        assert!(
            snap.period_type
                .as_deref()
                .is_some_and(|period| period.contains("WEEKLY"))
        );
    }

    #[test]
    fn shared_pool_is_not_the_sum_of_product_rows() {
        let json = include_bytes!("../tests/fixtures/billing_shared_pool.json");
        let snap = parse_credits_json(json).unwrap();
        assert!(snap.unified);
        assert!((snap.percent - 12.5).abs() < f32::EPSILON);
        assert!((snap.percent - (8.0 + 3.0)).abs() > 0.1);
        assert!((snap.percent - (12.5 + 8.0 + 3.0)).abs() > 0.1);
    }

    #[test]
    fn unified_breakdown_without_headline_is_not_summed() {
        let json = include_bytes!("../tests/fixtures/billing_unified_breakdown_only.json");
        let snap = parse_credits_json(json).unwrap();
        assert!(snap.unified);
        assert_eq!(snap.percent, 0.0);
        assert_eq!(snap.used, None);
        assert_ne!(snap.percent, 12.0);
    }

    #[test]
    fn legacy_build_slice_is_used_when_there_is_no_shared_pool() {
        let json = include_bytes!("../tests/fixtures/billing_legacy_build.json");
        let snap = parse_credits_json(json).unwrap();
        assert!(!snap.unified);
        assert!((snap.percent - 8.0).abs() < f32::EPSILON);
        assert_ne!(snap.percent, 13.0);
    }

    #[test]
    fn rejects_unusable_payloads() {
        assert!(parse_credits_json(b"not-json").is_err());
        assert!(parse_credits_json(b"{}").is_err());
        let unified_without_period = br#"{
            "config": {
                "isUnifiedBillingUser": true,
                "productUsage": [
                    {"product": "GrokBuild", "usagePercent": 8},
                    {"product": "GrokChat", "usagePercent": 4}
                ]
            }
        }"#;
        assert!(parse_credits_json(unified_without_period).is_err());
    }

    #[test]
    fn parse_ondemand_ratio() {
        let json = include_bytes!("../tests/fixtures/billing_ondemand.json");
        let snap = parse_credits_json(json).unwrap();
        assert!((snap.percent - 25.0).abs() < f32::EPSILON);
        assert_eq!(snap.used, Some(250.0));
        assert_eq!(snap.limit, Some(1000.0));
    }

    #[test]
    fn parse_omitted_percent_is_zero() {
        let json = include_bytes!("../tests/fixtures/billing_new_period.json");
        let snap = parse_credits_json(json).unwrap();
        assert_eq!(snap.percent, 0.0);
        assert_eq!(snap.used, None);
        assert_eq!(snap.limit, None);
        assert!(
            snap.period_type
                .as_deref()
                .is_some_and(|period| period.contains("WEEKLY"))
        );
        assert!(snap.resets_at.is_some());
    }

    #[test]
    fn format_percent_is_integer() {
        assert_eq!(format_percent(2.0), "2%");
        assert_eq!(format_percent(2.4), "2%");
        assert_eq!(format_percent(23.4), "23%");
    }

    #[test]
    fn weekly_pace_from_fixture() {
        use crate::pace::maybe_weekly_pace;
        let json = include_bytes!("../tests/fixtures/billing.json");
        let snap = parse_credits_json(json).unwrap();
        let start = snap.starts_at.unwrap();
        let mid = start + chrono::Duration::days(3) + chrono::Duration::hours(12);
        let pace = maybe_weekly_pace(
            50.0,
            mid,
            snap.starts_at,
            snap.resets_at,
            snap.period_type.as_deref(),
        )
        .unwrap();
        assert_eq!(
            pace.popup_line(),
            "Won't run out before reset · ~0% left at reset"
        );
    }

    #[test]
    fn configured_icons_keep_their_own_results_in_order() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("grok-mon-fetch-{nanos}"));
        let sim = root.join("sim");
        let pers = root.join("pers");
        std::fs::create_dir_all(&sim).unwrap();
        std::fs::create_dir_all(&pers).unwrap();
        let icons = vec![
            IconTarget {
                id: "0:~/.grok".into(),
                label: "Grok (sim sys)".into(),
                cli_home: sim,
            },
            IconTarget {
                id: "1:~/.grok-personal".into(),
                label: "Grok (pers)".into(),
                cli_home: pers,
            },
        ];
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let fetched = runtime.block_on(fetch_configured_usage(icons));
        assert_eq!(fetched.len(), 2);
        assert_eq!(fetched[0].id, "0:~/.grok");
        assert_eq!(fetched[1].id, "1:~/.grok-personal");
        assert!(matches!(
            fetched[0].result,
            Err(FetchError::Auth(AuthError::Missing))
        ));
        assert!(matches!(
            fetched[1].result,
            Err(FetchError::Auth(AuthError::Missing))
        ));
        let pool = parse_credits_json(include_bytes!("../tests/fixtures/billing_shared_pool.json"))
            .unwrap();
        assert!((pool.percent - 12.5).abs() < f32::EPSILON);
        assert_ne!(pool.percent, 8.0 + 3.0);
        let _ = std::fs::remove_dir_all(&root);
    }

    fn stamp(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn remaining_percent_clamps_and_rejects_non_finite() {
        assert_eq!(remaining_percent(28.0), 72.0);
        assert_eq!(remaining_percent(0.0), 100.0);
        assert_eq!(remaining_percent(100.0), 0.0);
        assert_eq!(remaining_percent(140.0), 0.0);
        assert_eq!(remaining_percent(-20.0), 100.0);
        assert_eq!(remaining_percent(f32::NAN), 0.0);
        assert_eq!(remaining_percent(f32::INFINITY), 0.0);
        assert_eq!(remaining_percent(f32::NEG_INFINITY), 0.0);
        assert!((remaining_percent(23.4) - 76.6).abs() < 0.001);
        assert_eq!(format_remaining_percent(28.0), "72% remaining");
        assert_eq!(format_remaining_percent(23.4), "77% remaining");
        assert_eq!(format_remaining_percent(0.0), "100% remaining");
        assert_eq!(format_remaining_percent(100.0), "0% remaining");
        assert_eq!(format_remaining_percent(99.6), "0% remaining");
        assert_eq!(format_remaining_percent(0.4), "100% remaining");
        assert!(!format_remaining_percent(28.0).contains("reset"));
    }

    #[test]
    fn time_remaining_fraction_clamps_past_reset_and_missing_data() {
        let start = stamp("2026-08-13T19:00:00Z");
        let end = stamp("2026-08-20T19:00:00Z");
        let mid = start + (end - start) / 2;
        let half = reset_window(mid, Some(start), Some(end));
        assert!((half.fill_percent.unwrap() - 50.0).abs() < 0.05, "{half:?}");

        let past = reset_window(end + chrono::Duration::hours(2), Some(start), Some(end));
        assert_eq!(past.fill_percent, Some(0.0));
        assert_eq!(past.label, "0m");

        let at_end = reset_window(end, Some(start), Some(end));
        assert_eq!(at_end.fill_percent, Some(0.0));
        assert_eq!(at_end.label, "0m");

        let before = reset_window(start - chrono::Duration::hours(1), Some(start), Some(end));
        assert_eq!(before.fill_percent, Some(100.0));
        assert_eq!(before.label, "7d 1h");

        let missing_start = reset_window(start, None, Some(end));
        assert_eq!(missing_start.fill_percent, None);
        assert_eq!(missing_start.label, "7d");

        let missing_end = reset_window(start, Some(start), None);
        assert_eq!(missing_end.fill_percent, None);
        assert_eq!(missing_end.label, "—");

        let neither = reset_window(start, None, None);
        assert_eq!(neither.fill_percent, None);
        assert_eq!(neither.label, "—");

        let zero = reset_window(start, Some(end), Some(end));
        assert_eq!(zero.fill_percent, None);
        assert_eq!(zero.label, "7d");

        let inverted = reset_window(start, Some(end), Some(start));
        assert_eq!(inverted.fill_percent, None);
        assert_eq!(inverted.label, "0m");
    }

    #[test]
    fn reset_duration_is_compact_and_has_no_reset_sentence() {
        let hours = chrono::Duration::hours;
        let mins = chrono::Duration::minutes;
        let secs = chrono::Duration::seconds;
        assert_eq!(format_reset_duration(hours(3) + mins(12)), "3h 12m");
        assert_eq!(
            format_reset_duration(chrono::Duration::days(2) + hours(4)),
            "2d 4h"
        );
        assert_eq!(format_reset_duration(chrono::Duration::days(2)), "2d");
        assert_eq!(format_reset_duration(hours(5)), "5h");
        assert_eq!(format_reset_duration(mins(45)), "45m");
        assert_eq!(format_reset_duration(secs(30)), "30s");
        assert_eq!(format_reset_duration(secs(0)), "0m");
        assert_eq!(format_reset_duration(secs(-5)), "0m");
        assert_eq!(
            format_reset_duration(chrono::Duration::days(1) + mins(30)),
            "1d"
        );
        let sample = format_reset_duration(hours(3) + mins(12));
        assert!(!sample.contains("reset"));
        assert!(!sample.contains("remain"));
    }
}
