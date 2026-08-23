use crate::auth::{AuthError, AuthIdentity, load_bearer};
use chrono::{DateTime, Utc};
use serde::Deserialize;

const BILLING_URL: &str = "https://cli-chat-proxy.grok.com/v1/billing?format=credits";
const SETTINGS_URL: &str = "https://cli-chat-proxy.grok.com/v1/settings";
const CLIENT_ID: &str = "grok-shell";
const TOKEN_AUTH: &str = "xai-grok-cli";
const USER_AGENT: &str = "cosmic-ext-applet-grok-monitor";

#[derive(Debug, Clone)]
pub struct UsageSnapshot {
    pub percent: f32,
    pub used: Option<f64>,
    pub limit: Option<f64>,
    pub resets_at: Option<DateTime<Utc>>,
    pub period_type: Option<String>,
    pub plan: Option<String>,
    pub email: Option<String>,
    pub fetched_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub enum FetchError {
    Auth(AuthError),
    Http(String),
    Parse(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auth(e) => write!(f, "{e}"),
            Self::Http(e) | Self::Parse(e) => write!(f, "{e}"),
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
}

#[derive(Deserialize)]
struct CurrentPeriod {
    #[serde(rename = "type")]
    period_type: Option<String>,
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

    let period_type = config
        .current_period
        .as_ref()
        .and_then(|p| p.period_type.clone());

    let grok_build_percent = config.product_usage.as_ref().and_then(|items| {
        items.iter().find_map(|item| {
            if item.product.as_deref() == Some("GrokBuild") {
                item.usage_percent
            } else {
                None
            }
        })
    });

    let (percent, used, limit) = if let Some(p) = config.credit_usage_percent.or(grok_build_percent)
    {
        (clamp_percent(p as f32), None, None)
    } else if let (Some(cap), Some(used_val)) = (
        config.on_demand_cap.as_ref().and_then(|a| a.val),
        config.on_demand_used.as_ref().and_then(|a| a.val),
    ) {
        if cap > 0.0 {
            (
                clamp_percent((used_val / cap * 100.0) as f32),
                Some(used_val),
                Some(cap),
            )
        } else if resets_at.is_some() {
            (0.0, None, None)
        } else {
            return Err(FetchError::Parse("no usage fields".into()));
        }
    } else if resets_at.is_some() {
        (0.0, None, None)
    } else {
        return Err(FetchError::Parse("no usage fields".into()));
    };

    let plan = config
        .subscription_tier
        .or(response.subscription_tier)
        .filter(|s| !s.is_empty());

    Ok(UsageSnapshot {
        percent,
        used,
        limit,
        resets_at,
        period_type,
        plan,
        email: None,
        fetched_at: Utc::now(),
    })
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

pub async fn fetch_usage() -> Result<UsageSnapshot, FetchError> {
    let bearer = load_bearer().map_err(FetchError::Auth)?;
    let client = http_client()?;
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

    if let Ok(plan) = fetch_plan(&client, headers).await
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

pub fn format_remaining(end: DateTime<Utc>) -> String {
    let delta = end - Utc::now();
    let secs = delta.num_seconds();
    if secs <= 0 {
        return "reset pending".into();
    }
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let mins = (secs % 3_600) / 60;
    if days > 0 {
        format!("resets in {days}d {hours}h")
    } else if hours > 0 {
        format!("resets in {hours}h {mins}m")
    } else {
        format!("resets in {mins}m")
    }
}

pub fn period_label(period_type: Option<&str>) -> &'static str {
    match period_type {
        Some(t) if t.contains("WEEKLY") => "Weekly",
        Some(t) if t.contains("MONTHLY") => "Monthly",
        Some(t) if t.contains("DAILY") => "Daily",
        _ => "Credits",
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
        assert!(snap.resets_at.is_some());
        assert_eq!(period_label(snap.period_type.as_deref()), "Weekly");
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
        assert_eq!(period_label(snap.period_type.as_deref()), "Weekly");
        assert!(snap.resets_at.is_some());
    }

    #[test]
    fn format_percent_is_integer() {
        assert_eq!(format_percent(2.0), "2%");
        assert_eq!(format_percent(2.4), "2%");
        assert_eq!(format_percent(23.4), "23%");
    }
}
