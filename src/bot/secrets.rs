use aes::Aes128;
use cbc::Decryptor;
use cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use data_encoding::BASE64;
use pbkdf2::pbkdf2_hmac_array;
use serde::Deserialize;
use serde_json::Value;
use sha1::Sha1;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use zeroize::{Zeroize, Zeroizing};

const SCOPED_PREFIX: &str = "scoped:v1:";
const ACCOUNT_SCOPE_LEN: usize = 64;
const OSCRYPT_SALT: &[u8] = b"saltysalt";
const OSCRYPT_ROUNDS: u32 = 1;

type Aes128CbcDec = Decryptor<Aes128>;

#[derive(Debug, Clone)]
pub enum AuthError {
    Missing,
    Expired,
    Invalid,
    Keyring,
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => write!(f, "not signed in — open Grok Bot"),
            Self::Expired => write!(f, "session expired — open Grok Bot"),
            Self::Invalid => write!(f, "invalid Grok Bot secrets"),
            Self::Keyring => write!(f, "unlock the login keyring"),
        }
    }
}

pub struct CursorBearer {
    pub token: String,
    pub machine_id: String,
    pub email: Option<String>,
}

impl Drop for CursorBearer {
    fn drop(&mut self) {
        self.token.zeroize();
        self.machine_id.zeroize();
    }
}

pub fn grok_bot_config_dir() -> PathBuf {
    grok_bot_config_dir_from(std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
}

fn grok_bot_config_dir_from(xdg: Option<PathBuf>) -> PathBuf {
    xdg.unwrap_or_else(|| dirs_home().join(".config"))
        .join("Grok Bot")
}

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSecret {
    pub account_scope: Option<String>,
    pub ciphertext: Vec<u8>,
}

pub fn parse_stored_secret(stored: &str) -> Result<StoredSecret, AuthError> {
    if stored.is_empty() {
        return Err(AuthError::Missing);
    }
    if let Some(rest) = stored.strip_prefix(SCOPED_PREFIX) {
        let (scope, b64) = rest.split_once(':').ok_or(AuthError::Invalid)?;
        if scope.len() != ACCOUNT_SCOPE_LEN
            || !scope.bytes().all(|b| b.is_ascii_hexdigit())
            || b64.is_empty()
        {
            return Err(AuthError::Invalid);
        }
        let ciphertext = BASE64
            .decode(b64.as_bytes())
            .map_err(|_| AuthError::Invalid)?;
        return Ok(StoredSecret {
            account_scope: Some(scope.to_ascii_lowercase()),
            ciphertext,
        });
    }
    let ciphertext = BASE64
        .decode(stored.as_bytes())
        .map_err(|_| AuthError::Invalid)?;
    Ok(StoredSecret {
        account_scope: None,
        ciphertext,
    })
}

pub fn decrypt_oscrypt(ciphertext: &[u8], password: &[u8]) -> Result<Vec<u8>, AuthError> {
    if ciphertext.len() < 19 {
        return Err(AuthError::Invalid);
    }
    let prefix = &ciphertext[..3];
    if prefix != b"v10" && prefix != b"v11" {
        return Err(AuthError::Invalid);
    }
    let key = pbkdf2_hmac_array::<Sha1, 16>(password, OSCRYPT_SALT, OSCRYPT_ROUNDS);
    let iv = [b' '; 16];
    Aes128CbcDec::new(&key.into(), &iv.into())
        .decrypt_padded_vec_mut::<Pkcs7>(&ciphertext[3..])
        .map_err(|_| AuthError::Invalid)
}

pub fn email_from_access_token(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let json = b64url_decode(payload)?;
    let value: serde_json::Value = serde_json::from_slice(&json).ok()?;
    value
        .get("email")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn b64url_decode(raw: &str) -> Option<Vec<u8>> {
    let mut padded = raw.replace('-', "+").replace('_', "/");
    while !padded.len().is_multiple_of(4) {
        padded.push('=');
    }
    BASE64.decode(padded.as_bytes()).ok()
}

#[derive(Deserialize)]
struct AccountsFile {
    active: Option<String>,
    #[serde(default)]
    accounts: HashMap<String, AccountSecrets>,
}

#[derive(Deserialize)]
struct AccountSecrets {
    #[serde(rename = "cursor-access-token")]
    access_token: Option<String>,
}

pub fn bearer_from_secrets_json(raw: &str, password: &[u8]) -> Result<CursorBearer, AuthError> {
    let map: HashMap<String, Value> = serde_json::from_str(raw).map_err(|_| AuthError::Invalid)?;
    let mut token = access_token_from_secrets(&map, password)?;
    if token.is_empty() {
        token.zeroize();
        return Err(AuthError::Missing);
    }
    if token_expired(&token) {
        token.zeroize();
        return Err(AuthError::Expired);
    }
    let mut machine_id = decrypt_stored(
        json_str(&map, "cursor-machine-id").ok_or(AuthError::Missing)?,
        password,
    )?;
    if machine_id.is_empty() {
        token.zeroize();
        machine_id.zeroize();
        return Err(AuthError::Invalid);
    }
    let email = email_from_access_token(&token);
    Ok(CursorBearer {
        token,
        machine_id,
        email,
    })
}

fn json_str<'a>(map: &'a HashMap<String, Value>, key: &str) -> Option<&'a str> {
    map.get(key).and_then(Value::as_str)
}

