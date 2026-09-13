use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use zeroize::Zeroize;

#[derive(Debug, Clone)]
pub struct AuthIdentity {
    pub email: Option<String>,
    pub principal_type: Option<String>,
}

#[derive(Debug, Clone)]
pub enum AuthError {
    Missing,
    Expired,
    Invalid,
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => write!(f, "not signed in — run grok login"),
            Self::Expired => write!(f, "session expired — run grok login"),
            Self::Invalid => write!(f, "invalid auth.json"),
        }
    }
}

#[derive(Deserialize)]
struct AuthEntry {
    key: Option<String>,
    email: Option<String>,
    expires_at: Option<String>,
    principal_type: Option<String>,
}

impl Drop for AuthEntry {
    fn drop(&mut self) {
        if let Some(ref mut key) = self.key {
            key.zeroize();
        }
    }
}

pub struct Bearer {
    pub id: String,
    pub token: String,
    pub identity: AuthIdentity,
}

impl Drop for Bearer {
    fn drop(&mut self) {
        self.token.zeroize();
    }
}

pub fn grok_home() -> PathBuf {
    grok_home_from(std::env::var("GROK_HOME").ok())
}

fn grok_home_from(override_home: Option<String>) -> PathBuf {
    match override_home {
        Some(home) if !home.is_empty() => PathBuf::from(home),
        _ => dirs_home().join(".grok"),
    }
}

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Read the current Grok Build OIDC token. Do not log or persist it.
#[allow(dead_code)]
pub fn load_bearer() -> Result<Bearer, AuthError> {
    let mut bearers = load_bearers()?;
    bearers.drain(..).next().ok_or(AuthError::Missing)
}

/// Every unexpired Grok Build token, xAI entries first.
pub fn load_bearers() -> Result<Vec<Bearer>, AuthError> {
    let path = grok_home().join("auth.json");
    let mut raw = fs::read_to_string(&path).map_err(|_| AuthError::Missing)?;
    let result = bearers_from_auth_json(&raw);
    raw.zeroize();
    result
}

#[cfg(test)]
fn bearer_from_auth_json(raw: &str) -> Result<Bearer, AuthError> {
    let mut bearers = bearers_from_auth_json(raw)?;
    bearers.drain(..).next().ok_or(AuthError::Missing)
}

