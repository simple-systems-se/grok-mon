use super::auth::{AuthError, Credentials, load_credentials};
use chrono::{DateTime, Utc};
use serde_json::Value;

const MANAGEMENT_URL: &str = "https://management-api.x.ai";
const USER_AGENT: &str = "cosmic-ext-applet-grok-monitor-api";
/// Ring and color treat $50 remaining as a full prepaid wallet.
pub const RING_FULL_CENTS: i64 = 5_000;

#[derive(Debug, Clone)]
pub struct ApiToken {
    pub name: String,
    pub redacted: Option<String>,
    pub disabled: bool,
}

#[derive(Debug, Clone)]
pub struct ApiSnapshot {
    pub remaining_cents: i64,
    pub used_cents: Option<i64>,
    pub team_id: String,
    pub key_name: Option<String>,
    pub tokens: Option<Vec<ApiToken>>,
    pub fetched_at: DateTime<Utc>,
}

impl ApiSnapshot {
    pub fn remaining_percent(&self) -> f32 {
        remaining_percent(self.remaining_cents)
    }

    pub fn used_percent(&self) -> f32 {
        100.0 - self.remaining_percent()
    }
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

pub fn remaining_percent(remaining_cents: i64) -> f32 {
    let rem = remaining_cents.max(0) as f32;
    (rem / RING_FULL_CENTS as f32 * 100.0).clamp(0.0, 100.0)
}

pub fn remaining_cents_from_ledger(val: i64) -> i64 {
    -val
}

pub fn format_usd(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    let dollars = abs / 100;
    let rem = abs % 100;
    if rem == 0 {
        format!("{sign}${dollars}")
    } else {
        format!("{sign}${dollars}.{rem:02}")
    }
}

pub fn parse_snapshot(
    validation: &[u8],
    balance: Option<&[u8]>,
    preview: Option<&[u8]>,
    keys: Option<&[u8]>,
    configured_team: Option<&str>,
) -> Result<ApiSnapshot, FetchError> {
    let validation: Value =
        serde_json::from_slice(validation).map_err(|e| FetchError::Parse(e.to_string()))?;
    let team_id = resolve_team_id(&validation, configured_team)?;
    let key_name = json_string(&validation, "name").filter(|s| !s.is_empty());

    let preview_val = preview.and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok());
    let (preview_remaining, preview_used) = preview_val
        .as_ref()
        .map(parse_preview)
        .unwrap_or((None, None));

    let posted = match balance {
        Some(bytes) => match parse_balance(bytes) {
            Ok(v) => Some(v),
            Err(err) => {
                if preview_remaining.is_none() && preview_used.is_none() {
                    return Err(err);
                }
                tracing::debug!("prepaid balance parse skipped: {err}");
                None
            }
        },
        None => None,
    };

    let remaining_cents = live_remaining_cents(posted, preview_remaining, preview_used)?;

    let tokens = keys.and_then(|bytes| {
        serde_json::from_slice::<Value>(bytes)
            .ok()
            .map(|v| parse_keys(&v))
    });

    Ok(ApiSnapshot {
        remaining_cents,
        used_cents: preview_used,
        team_id,
        key_name,
        tokens,
        fetched_at: Utc::now(),
    })
}

fn resolve_team_id(validation: &Value, configured: Option<&str>) -> Result<String, FetchError> {
    if let Some(id) = configured.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(id.to_string());
    }
    let scope = json_string(validation, "scope").unwrap_or_default();
    if scope.eq_ignore_ascii_case("SCOPE_ORGANIZATION") {
        return Err(FetchError::Parse(
            "organization key — set team_id in credentials.json or XAI_TEAM_ID".into(),
        ));
    }
    json_string(validation, "scopeId")
        .or_else(|| json_string(validation, "scope_id"))
        .or_else(|| json_string(validation, "teamId"))
        .or_else(|| json_string(validation, "team_id"))
        .filter(|s| !s.is_empty())
        .ok_or_else(|| FetchError::Parse("validation response missing team id".into()))
}

fn parse_balance(bytes: &[u8]) -> Result<i64, FetchError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|e| FetchError::Parse(e.to_string()))?;
    let total = value
        .get("total")
        .ok_or_else(|| FetchError::Parse("missing prepaid total".into()))?;
    let val = parse_cent(total).ok_or_else(|| FetchError::Parse("invalid prepaid total".into()))?;
    Ok(remaining_cents_from_ledger(val))
}

