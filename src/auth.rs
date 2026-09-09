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
pub fn load_bearer() -> Result<Bearer, AuthError> {
    let path = grok_home().join("auth.json");
    let mut raw = fs::read_to_string(&path).map_err(|_| AuthError::Missing)?;
    let result = bearer_from_auth_json(&raw);
    raw.zeroize();
    result
}

fn bearer_from_auth_json(raw: &str) -> Result<Bearer, AuthError> {
    let map: HashMap<String, AuthEntry> =
        serde_json::from_str(raw).map_err(|_| AuthError::Invalid)?;

    let mut preferred_fresh: Option<AuthEntry> = None;
    let mut preferred_stale: Option<AuthEntry> = None;
    let mut fallback: Option<AuthEntry> = None;
    for (key, entry) in map {
        if entry.key.as_deref().is_none_or(|k| k.is_empty()) {
            continue;
        }
        if key.starts_with("https://auth.x.ai::") {
            if entry_expired(&entry) {
                if preferred_stale.is_none() {
                    preferred_stale = Some(entry);
                }
            } else if preferred_fresh.is_none() {
                preferred_fresh = Some(entry);
            }
        } else if fallback.is_none() {
            fallback = Some(entry);
        }
    }
    let mut entry = match (preferred_fresh, fallback, preferred_stale) {
        (Some(entry), _, _) => entry,
        (None, Some(entry), _) if !entry_expired(&entry) => entry,
        (None, _, Some(entry)) => entry,
        (None, Some(entry), None) => entry,
        (None, None, None) => return Err(AuthError::Missing),
    };
    let mut token = entry
        .key
        .take()
        .filter(|k| !k.is_empty())
        .ok_or(AuthError::Missing)?;

    if entry_expired(&entry) {
        token.zeroize();
        return Err(AuthError::Expired);
    }

    Ok(Bearer {
        token,
        identity: AuthIdentity {
            email: entry.email.take(),
            principal_type: entry.principal_type.take(),
        },
    })
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
}
