use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

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

pub struct Bearer {
    pub token: String,
    pub identity: AuthIdentity,
}

impl Drop for Bearer {
    fn drop(&mut self) {
        self.token.clear();
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
    let raw = fs::read_to_string(&path).map_err(|_| AuthError::Missing)?;
    let map: HashMap<String, AuthEntry> =
        serde_json::from_str(&raw).map_err(|_| AuthError::Invalid)?;

    let mut preferred: Option<AuthEntry> = None;
    let mut fallback: Option<AuthEntry> = None;
    for (key, entry) in map {
        if key.starts_with("https://auth.x.ai::") {
            preferred = Some(entry);
        } else if fallback.is_none() && entry.key.is_some() {
            fallback = Some(entry);
        }
    }
    let entry = preferred.or(fallback).ok_or(AuthError::Missing)?;
    let token = entry.key.filter(|k| !k.is_empty()).ok_or(AuthError::Missing)?;

    if let Some(expires) = entry.expires_at.as_deref() {
        if let Ok(when) = DateTime::parse_from_rfc3339(expires) {
            if when.with_timezone(&Utc) <= Utc::now() {
                return Err(AuthError::Expired);
            }
        }
    }

    Ok(Bearer {
        token,
        identity: AuthIdentity {
            email: entry.email,
            principal_type: entry.principal_type,
        },
    })
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
        assert_eq!(grok_home_from(Some(String::new())), dirs_home().join(".grok"));
        assert_eq!(grok_home_from(None), dirs_home().join(".grok"));
    }
}