/// Live remaining is posted prepaid ledger minus current-period spend.
///
/// `prepaidCredits` on the invoice preview can lag behind live spend. The
/// posted ledger does not deduct mid-cycle either, so remaining is
/// `posted - used` when both are present. Preview remaining is only a
/// fallback when the ledger is missing.
pub fn live_remaining_cents(
    posted: Option<i64>,
    preview_remaining: Option<i64>,
    used: Option<i64>,
) -> Result<i64, FetchError> {
    match (posted, used) {
        (Some(posted), Some(used)) => Ok(posted - used),
        (Some(posted), None) => Ok(preview_remaining.unwrap_or(posted)),
        (None, _) => preview_remaining
            .ok_or_else(|| FetchError::Parse("no prepaid remaining in billing response".into())),
    }
}

fn parse_preview(preview: &Value) -> (Option<i64>, Option<i64>) {
    let invoice = preview
        .get("coreInvoice")
        .or_else(|| preview.get("core_invoice"));
    let Some(invoice) = invoice else {
        return (None, None);
    };
    let remaining = invoice
        .get("prepaidCredits")
        .or_else(|| invoice.get("prepaid_credits"))
        .and_then(parse_cent)
        .map(remaining_cents_from_ledger);
    (remaining, preview_spend(invoice))
}

fn preview_spend(invoice: &Value) -> Option<i64> {
    let used_field = invoice
        .get("prepaidCreditsUsed")
        .or_else(|| invoice.get("prepaid_credits_used"))
        .and_then(parse_cent)
        .map(i64::saturating_abs);
    let total = invoice
        .get("totalWithCorr")
        .or_else(|| invoice.get("total_with_corr"))
        .and_then(parse_cent)
        .map(i64::saturating_abs);
    let lines = line_spend(invoice);
    [used_field, total, lines].into_iter().flatten().max()
}

fn line_spend(invoice: &Value) -> Option<i64> {
    let lines = invoice.get("lines")?.as_array()?;
    let mut sum = 0i64;
    let mut any = false;
    for line in lines {
        if let Some(amount) = line.get("amount").and_then(parse_cent) {
            sum = sum.saturating_add(amount.saturating_abs());
            any = true;
        }
    }
    any.then_some(sum)
}

fn parse_keys(value: &Value) -> Vec<ApiToken> {
    let Some(list) = value
        .get("apiKeys")
        .or_else(|| value.get("api_keys"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|item| {
            let redacted = json_string(item, "redactedApiKey")
                .or_else(|| json_string(item, "redacted_api_key"))
                .filter(|s| !s.is_empty());
            let name = json_string(item, "name")
                .filter(|s| !s.is_empty())
                .or_else(|| redacted.clone())
                .or_else(|| {
                    json_string(item, "apiKeyId").or_else(|| json_string(item, "api_key_id"))
                })
                .filter(|s| !s.is_empty())?;
            let disabled = json_bool(item, "disabled").unwrap_or(false);
            Some(ApiToken {
                name,
                redacted,
                disabled,
            })
        })
        .collect()
}

fn parse_cent(value: &Value) -> Option<i64> {
    match value {
        // Protobuf JSON omits default fields; `{"total":{}}` is 0 cents.
        Value::Object(map) => map.get("val").map_or(Some(0), parse_cent),
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f.round() as i64)),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn json_string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn json_bool(value: &Value, key: &str) -> Option<bool> {
    let v = value.get(key)?;
    match v {
        Value::Bool(b) => Some(*b),
        Value::String(s) => match s.to_ascii_lowercase().as_str() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        },
        Value::Number(n) => n.as_i64().map(|n| n != 0),
        _ => None,
    }
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
    Ok(headers)
}

async fn get_json(
    client: &reqwest::Client,
    headers: reqwest::header::HeaderMap,
    path: &str,
) -> Result<Vec<u8>, FetchError> {
    let url = format!("{MANAGEMENT_URL}{path}");
    let response = client
        .get(url)
        .headers(headers)
        .send()
        .await
        .map_err(|e| FetchError::Http(e.to_string()))?;
    let status = response.status();
    let body = response
        .bytes()
        .await
        .map_err(|e| FetchError::Http(e.to_string()))?;
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(FetchError::Auth(AuthError::Rejected));
    }
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(FetchError::Http("team not found".into()));
    }
    if !status.is_success() {
        return Err(FetchError::Http(format!("management HTTP {status}")));
    }
    Ok(body.to_vec())
}

pub async fn fetch_api_usage() -> Result<ApiSnapshot, FetchError> {
    let creds = load_credentials().map_err(FetchError::Auth)?;
    match tokio::time::timeout(
        std::time::Duration::from_secs(20),
        fetch_with_credentials(&creds),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err(FetchError::Http("management API timed out".into())),
    }
}