fn access_token_from_secrets(
    map: &HashMap<String, Value>,
    password: &[u8],
) -> Result<String, AuthError> {
    let mut fallback_err = None;
    if map.get("cursor-accounts").is_some() {
        match access_token_from_accounts(map, password) {
            Ok(token) => return Ok(token),
            Err(err @ AuthError::Expired) => return Err(err),
            Err(err) => fallback_err = Some(err),
        }
    }
    if let Some(stored) = json_str(map, "cursor-access-token") {
        match decrypt_stored(stored, password) {
            Ok(token) => return Ok(token),
            Err(err @ (AuthError::Expired | AuthError::Missing)) => return Err(err),
            Err(err) => fallback_err = Some(err),
        }
    }
    Err(fallback_err.unwrap_or(AuthError::Missing))
}

fn access_token_from_accounts(
    map: &HashMap<String, Value>,
    password: &[u8],
) -> Result<String, AuthError> {
    let file = parse_accounts_field(map.get("cursor-accounts").ok_or(AuthError::Missing)?)?;
    let stored = account_token(&file)?;
    decrypt_stored(stored, password)
}

fn parse_accounts_field(value: &Value) -> Result<AccountsFile, AuthError> {
    match value {
        Value::String(s) => serde_json::from_str(s).map_err(|_| AuthError::Invalid),
        Value::Object(_) => AccountsFile::deserialize(value).map_err(|_| AuthError::Invalid),
        _ => Err(AuthError::Invalid),
    }
}

fn account_token(file: &AccountsFile) -> Result<&str, AuthError> {
    let by_active = file
        .active
        .as_deref()
        .filter(|id| !id.is_empty())
        .and_then(|id| file.accounts.get(id));
    let account = by_active
        .or_else(|| file.accounts.values().next())
        .ok_or(AuthError::Missing)?;
    account
        .access_token
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or(AuthError::Missing)
}

fn decrypt_stored(stored: &str, password: &[u8]) -> Result<String, AuthError> {
    let parsed = parse_stored_secret(stored)?;
    let plain = decrypt_oscrypt(&parsed.ciphertext, password)?;
    match String::from_utf8(plain) {
        Ok(s) => Ok(s),
        Err(err) => {
            let mut bytes = err.into_bytes();
            bytes.zeroize();
            Err(AuthError::Invalid)
        }
    }
}

fn token_expired(token: &str) -> bool {
    let Some(payload) = token.split('.').nth(1) else {
        return false;
    };
    let Some(json) = b64url_decode(payload) else {
        return false;
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&json) else {
        return false;
    };
    let Some(exp) = value.get("exp").and_then(|v| v.as_i64()) else {
        return false;
    };
    let now = chrono::Utc::now().timestamp();
    exp <= now
}

pub async fn load_bearer() -> Result<CursorBearer, AuthError> {
    let path = grok_bot_config_dir().join("sand-secrets.json");
    load_bearer_from_path(&path).await
}

pub async fn load_bearer_from_path(path: &Path) -> Result<CursorBearer, AuthError> {
    let raw = std::fs::read_to_string(path).map_err(|_| AuthError::Missing)?;
    let passwords =
        match tokio::time::timeout(std::time::Duration::from_secs(15), keyring_passwords()).await {
            Ok(result) => result?,
            Err(_) => return Err(AuthError::Keyring),
        };
    first_matching_bearer(&raw, passwords.iter().map(|p| p.as_slice()))
}

pub fn first_matching_bearer(
    raw: &str,
    passwords: impl IntoIterator<Item = impl AsRef<[u8]>>,
) -> Result<CursorBearer, AuthError> {
    let mut last_err = AuthError::Keyring;
    let mut tried = false;
    for password in passwords {
        tried = true;
        match bearer_from_secrets_json(raw, password.as_ref()) {
            Ok(bearer) => return Ok(bearer),
            Err(err @ (AuthError::Expired | AuthError::Missing)) => return Err(err),
            Err(err) => last_err = err,
        }
    }
    if tried {
        Err(last_err)
    } else {
        Err(AuthError::Keyring)
    }
}

