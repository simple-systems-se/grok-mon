use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
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

/// Why one CLI home did not become the personal chip's weekly login.
///
/// Variants carry no account id, email, or token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliHomeStatus {
    Ok,
    Missing,
    Expired,
    Invalid,
    DuplicateOfEarlier,
    AttachedToOtherAccount,
    Unmatched,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliHomeReport {
    pub label: String,
    pub status: CliHomeStatus,
    new_ids: Vec<String>,
}

pub enum IdPlace {
    Personal,
    OtherAccount,
    SeparateChip,
    Unknown,
}

impl CliHomeReport {
    pub fn refine_attachment(&mut self, place: impl Fn(&str) -> IdPlace) {
        if self.status != CliHomeStatus::Ok {
            return;
        }
        let mut on_personal = false;
        let mut on_other = false;
        let mut separate = false;
        for id in &self.new_ids {
            match place(id) {
                IdPlace::Personal => on_personal = true,
                IdPlace::OtherAccount => on_other = true,
                IdPlace::SeparateChip | IdPlace::Unknown => separate = true,
            }
        }
        let next = if on_personal && !on_other && !separate {
            CliHomeStatus::Ok
        } else if separate {
            CliHomeStatus::Unmatched
        } else if on_other {
            CliHomeStatus::AttachedToOtherAccount
        } else {
            CliHomeStatus::Ok
        };
        if next != self.status {
            self.status = next;
            log_home_status(self);
        }
    }

    pub fn personal_explanation(&self) -> Option<String> {
        personal_dash_explanation(&self.label, self.status)
    }
}

pub struct CliAuthLoad {
    pub bearers: Result<Vec<Bearer>, AuthError>,
    pub homes: Vec<CliHomeReport>,
}

/// Every unexpired Grok Build token in the default home, xAI entries first.
pub fn load_bearers() -> Result<Vec<Bearer>, AuthError> {
    load_cli_homes(&[grok_home()]).bearers
}

/// Read each home's `auth.json`. The first home is the default `~/.grok`
/// (or `$GROK_HOME`). Later homes are extra official CLI profiles. A repeated
/// account id keeps the earlier token, so the default home stays the one
/// already linked on the panel.
pub fn load_cli_homes(homes: &[PathBuf]) -> CliAuthLoad {
    let mut all = Vec::new();
    let mut seen_ids = HashSet::new();
    let mut reports = Vec::new();
    let mut saw_expired = false;
    let mut saw_invalid = false;
    for home in homes {
        let label = display_home(home);
        let path = home.join("auth.json");
        let mut raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let report = CliHomeReport {
                    label,
                    status: CliHomeStatus::Missing,
                    new_ids: Vec::new(),
                };
                log_home_status(&report);
                reports.push(report);
                continue;
            }
            Err(_) => {
                let report = CliHomeReport {
                    label,
                    status: CliHomeStatus::Invalid,
                    new_ids: Vec::new(),
                };
                log_home_status(&report);
                reports.push(report);
                saw_invalid = true;
                continue;
            }
        };
        let parsed = bearers_from_auth_json(&raw);
        raw.zeroize();
        let report = match parsed {
            Ok(bearers) => {
                let mut new_ids = Vec::new();
                let mut duplicates = 0usize;
                for bearer in bearers {
                    if seen_ids.insert(bearer.id.clone()) {
                        new_ids.push(bearer.id.clone());
                        all.push(bearer);
                    } else {
                        duplicates += 1;
                    }
                }
                let status = if new_ids.is_empty() && duplicates > 0 {
                    CliHomeStatus::DuplicateOfEarlier
                } else {
                    CliHomeStatus::Ok
                };
                if duplicates > 0 && !new_ids.is_empty() {
                    tracing::warn!(
                        home = %label,
                        "cli home skipped a duplicate login already read from an earlier home"
                    );
                }
                CliHomeReport {
                    label,
                    status,
                    new_ids,
                }
            }
            Err(AuthError::Expired) => {
                saw_expired = true;
                CliHomeReport {
                    label,
                    status: CliHomeStatus::Expired,
                    new_ids: Vec::new(),
                }
            }
            Err(AuthError::Invalid) => {
                saw_invalid = true;
                CliHomeReport {
                    label,
                    status: CliHomeStatus::Invalid,
                    new_ids: Vec::new(),
                }
            }
            Err(AuthError::Missing) => CliHomeReport {
                label,
                status: CliHomeStatus::Missing,
                new_ids: Vec::new(),
            },
        };
        log_home_status(&report);
        reports.push(report);
    }
    let bearers = if !all.is_empty() {
        all.sort_by(|a, b| {
            email_sort_key(&a.identity.email)
                .cmp(&email_sort_key(&b.identity.email))
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(all)
    } else if saw_expired {
        Err(AuthError::Expired)
    } else if saw_invalid {
        Err(AuthError::Invalid)
    } else {
        Err(AuthError::Missing)
    };
    CliAuthLoad {
        bearers,
        homes: reports,
    }
}

pub fn display_home(path: &std::path::Path) -> String {
    let home = dirs_home();
    if let Ok(rest) = path.strip_prefix(&home) {
        let rest = rest.to_string_lossy();
        if rest.is_empty() {
            return "~".into();
        }
        return format!("~/{rest}");
    }
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "cli home".into())
}