fn bearers_from_auth_json(raw: &str) -> Result<Vec<Bearer>, AuthError> {
    let map: HashMap<String, AuthEntry> =
        serde_json::from_str(raw).map_err(|_| AuthError::Invalid)?;

    let mut xai_fresh = Vec::new();
    let mut xai_stale = 0usize;
    let mut fallback_fresh = Vec::new();
    let mut fallback_stale = 0usize;
    for (id, mut entry) in map {
        if entry.key.as_deref().is_none_or(|k| k.is_empty()) {
            continue;
        }
        let expired = entry_expired(&entry);
        let Some(token) = entry.key.take().filter(|k| !k.is_empty()) else {
            continue;
        };
        let bearer = Bearer {
            id: id.clone(),
            token,
            identity: AuthIdentity {
                email: entry.email.take(),
                principal_type: entry.principal_type.take(),
            },
        };
        if id.starts_with("https://auth.x.ai::") {
            if expired {
                xai_stale += 1;
            } else {
                xai_fresh.push(bearer);
            }
        } else if expired {
            fallback_stale += 1;
        } else {
            fallback_fresh.push(bearer);
        }
    }

    let mut chosen = if !xai_fresh.is_empty() {
        xai_fresh
    } else if !fallback_fresh.is_empty() {
        fallback_fresh
    } else if xai_stale > 0 || fallback_stale > 0 {
        return Err(AuthError::Expired);
    } else {
        return Err(AuthError::Missing);
    };
    chosen.sort_by(|a, b| {
        email_sort_key(&a.identity.email)
            .cmp(&email_sort_key(&b.identity.email))
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(chosen)
}

fn email_sort_key(email: &Option<String>) -> String {
    email.as_deref().unwrap_or("").to_ascii_lowercase()
}

fn entry_expired(entry: &AuthEntry) -> bool {
    entry
        .expires_at
        .as_deref()
        .and_then(|expires| DateTime::parse_from_rfc3339(expires).ok())
        .is_some_and(|when| when.with_timezone(&Utc) <= Utc::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grok_home_respects_override() {
        assert_eq!(
            grok_home_from(Some("/tmp/grok-test-home".into())),
            PathBuf::from("/tmp/grok-test-home")
        );
        assert_eq!(
            grok_home_from(Some(String::new())),
            dirs_home().join(".grok")
        );
        assert_eq!(grok_home_from(None), dirs_home().join(".grok"));
    }

    #[test]
    fn prefers_unexpired_xai_token() {
        let json = r#"{
            "https://auth.x.ai::stale": {"key":"stale-token","expires_at":"2000-01-01T00:00:00Z","email":"old@x.ai"},
            "https://auth.x.ai::fresh": {"key":"fresh-token","expires_at":"2099-01-01T00:00:00Z","email":"new@x.ai"},
            "https://other.example::": {"key":"other-token","expires_at":"2099-01-01T00:00:00Z"}
        }"#;
        let bearer = bearer_from_auth_json(json).unwrap();
        assert_eq!(bearer.token, "fresh-token");
        assert_eq!(bearer.identity.email.as_deref(), Some("new@x.ai"));
    }

    #[test]
    fn skips_empty_xai_key() {
        let json = r#"{
            "https://auth.x.ai::empty": {"key":"","expires_at":"2099-01-01T00:00:00Z"},
            "https://cli.example::": {"key":"fallback-token","expires_at":"2099-01-01T00:00:00Z","email":"cli@x.ai"}
        }"#;
        let bearer = bearer_from_auth_json(json).unwrap();
        assert_eq!(bearer.token, "fallback-token");
    }

    #[test]
    fn expired_xai_does_not_shadow_live_fallback() {
        let json = r#"{
            "https://auth.x.ai::stale": {"key":"stale-token","expires_at":"2000-01-01T00:00:00Z"},
            "https://cli.example::": {"key":"fallback-token","expires_at":"2099-01-01T00:00:00Z"}
        }"#;
        let bearer = bearer_from_auth_json(json).unwrap();
        assert_eq!(bearer.token, "fallback-token");
    }

    #[test]
    fn sole_expired_xai_is_expired() {
        let json = r#"{
            "https://auth.x.ai::stale": {"key":"stale-token","expires_at":"2000-01-01T00:00:00Z"}
        }"#;
        assert!(matches!(
            bearer_from_auth_json(json),
            Err(AuthError::Expired)
        ));
    }

    #[test]
    fn load_bearers_returns_all_fresh_xai() {
        let json = r#"{
            "https://auth.x.ai::b": {"key":"token-b","expires_at":"2099-01-01T00:00:00Z","email":"b@x.ai"},
            "https://auth.x.ai::a": {"key":"token-a","expires_at":"2099-01-01T00:00:00Z","email":"a@x.ai"},
            "https://auth.x.ai::stale": {"key":"stale-token","expires_at":"2000-01-01T00:00:00Z","email":"old@x.ai"},
            "https://cli.example::": {"key":"fallback-token","expires_at":"2099-01-01T00:00:00Z","email":"cli@x.ai"}
        }"#;
        let bearers = bearers_from_auth_json(json).unwrap();
        let emails: Vec<_> = bearers
            .iter()
            .map(|b| b.identity.email.as_deref().unwrap())
            .collect();
        assert_eq!(emails, ["a@x.ai", "b@x.ai"]);
        assert_eq!(bearers[0].token, "token-a");
        assert_eq!(bearers[1].token, "token-b");
    }
}
