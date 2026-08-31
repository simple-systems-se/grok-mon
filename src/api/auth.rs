use serde_json::Value;
use std::path::{Path, PathBuf};
use zeroize::Zeroize;

#[derive(Debug, Clone)]
pub enum AuthError {
    Missing,
    InferenceKey,
    Invalid,
    Rejected,
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => write!(
                f,
                "not configured — add a management key to {}",
                credentials_path().display()
            ),
            Self::InferenceKey => {
                write!(f, "need a Management API key, not XAI_API_KEY")
            }
            Self::Invalid => write!(f, "invalid management key config"),
            Self::Rejected => write!(f, "management key rejected"),
        }
    }
}

pub struct Credentials {
    pub key: String,
    pub team_id: Option<String>,
}

impl Drop for Credentials {
    fn drop(&mut self) {
        self.key.zeroize();
    }
}

pub fn credentials_path() -> PathBuf {
    credentials_path_from(std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
}

fn credentials_path_from(xdg: Option<PathBuf>) -> PathBuf {
    xdg.unwrap_or_else(|| dirs_home().join(".config"))
        .join("grok-mon-api")
        .join("credentials.json")
}

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Read the Management API key. Do not log or persist it.
pub fn load_credentials() -> Result<Credentials, AuthError> {
    combine_file_and_env(
        read_credentials_file(&credentials_path()),
        env_nonempty("XAI_MANAGEMENT_API_KEY").or_else(|| env_nonempty("XAI_MANAGEMENT_KEY")),
        env_nonempty("XAI_TEAM_ID"),
    )
}

pub fn read_credentials_file(path: &Path) -> Result<Credentials, AuthError> {
    let mut raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(AuthError::Missing);
        }
        Err(_) => return Err(AuthError::Invalid),
    };
    let parsed = parse_credentials_json(&raw);
    raw.zeroize();
    parsed
}

pub fn parse_credentials_json(raw: &str) -> Result<Credentials, AuthError> {
    let value: Value = serde_json::from_str(raw).map_err(|_| AuthError::Invalid)?;
    let obj = value.as_object().ok_or(AuthError::Invalid)?;
    let key = first_string(
        obj,
        &[
            "management_key",
            "managementKey",
            "management_api_key",
            "managementApiKey",
            "XAI_MANAGEMENT_API_KEY",
            "XAI_MANAGEMENT_KEY",
            "key",
        ],
    )
    .unwrap_or_default();
    let team_id = first_string(
        obj,
        &[
            "team_id",
            "teamId",
            "XAI_TEAM_ID",
            "workspace_id",
            "workspaceId",
        ],
    );
    Ok(Credentials { key, team_id })
}

fn combine_file_and_env(
    file: Result<Credentials, AuthError>,
    env_key: Option<String>,
    env_team: Option<String>,
) -> Result<Credentials, AuthError> {
    match file {
        Ok(mut creds) => {
            overlay_env(&mut creds, env_key, env_team);
            finish(creds)
        }
        Err(AuthError::Missing) => {
            let mut creds = Credentials {
                key: String::new(),
                team_id: None,
            };
            overlay_env(&mut creds, env_key, env_team);
            finish(creds)
        }
        Err(err) => {
            let mut creds = Credentials {
                key: String::new(),
                team_id: None,
            };
            overlay_env(&mut creds, env_key, env_team);
            if creds.key.is_empty() {
                Err(err)
            } else {
                finish(creds)
            }
        }
    }
}

fn overlay_env(creds: &mut Credentials, env_key: Option<String>, env_team: Option<String>) {
    if let Some(key) = env_key.filter(|s| !s.is_empty()) {
        creds.key.zeroize();
        creds.key = key;
    }
    if let Some(team) = env_team.filter(|s| !s.is_empty()) {
        creds.team_id = Some(team);
    }
}

fn finish(creds: Credentials) -> Result<Credentials, AuthError> {
    if creds.key.is_empty() {
        if env_nonempty("XAI_API_KEY").is_some() {
            return Err(AuthError::InferenceKey);
        }
        return Err(AuthError::Missing);
    }
    Ok(creds)
}

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.is_empty())
}

fn first_string(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = obj.get(*key)
            && let Some(s) = json_string(value)
            && !s.is_empty()
        {
            return Some(s);
        }
    }
    None
}

fn json_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process;

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("grok-mon-api-auth-{name}-{}", process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.join("credentials.json")
    }

    #[test]
    fn parse_management_key_and_team() {
        let creds =
            parse_credentials_json(r#"{"management_key":"xai-mgmt-test","team_id":"team-1"}"#)
                .unwrap();
        assert_eq!(creds.key, "xai-mgmt-test");
        assert_eq!(creds.team_id.as_deref(), Some("team-1"));
    }

    #[test]
    fn parse_alt_keys() {
        let creds =
            parse_credentials_json(r#"{"XAI_MANAGEMENT_API_KEY":"xai-alt","teamId":"abc"}"#)
                .unwrap();
        assert_eq!(creds.key, "xai-alt");
        assert_eq!(creds.team_id.as_deref(), Some("abc"));
    }

    #[test]
    fn parse_rejects_invalid_json() {
        assert!(matches!(
            parse_credentials_json("not-json"),
            Err(AuthError::Invalid)
        ));
    }

    #[test]
    fn read_file_roundtrip() {
        let path = temp_file("roundtrip");
        fs::write(&path, r#"{"management_key":"xai-file","team_id":"t"}"#).unwrap();
        let creds = read_credentials_file(&path).unwrap();
        assert_eq!(creds.key, "xai-file");
        assert_eq!(creds.team_id.as_deref(), Some("t"));
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir(path.parent().unwrap());
    }

    #[test]
    fn finish_missing_and_inference() {
        let empty = Credentials {
            key: String::new(),
            team_id: None,
        };
        // Cannot reliably assert InferenceKey without mutating process env.
        assert!(matches!(
            finish(empty),
            Err(AuthError::Missing | AuthError::InferenceKey)
        ));
    }

    #[test]
    fn credentials_path_uses_xdg() {
        assert_eq!(
            credentials_path_from(Some(PathBuf::from("/tmp/xdg"))),
            PathBuf::from("/tmp/xdg/grok-mon-api/credentials.json")
        );
    }

    #[test]
    fn env_overrides_invalid_file() {
        let creds = combine_file_and_env(
            Err(AuthError::Invalid),
            Some("xai-from-env".into()),
            Some("team-env".into()),
        )
        .unwrap();
        assert_eq!(creds.key, "xai-from-env");
        assert_eq!(creds.team_id.as_deref(), Some("team-env"));
    }

    #[test]
    fn invalid_file_without_env_stays_invalid() {
        assert!(matches!(
            combine_file_and_env(Err(AuthError::Invalid), None, None),
            Err(AuthError::Invalid)
        ));
    }

    #[test]
    fn unreadable_path_is_invalid() {
        let dir = temp_file("unreadable").parent().unwrap().to_path_buf();
        assert!(matches!(
            read_credentials_file(&dir),
            Err(AuthError::Invalid)
        ));
        let _ = fs::remove_dir_all(&dir);
    }
}