async fn fetch_with_credentials(creds: &Credentials) -> Result<ApiSnapshot, FetchError> {
    let client = http_client()?;
    let headers = auth_headers(&creds.key)?;
    let validation = get_json(&client, headers.clone(), "/auth/management-keys/validation").await?;

    let team_id = resolve_team_id(
        &serde_json::from_slice(&validation).map_err(|e| FetchError::Parse(e.to_string()))?,
        creds.team_id.as_deref(),
    )?;

    let balance_path = format!("/v1/billing/teams/{team_id}/prepaid/balance");
    let preview_path = format!("/v1/billing/teams/{team_id}/postpaid/invoice/preview");
    let keys_path = format!("/auth/teams/{team_id}/api-keys?activeOnly=true");
    let (balance, preview, keys) = tokio::join!(
        get_json(&client, headers.clone(), &balance_path),
        get_json(&client, headers.clone(), &preview_path),
        get_json(&client, headers.clone(), &keys_path),
    );

    let (balance_bytes, preview_bytes) = merge_billing_bodies(balance, preview)?;
    let keys_bytes = match keys {
        Ok(body) => Some(body),
        Err(err) => {
            tracing::debug!("api key list skipped: {err}");
            None
        }
    };

    let snapshot = parse_snapshot(
        &validation,
        balance_bytes.as_deref(),
        preview_bytes.as_deref(),
        keys_bytes.as_deref(),
        creds.team_id.as_deref(),
    )?;
    tracing::info!(
        remaining_cents = snapshot.remaining_cents,
        used_cents = snapshot.used_cents,
        "fetched xAI API usage"
    );
    Ok(snapshot)
}

type BillingBodies = (Option<Vec<u8>>, Option<Vec<u8>>);