async fn keyring_passwords() -> Result<Vec<Zeroizing<Vec<u8>>>, AuthError> {
    use secret_service::{EncryptionType, SecretService};
    let ss = SecretService::connect(EncryptionType::Dh)
        .await
        .map_err(|_| AuthError::Keyring)?;
    let schema_search = ss
        .search_items(HashMap::from([
            ("application", "Grok Bot"),
            ("xdg:schema", "chrome_libsecret_os_crypt_password_v2"),
        ]))
        .await
        .map_err(|_| AuthError::Keyring)?;
    let search = if schema_search.unlocked.is_empty() && schema_search.locked.is_empty() {
        ss.search_items(HashMap::from([("application", "Grok Bot")]))
            .await
            .map_err(|_| AuthError::Keyring)?
    } else {
        schema_search
    };
    secrets_from_search(search).await
}

async fn secrets_from_search(
    search: secret_service::SearchItemsResult<secret_service::Item<'_>>,
) -> Result<Vec<Zeroizing<Vec<u8>>>, AuthError> {
    let mut passwords = Vec::new();
    for item in &search.unlocked {
        if let Ok(secret) = item.get_secret().await
            && !secret.is_empty()
        {
            passwords.push(Zeroizing::new(secret));
        }
    }
    for item in &search.locked {
        if item.unlock().await.is_err() {
            continue;
        }
        if let Ok(secret) = item.get_secret().await
            && !secret.is_empty()
        {
            passwords.push(Zeroizing::new(secret));
        }
    }
    Ok(passwords)
}