pub fn home_status_log(label: &str, status: CliHomeStatus) -> String {
    match status {
        CliHomeStatus::Ok => format!("cli home ok: {label}"),
        CliHomeStatus::Missing => format!("cli home has no login: {label}"),
        CliHomeStatus::Expired => format!("cli home login expired: {label}"),
        CliHomeStatus::Invalid => format!("cli home login could not be read: {label}"),
        CliHomeStatus::DuplicateOfEarlier => {
            format!("cli home skipped: same account as an earlier home: {label}")
        }
        CliHomeStatus::AttachedToOtherAccount => {
            format!("cli home login is attached to another account: {label}")
        }
        CliHomeStatus::Unmatched => format!("cli home login is on its own chip: {label}"),
    }
}

pub fn personal_dash_explanation(label: &str, status: CliHomeStatus) -> Option<String> {
    let command = login_command(label);
    let text = match status {
        CliHomeStatus::Ok => return None,
        CliHomeStatus::DuplicateOfEarlier => format!(
            "The login in {label} is the same account as the default ~/.grok login. Sign in with the personal account: {command}"
        ),
        CliHomeStatus::Missing => {
            format!("{label} has no Grok CLI login. Sign in with the personal account: {command}")
        }
        CliHomeStatus::Expired => {
            format!("The login in {label} is expired. Sign in with the personal account: {command}")
        }
        CliHomeStatus::Invalid => format!(
            "The login in {label} could not be read. Sign in with the personal account: {command}"
        ),
        CliHomeStatus::AttachedToOtherAccount => format!(
            "The login in {label} is attached to another account. Open that chip and choose Grok (pers) under Same weekly pool."
        ),
        CliHomeStatus::Unmatched => format!(
            "A login from {label} is on its own chip. Open that chip and choose Grok (pers) under Same weekly pool."
        ),
    };
    Some(text)
}

fn login_command(label: &str) -> String {
    let path = if let Some(rest) = label.strip_prefix("~/") {
        format!("$HOME/{rest}")
    } else if label == "~" {
        "$HOME".into()
    } else {
        label.to_string()
    };
    format!("GROK_HOME=\"{path}\" grok login")
}

fn log_home_status(report: &CliHomeReport) {
    let line = home_status_log(&report.label, report.status);
    match report.status {
        CliHomeStatus::Ok => tracing::info!("{line}"),
        _ => tracing::warn!("{line}"),
    }
}