fn merge_billing_bodies(
    balance: Result<Vec<u8>, FetchError>,
    preview: Result<Vec<u8>, FetchError>,
) -> Result<BillingBodies, FetchError> {
    match (balance, preview) {
        (Ok(balance), Ok(preview)) => Ok((Some(balance), Some(preview))),
        (Ok(balance), Err(err)) => {
            tracing::debug!("invoice preview skipped: {err}");
            Ok((Some(balance), None))
        }
        (Err(err), Ok(preview)) => {
            tracing::debug!("prepaid balance skipped: {err}");
            Ok((None, Some(preview)))
        }
        (Err(first), Err(second)) => {
            tracing::debug!("prepaid balance failed: {first}; invoice preview failed: {second}");
            Err(first)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_negates_cents() {
        assert_eq!(remaining_cents_from_ledger(-1000), 1000);
        assert_eq!(remaining_cents_from_ledger(0), 0);
        assert_eq!(remaining_cents_from_ledger(50), -50);
    }

    #[test]
    fn format_usd_compact() {
        assert_eq!(format_usd(1234), "$12.34");
        assert_eq!(format_usd(1200), "$12");
        assert_eq!(format_usd(0), "$0");
        assert_eq!(format_usd(-4), "-$0.04");
        assert_eq!(format_usd(10_000), "$100");
    }

    #[test]
    fn remaining_percent_caps_at_full() {
        assert_eq!(remaining_percent(0), 0.0);
        assert!((remaining_percent(2500) - 50.0).abs() < f32::EPSILON);
        assert_eq!(remaining_percent(RING_FULL_CENTS), 100.0);
        assert_eq!(remaining_percent(RING_FULL_CENTS * 4), 100.0);
        assert_eq!(remaining_percent(-100), 0.0);
    }

    #[test]
    fn parse_live_remaining_is_posted_minus_used() {
        let validation = include_bytes!("../../tests/fixtures/api_validation.json");
        let balance = include_bytes!("../../tests/fixtures/api_balance.json");
        let preview = include_bytes!("../../tests/fixtures/api_preview.json");
        let keys = include_bytes!("../../tests/fixtures/api_keys.json");
        let snap =
            parse_snapshot(validation, Some(balance), Some(preview), Some(keys), None).unwrap();
        assert_eq!(snap.remaining_cents, 890);
        assert_eq!(snap.used_cents, Some(344));
        assert_eq!(snap.team_id, "65c1e471-205f-4566-9c5a-07198bcdf4ce");
        assert_eq!(snap.key_name.as_deref(), Some("grok-mon"));
        let tokens = snap.tokens.expect("key list");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].name, "prod");
        assert!(tokens[1].disabled);
    }

    #[test]
    fn lagging_prepaid_credits_uses_posted_minus_spend() {
        let validation = include_bytes!("../../tests/fixtures/api_validation.json");
        let balance = include_bytes!("../../tests/fixtures/api_balance_posted.json");
        let preview = include_bytes!("../../tests/fixtures/api_preview_lagging.json");
        let snap = parse_snapshot(validation, Some(balance), Some(preview), None, None).unwrap();
        // Posted $100, invoice spend $2.20. prepaidCredits only moved $0.36.
        assert_eq!(snap.remaining_cents, 9780);
        assert_eq!(snap.used_cents, Some(220));
        assert_eq!(format_usd(snap.remaining_cents), "$97.80");
    }

    #[test]
    fn live_remaining_prefers_posted_minus_used() {
        assert_eq!(
            live_remaining_cents(Some(10_000), Some(9964), Some(220)).unwrap(),
            9780
        );
        assert_eq!(
            live_remaining_cents(Some(1234), Some(890), Some(344)).unwrap(),
            890
        );
        assert_eq!(
            live_remaining_cents(None, Some(890), Some(344)).unwrap(),
            890
        );
        assert_eq!(live_remaining_cents(Some(1234), None, None).unwrap(), 1234);
        assert!(live_remaining_cents(None, None, Some(220)).is_err());
    }

    #[test]
    fn parse_cent_empty_object_is_zero() {
        assert_eq!(parse_balance(br#"{"total":{}}"#).unwrap(), 0);
        assert!(parse_balance(br#"{}"#).is_err());
        assert_eq!(parse_cent(&serde_json::json!({"val":"-220"})), Some(-220));
        assert_eq!(parse_cent(&serde_json::json!("40")), Some(40));
    }

    #[test]
    fn parse_keeps_preview_when_balance_invalid() {
        let validation = include_bytes!("../../tests/fixtures/api_validation.json");
        let preview = include_bytes!("../../tests/fixtures/api_preview.json");
        let snap = parse_snapshot(validation, Some(br#"{}"#), Some(preview), None, None).unwrap();
        assert_eq!(snap.remaining_cents, 890);
        assert_eq!(snap.used_cents, Some(344));
        assert!(snap.tokens.is_none());
    }

    #[test]
    fn parse_balance_when_preview_omitted() {
        let validation = include_bytes!("../../tests/fixtures/api_validation.json");
        let balance = include_bytes!("../../tests/fixtures/api_balance.json");
        let snap = parse_snapshot(validation, Some(balance), None, None, None).unwrap();
        assert_eq!(snap.remaining_cents, 1234);
        assert_eq!(snap.used_cents, None);
        assert!(snap.tokens.is_none());
    }

    #[test]
    fn parse_zero_total_is_empty_wallet() {
        let validation = include_bytes!("../../tests/fixtures/api_validation.json");
        let balance = include_bytes!("../../tests/fixtures/api_balance_empty.json");
        let snap = parse_snapshot(validation, Some(balance), None, None, None).unwrap();
        assert_eq!(snap.remaining_cents, 0);
    }

    #[test]
    fn missing_total_is_error() {
        let validation = include_bytes!("../../tests/fixtures/api_validation.json");
        let err = parse_snapshot(validation, Some(br#"{}"#), None, None, None).unwrap_err();
        assert!(matches!(err, FetchError::Parse(_)));
    }

    #[test]
    fn org_scope_requires_configured_team() {
        let validation = br#"{
            "scope": "SCOPE_ORGANIZATION",
            "scopeId": "org-1",
            "name": "org key"
        }"#;
        let err = parse_snapshot(
            validation,
            Some(br#"{"total":{"val":"0"}}"#),
            None,
            None,
            None,
        )
        .unwrap_err();
        assert!(matches!(err, FetchError::Parse(msg) if msg.contains("team_id")));
        let snap = parse_snapshot(
            validation,
            Some(br#"{"total":{"val":"-500"}}"#),
            None,
            None,
            Some("team-from-file"),
        )
        .unwrap();
        assert_eq!(snap.team_id, "team-from-file");
        assert_eq!(snap.remaining_cents, 500);
    }

    #[test]
    fn merge_billing_keeps_one_success() {
        let ok = Ok(b"ok".to_vec());
        let auth = Err(FetchError::Auth(AuthError::Rejected));
        let http = Err(FetchError::Http("management HTTP 500".into()));
        let (balance, preview) = merge_billing_bodies(ok.clone(), auth).unwrap();
        assert_eq!(balance.as_deref(), Some(b"ok".as_slice()));
        assert!(preview.is_none());
        let err =
            merge_billing_bodies(Err(FetchError::Auth(AuthError::Rejected)), http).unwrap_err();
        assert!(matches!(err, FetchError::Auth(AuthError::Rejected)));
        let (balance, preview) =
            merge_billing_bodies(Err(FetchError::Http("missing".into())), ok).unwrap();
        assert!(balance.is_none());
        assert_eq!(preview.as_deref(), Some(b"ok".as_slice()));
    }
}
