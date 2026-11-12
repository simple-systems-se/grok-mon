use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use zeroize::Zeroize;

#[derive(Debug, Clone)]
pub struct AuthIdentity {
    pub email: Option<String>,
    pub principal_type: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthError {
    Missing,
    Expired,
    Invalid,
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => write!(f, "not signed in"),
            Self::Expired => write!(f, "session expired"),
            Self::Invalid => write!(f, "session file is unreadable"),
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

/// Read the one session stored in this CLI home. Do not log or persist it.
///
/// A home with several entries still yields one session. The process
/// `GROK_HOME` is not consulted; pass the icon's home.
pub fn load_bearer_from(home: &Path) -> Result<Bearer, AuthError> {
    let path = home.join("auth.json");
    let mut raw = fs::read_to_string(&path).map_err(|_| AuthError::Missing)?;
    let result = primary_bearer_from_auth_json(&raw);
    raw.zeroize();
    result
}

fn primary_bearer_from_auth_json(raw: &str) -> Result<Bearer, AuthError> {
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
    fn prefers_unexpired_xai_token() {
        let json = r#"{
            "https://auth.x.ai::stale": {"key":"stale-token","expires_at":"2000-01-01T00:00:00Z","email":"old@x.ai"},
            "https://auth.x.ai::fresh": {"key":"fresh-token","expires_at":"2099-01-01T00:00:00Z","email":"new@x.ai"},
            "https://other.example::": {"key":"other-token","expires_at":"2099-01-01T00:00:00Z"}
        }"#;
        let bearer = primary_bearer_from_auth_json(json).unwrap();
        assert_eq!(bearer.token, "fresh-token");
        assert_eq!(bearer.identity.email.as_deref(), Some("new@x.ai"));
    }

    #[test]
    fn skips_empty_xai_key() {
        let json = r#"{
            "https://auth.x.ai::empty": {"key":"","expires_at":"2099-01-01T00:00:00Z"},
            "https://cli.example::": {"key":"fallback-token","expires_at":"2099-01-01T00:00:00Z","email":"cli@x.ai"}
        }"#;
        let bearer = primary_bearer_from_auth_json(json).unwrap();
        assert_eq!(bearer.token, "fallback-token");
    }

    #[test]
    fn expired_xai_does_not_shadow_live_fallback() {
        let json = r#"{
            "https://auth.x.ai::stale": {"key":"stale-token","expires_at":"2000-01-01T00:00:00Z"},
            "https://cli.example::": {"key":"fallback-token","expires_at":"2099-01-01T00:00:00Z"}
        }"#;
        let bearer = primary_bearer_from_auth_json(json).unwrap();
        assert_eq!(bearer.token, "fallback-token");
    }

    #[test]
    fn sole_expired_xai_is_expired() {
        let json = r#"{
            "https://auth.x.ai::stale": {"key":"stale-token","expires_at":"2000-01-01T00:00:00Z"}
        }"#;
        assert!(matches!(
            primary_bearer_from_auth_json(json),
            Err(AuthError::Expired)
        ));
    }

    #[test]
    fn one_cli_home_yields_one_session_even_with_multiple_entries() {
        let json = r#"{
            "https://auth.x.ai::b": {"key":"token-b","expires_at":"2099-01-01T00:00:00Z","email":"b@x.ai"},
            "https://auth.x.ai::a": {"key":"token-a","expires_at":"2099-01-01T00:00:00Z","email":"a@x.ai"},
            "https://auth.x.ai::stale": {"key":"stale-token","expires_at":"2000-01-01T00:00:00Z","email":"old@x.ai"},
            "https://cli.example::": {"key":"fallback-token","expires_at":"2099-01-01T00:00:00Z","email":"cli@x.ai"}
        }"#;
        let bearer = primary_bearer_from_auth_json(json).unwrap();
        assert_eq!(bearer.identity.email.as_deref(), Some("a@x.ai"));
        assert_eq!(bearer.token, "token-a");
    }

    #[test]
    fn sessions_stay_inside_their_cli_home() {
        let _guard = crate::login::PROCESS_ENV_LOCK.lock().unwrap();
        let previous = std::env::var_os("GROK_HOME");
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("grok-mon-auth-{nanos}"));
        let env_home = root.join("env");
        let sim = root.join("sim");
        let pers = root.join("pers");
        write_auth(&env_home, "env@example.com", "token-env");
        write_auth(&sim, "sim@example.com", "token-sim");
        write_auth(&pers, "pers@example.com", "token-pers");
        // SAFETY: tests serialize environment changes with PROCESS_ENV_LOCK.
        unsafe { std::env::set_var("GROK_HOME", &env_home) };

        let sim_bearer = load_bearer_from(&sim).unwrap();
        let pers_bearer = load_bearer_from(&pers).unwrap();
        assert_eq!(
            sim_bearer.identity.email.as_deref(),
            Some("sim@example.com")
        );
        assert_eq!(
            pers_bearer.identity.email.as_deref(),
            Some("pers@example.com")
        );
        assert_ne!(
            sim_bearer.identity.email.as_deref(),
            pers_bearer.identity.email.as_deref()
        );
        assert_ne!(
            sim_bearer.identity.email.as_deref(),
            Some("env@example.com")
        );
        assert!(load_bearer_from(&root.join("missing")).is_err());

        let _ = fs::remove_dir_all(&root);
        match previous {
            // SAFETY: tests serialize environment changes with PROCESS_ENV_LOCK.
            Some(value) => unsafe { std::env::set_var("GROK_HOME", value) },
            None => unsafe { std::env::remove_var("GROK_HOME") },
        }
    }

    fn write_auth(home: &Path, email: &str, token: &str) {
        fs::create_dir_all(home).unwrap();
        let json = format!(
            r#"{{"https://auth.x.ai::id":{{"key":"{token}","expires_at":"2099-01-01T00:00:00Z","email":"{email}"}}}}"#
        );
        fs::write(home.join("auth.json"), json).unwrap();
    }
}