pub fn auth_homes(extra: &[String]) -> Vec<PathBuf> {
    let mut homes = vec![grok_home()];
    for raw in extra {
        let path = expand_home_path(raw);
        if path.as_os_str().is_empty() {
            continue;
        }
        if homes.iter().any(|existing| existing == &path) {
            continue;
        }
        homes.push(path);
    }
    homes
}

fn expand_home_path(raw: &str) -> PathBuf {
    let trimmed = raw.trim();
    if trimmed == "~" {
        return dirs_home();
    }
    if let Some(rest) = trimmed.strip_prefix("~/") {
        return dirs_home().join(rest);
    }
    PathBuf::from(trimmed)
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

    #[test]
    fn second_home_is_added_and_the_default_home_wins_a_duplicate_id() {
        let root = std::env::temp_dir().join(format!(
            "grok-mon-auth-homes-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let primary = root.join("default");
        let personal = root.join("personal");
        std::fs::create_dir_all(&primary).unwrap();
        std::fs::create_dir_all(&personal).unwrap();
        std::fs::write(
            primary.join("auth.json"),
            r#"{"https://auth.x.ai::work":{"key":"work-token","expires_at":"2099-01-01T00:00:00Z","email":"work@example.com"}}"#,
        )
        .unwrap();
        std::fs::write(
            personal.join("auth.json"),
            r#"{"https://auth.x.ai::work":{"key":"should-not-replace","expires_at":"2099-01-01T00:00:00Z","email":"work@example.com"},"https://auth.x.ai::pers":{"key":"pers-token","expires_at":"2099-01-01T00:00:00Z","email":"pers@example.com"}}"#,
        )
        .unwrap();
        let bearers = load_cli_homes(&[primary, personal]).bearers.unwrap();
        let work = bearers
            .iter()
            .find(|bearer| bearer.id == "https://auth.x.ai::work")
            .unwrap();
        let pers = bearers
            .iter()
            .find(|bearer| bearer.id == "https://auth.x.ai::pers")
            .unwrap();
        assert_eq!(work.token, "work-token");
        assert_eq!(pers.token, "pers-token");
        assert_eq!(bearers.len(), 2);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn tilde_extra_home_expands_and_default_home_is_not_repeated() {
        let homes = auth_homes(&["~/.grok-personal".into(), "~/.grok".into()]);
        assert_eq!(homes[0], dirs_home().join(".grok"));
        assert_eq!(homes[1], dirs_home().join(".grok-personal"));
        assert_eq!(homes.len(), 2);
    }

    fn fresh(id: &str, token: &str, email: &str) -> String {
        format!(
            r#"{{"{id}":{{"key":"{token}","expires_at":"2099-01-01T00:00:00Z","email":"{email}"}}}}"#
        )
    }

    fn assert_no_secrets(text: &str) {
        for secret in [
            "pers-token",
            "work-token",
            "pers@example.com",
            "work@example.com",
            "https://auth.x.ai::pers",
            "https://auth.x.ai::work",
            "secret-key",
        ] {
            assert!(!text.contains(secret), "{text}");
        }
    }

    #[test]
    fn each_home_status_is_sanitized() {
        let root = std::env::temp_dir().join(format!(
            "grok-mon-home-status-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let default_home = root.join(".grok");
        let same = root.join(".grok-personal");
        let expired = root.join(".grok-expired");
        let invalid = root.join(".grok-invalid");
        let missing = root.join(".grok-missing");
        let unreadable = root.join(".grok-unreadable");
        for dir in [&default_home, &same, &expired, &invalid, &missing] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(
            default_home.join("auth.json"),
            fresh("https://auth.x.ai::work", "work-token", "work@example.com"),
        )
        .unwrap();
        std::fs::write(
            same.join("auth.json"),
            fresh("https://auth.x.ai::work", "pers-token", "pers@example.com"),
        )
        .unwrap();
        std::fs::write(
            expired.join("auth.json"),
            r#"{"https://auth.x.ai::old":{"key":"secret-key","expires_at":"2000-01-01T00:00:00Z","email":"old@example.com"}}"#,
        )
        .unwrap();
        std::fs::write(invalid.join("auth.json"), "not-json").unwrap();
        std::fs::create_dir_all(unreadable.join("auth.json")).unwrap();

        let loaded = load_cli_homes(&[
            default_home,
            same.clone(),
            expired,
            invalid,
            missing.clone(),
            unreadable,
        ]);
        let statuses: Vec<_> = loaded.homes.iter().map(|home| home.status).collect();
        assert_eq!(
            statuses,
            vec![
                CliHomeStatus::Ok,
                CliHomeStatus::DuplicateOfEarlier,
                CliHomeStatus::Expired,
                CliHomeStatus::Invalid,
                CliHomeStatus::Missing,
                CliHomeStatus::Invalid,
            ]
        );
        let bearers = loaded.bearers.unwrap();
        assert_eq!(bearers.len(), 1);
        assert_eq!(bearers[0].token, "work-token");
        for home in &loaded.homes {
            let log = home_status_log(&home.label, home.status);
            assert_no_secrets(&log);
            if let Some(explanation) = home.personal_explanation() {
                assert_no_secrets(&explanation);
                assert!(explanation.contains(&home.label));
            }
        }
        let duplicate = loaded
            .homes
            .iter()
            .find(|home| home.status == CliHomeStatus::DuplicateOfEarlier)
            .unwrap();
        let explanation = duplicate.personal_explanation().unwrap();
        assert!(explanation.contains("same account as the default ~/.grok login"));
        assert!(explanation.contains("GROK_HOME="));
        assert!(explanation.contains("grok login"));
        assert_no_secrets(&explanation);
        assert!(
            display_home(&same).ends_with(".grok-personal")
                || display_home(&same).contains("personal")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn unmatched_and_other_account_explanations_name_the_pool_setting() {
        let unmatched =
            personal_dash_explanation("~/.grok-personal", CliHomeStatus::Unmatched).unwrap();
        let other =
            personal_dash_explanation("~/.grok-personal", CliHomeStatus::AttachedToOtherAccount)
                .unwrap();
        assert!(unmatched.contains("Same weekly pool"));
        assert!(other.contains("Same weekly pool"));
        assert!(personal_dash_explanation("~/.grok-personal", CliHomeStatus::Ok).is_none());
        assert_no_secrets(&unmatched);
        assert_no_secrets(&other);
    }

    #[test]
    fn refine_marks_a_separate_chip_and_another_account() {
        let mut separate = CliHomeReport {
            label: "~/.grok-personal".into(),
            status: CliHomeStatus::Ok,
            new_ids: vec!["https://auth.x.ai::pers".into()],
        };
        separate.refine_attachment(|_| IdPlace::SeparateChip);
        assert_eq!(separate.status, CliHomeStatus::Unmatched);
        let line = separate.personal_explanation().unwrap();
        assert!(line.contains("Same weekly pool"));
        assert_no_secrets(&line);

        let mut other = CliHomeReport {
            label: "~/.grok-personal".into(),
            status: CliHomeStatus::Ok,
            new_ids: vec!["https://auth.x.ai::pers".into()],
        };
        other.refine_attachment(|_| IdPlace::OtherAccount);
        assert_eq!(other.status, CliHomeStatus::AttachedToOtherAccount);
        assert_no_secrets(&other.personal_explanation().unwrap());

        let mut personal = CliHomeReport {
            label: "~/.grok-personal".into(),
            status: CliHomeStatus::Ok,
            new_ids: vec!["https://auth.x.ai::pers".into()],
        };
        personal.refine_attachment(|_| IdPlace::Personal);
        assert_eq!(personal.status, CliHomeStatus::Ok);
        assert!(personal.personal_explanation().is_none());
    }
}