pub fn client_version_from_marker(path: &Path) -> String {
    #[derive(Deserialize)]
    struct Marker {
        #[serde(rename = "appVersion")]
        app_version: Option<String>,
    }
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Marker>(&raw).ok())
        .and_then(|m| m.app_version)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "0.16.0".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes::Aes128;
    use cbc::Encryptor;
    use cipher::{BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};

    type Aes128CbcEnc = Encryptor<Aes128>;

    const PASSWORD: &[u8] = b"test-password-24-bytes!!";
    const PLAIN: &[u8] = b"hello-from-oscrypt-v11";
    const SCOPE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn encrypt_oscrypt(plain: &[u8], password: &[u8], prefix: &[u8; 3]) -> Vec<u8> {
        let key = pbkdf2_hmac_array::<Sha1, 16>(password, OSCRYPT_SALT, OSCRYPT_ROUNDS);
        let iv = [b' '; 16];
        let mut body =
            Aes128CbcEnc::new(&key.into(), &iv.into()).encrypt_padded_vec_mut::<Pkcs7>(plain);
        let mut out = prefix.to_vec();
        out.append(&mut body);
        out
    }

    #[test]
    fn parse_scoped_v1() {
        let ct = encrypt_oscrypt(PLAIN, PASSWORD, b"v11");
        let stored = format!("{SCOPED_PREFIX}{SCOPE}:{}", BASE64.encode(&ct));
        let parsed = parse_stored_secret(&stored).unwrap();
        assert_eq!(parsed.account_scope.as_deref(), Some(SCOPE));
        assert_eq!(
            decrypt_oscrypt(&parsed.ciphertext, PASSWORD).unwrap(),
            PLAIN
        );
    }

    #[test]
    fn parse_legacy_unscoped() {
        let ct = encrypt_oscrypt(PLAIN, PASSWORD, b"v10");
        let stored = BASE64.encode(&ct);
        let parsed = parse_stored_secret(&stored).unwrap();
        assert_eq!(parsed.account_scope, None);
        assert_eq!(
            decrypt_oscrypt(&parsed.ciphertext, PASSWORD).unwrap(),
            PLAIN
        );
    }

    #[test]
    fn parse_rejects_bad_scope() {
        assert!(parse_stored_secret("scoped:v1:short:YQ==").is_err());
        assert!(parse_stored_secret("scoped:v1:nocolon").is_err());
        assert!(parse_stored_secret("").is_err());
    }

    #[test]
    fn canned_v11_blob() {
        let b64 = "djExcgfW6y1Bw9nQ8dVO5kJowJxU9fXFeUmYuhanK7z4yAE=";
        let ct = BASE64.decode(b64.as_bytes()).unwrap();
        assert_eq!(decrypt_oscrypt(&ct, PASSWORD).unwrap(), PLAIN);
    }

    #[test]
    fn email_from_jwt() {
        let payload = BASE64.encode(br#"{"email":"bot@example.com","exp":4102444800}"#);
        let token = format!("eyJhbGciOiJI.{payload}.sig");
        assert_eq!(
            email_from_access_token(&token).as_deref(),
            Some("bot@example.com")
        );
    }

    #[test]
    fn first_matching_stops_on_expired_decrypt() {
        let payload = BASE64.encode(br#"{"email":"bot@example.com","exp":1}"#);
        let token_plain = format!("eyJhbGciOiJI.{payload}.sig");
        let mid_plain = "machine-id-uuid";
        let token_ct = encrypt_oscrypt(token_plain.as_bytes(), PASSWORD, b"v11");
        let mid_ct = encrypt_oscrypt(mid_plain.as_bytes(), PASSWORD, b"v11");
        let json = serde_json::json!({
            "cursor-access-token": format!("{SCOPED_PREFIX}{SCOPE}:{}", BASE64.encode(&token_ct)),
            "cursor-machine-id": BASE64.encode(&mid_ct),
        })
        .to_string();
        assert!(matches!(
            first_matching_bearer(&json, [PASSWORD, b"wrong-password"]),
            Err(AuthError::Expired)
        ));
        assert!(matches!(
            first_matching_bearer(&json, [b"wrong-password".as_slice()]),
            Err(AuthError::Invalid)
        ));
    }

    fn sample_token() -> String {
        "eyJhbGciOiJI.eyJlbWFpbCI6ImJvdEBleGFtcGxlLmNvbSIsImV4cCI6NDEwMjQ0NDgwMH0.sig".into()
    }

    fn encrypt_field(plain: &str) -> String {
        BASE64.encode(&encrypt_oscrypt(plain.as_bytes(), PASSWORD, b"v11"))
    }

    fn accounts_blob(token_stored: &str) -> serde_json::Value {
        serde_json::json!({
            "active": SCOPE,
            "accounts": {
                SCOPE: {
                    "cursor-access-token": token_stored,
                }
            }
        })
    }

    #[test]
    fn bearer_from_json_roundtrip() {
        let token_plain = sample_token();
        let mid_plain = "machine-id-uuid";
        let json = serde_json::json!({
            "cursor-access-token": format!("{SCOPED_PREFIX}{SCOPE}:{}", encrypt_field(&token_plain)),
            "cursor-machine-id": encrypt_field(mid_plain),
        })
        .to_string();
        let bearer = bearer_from_secrets_json(&json, PASSWORD).unwrap();
        assert_eq!(bearer.token, token_plain);
        assert_eq!(bearer.machine_id, mid_plain);
        assert_eq!(bearer.email.as_deref(), Some("bot@example.com"));
    }

    #[test]
    fn bearer_from_nested_accounts_string() {
        let token_plain = sample_token();
        let mid_plain = "machine-id-uuid";
        let json = serde_json::json!({
            "cursor-accounts": accounts_blob(&encrypt_field(&token_plain)).to_string(),
            "cursor-machine-id": encrypt_field(mid_plain),
        })
        .to_string();
        let bearer = bearer_from_secrets_json(&json, PASSWORD).unwrap();
        assert_eq!(bearer.token, token_plain);
        assert_eq!(bearer.machine_id, mid_plain);
        assert_eq!(bearer.email.as_deref(), Some("bot@example.com"));
    }

    #[test]
    fn bearer_from_nested_accounts_object() {
        let token_plain = sample_token();
        let mid_plain = "machine-id-uuid";
        let json = serde_json::json!({
            "cursor-accounts": accounts_blob(&encrypt_field(&token_plain)),
            "cursor-machine-id": encrypt_field(mid_plain),
        })
        .to_string();
        let bearer = bearer_from_secrets_json(&json, PASSWORD).unwrap();
        assert_eq!(bearer.token, token_plain);
        assert_eq!(bearer.machine_id, mid_plain);
    }

    #[test]
    fn nested_accounts_without_token_is_missing() {
        let json = serde_json::json!({
            "cursor-accounts": {
                "active": SCOPE,
                "accounts": { SCOPE: {} }
            },
            "cursor-machine-id": encrypt_field("machine-id-uuid"),
        })
        .to_string();
        assert!(matches!(
            bearer_from_secrets_json(&json, PASSWORD),
            Err(AuthError::Missing)
        ));
    }

    #[test]
    fn prefers_active_accounts_token() {
        let top = sample_token();
        let nested =
            "eyJhbGciOiJI.eyJlbWFpbCI6Im90aGVyQGV4YW1wbGUuY29tIiwiZXhwIjo0MTAyNDQ0ODAwfQ.sig";
        let json = serde_json::json!({
            "cursor-access-token": encrypt_field(&top),
            "cursor-accounts": accounts_blob(&encrypt_field(nested)),
            "cursor-machine-id": encrypt_field("machine-id-uuid"),
        })
        .to_string();
        let bearer = bearer_from_secrets_json(&json, PASSWORD).unwrap();
        assert_eq!(bearer.token, nested);
        assert_eq!(bearer.email.as_deref(), Some("other@example.com"));
    }

    #[test]
    fn grok_bot_config_dir_uses_xdg() {
        assert_eq!(
            grok_bot_config_dir_from(Some(PathBuf::from("/tmp/xdg"))),
            PathBuf::from("/tmp/xdg/Grok Bot")
        );
    }
}
