use super::roots::{
    DEFAULT_GROK_BOT_DIR_NAME, discover_config_roots, keyring_app_name, root_used_at, secrets_path,
    session_marker_path,
};
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
use std::time::SystemTime;
use zeroize::{Zeroize, Zeroizing};

pub use super::roots::grok_bot_config_dir;

const SCOPED_PREFIX: &str = "scoped:v1:";
const ACCOUNT_SCOPE_LEN: usize = 64;
const OSCRYPT_SALT: &[u8] = b"saltysalt";
const OSCRYPT_ROUNDS: u32 = 1;
/// Chromium OSCrypt v10 hardcoded password. COSMIC is not a libsecret desktop
/// in Chromium, so Grok Bot encrypts `sand-secrets.json` with this key and
/// leaves any leftover "Chromium Safe Storage" item stale.
const OSCRYPT_V10_PASSWORD: &[u8] = b"peanuts";

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
    #[allow(dead_code)]
    pub email: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Identity {
    pub email: Option<String>,
    pub name: Option<String>,
}

impl Identity {
    pub fn display(&self) -> Option<&str> {
        self.email
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .or_else(|| {
                self.name
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
            })
    }
}

pub struct CursorAccount {
    pub id: String,
    pub active: bool,
    pub identity: Identity,
    pub token: Option<String>,
    pub machine_id: String,
    pub error: Option<AuthError>,
    pub config_dir: PathBuf,
    pub used_at: Option<SystemTime>,
    pub running: bool,
}

impl Drop for CursorAccount {
    fn drop(&mut self) {
        if let Some(ref mut token) = self.token {
            token.zeroize();
        }
        self.machine_id.zeroize();
    }
}

impl Drop for CursorBearer {
    fn drop(&mut self) {
        self.token.zeroize();
        self.machine_id.zeroize();
    }
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
    let body = &ciphertext[3..];
    let mut last_err = AuthError::Invalid;
    for candidate in oscrypt_passwords(password) {
        match decrypt_oscrypt_with(body, candidate) {
            Ok(plain) => return Ok(plain),
            Err(err) => last_err = err,
        }
    }
    Err(last_err)
}

fn decrypt_oscrypt_with(body: &[u8], password: &[u8]) -> Result<Vec<u8>, AuthError> {
    let key = pbkdf2_hmac_array::<Sha1, 16>(password, OSCRYPT_SALT, OSCRYPT_ROUNDS);
    let iv = [b' '; 16];
    Aes128CbcDec::new(&key.into(), &iv.into())
        .decrypt_padded_vec_mut::<Pkcs7>(body)
        .map_err(|_| AuthError::Invalid)
}

fn oscrypt_passwords(password: &[u8]) -> impl Iterator<Item = &[u8]> {
    [
        Some(password),
        (password != OSCRYPT_V10_PASSWORD).then_some(OSCRYPT_V10_PASSWORD),
        (password != b"").then_some(b"".as_slice()),
    ]
    .into_iter()
    .flatten()
}

#[cfg(test)]
pub fn email_from_access_token(token: &str) -> Option<String> {
    identity_from_access_token(token).email
}

pub fn identity_from_access_token(token: &str) -> Identity {
    let Some(payload) = token.split('.').nth(1) else {
        return Identity::default();
    };
    let Some(json) = b64url_decode(payload) else {
        return Identity::default();
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&json) else {
        return Identity::default();
    };
    let claim = |keys: &[&str]| {
        keys.iter()
            .find_map(|key| value.get(*key))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    Identity {
        email: claim(&["email"]),
        name: claim(&["name", "preferred_username", "username"]),
    }
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
    let identity = identity_from_access_token(&token);
    Ok(CursorBearer {
        token,
        machine_id,
        email: identity.email,
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
    let account = if let Some(id) = file.active.as_deref().filter(|id| !id.is_empty()) {
        file.accounts.get(id).ok_or(AuthError::Missing)?
    } else if file.accounts.len() == 1 {
        file.accounts.values().next().ok_or(AuthError::Missing)?
    } else {
        return Err(AuthError::Missing);
    };
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

#[allow(dead_code)]
pub async fn load_bearer() -> Result<CursorBearer, AuthError> {
    let mut last = AuthError::Missing;
    let mut paths: Vec<PathBuf> = discover_config_roots()
        .into_iter()
        .map(|dir| secrets_path(&dir))
        .collect();
    if paths.is_empty() {
        paths.push(secrets_path(&grok_bot_config_dir()));
    }
    for path in paths {
        match load_bearer_from_path(&path).await {
            Ok(bearer) => return Ok(bearer),
            Err(err) => last = err,
        }
    }
    Err(last)
}

#[allow(dead_code)]
pub async fn load_bearer_from_path(path: &Path) -> Result<CursorBearer, AuthError> {
    let raw = std::fs::read_to_string(path).map_err(|_| AuthError::Missing)?;
    let v10_err = match first_matching_bearer(&raw, [OSCRYPT_V10_PASSWORD, b"".as_slice()]) {
        Ok(bearer) => return Ok(bearer),
        Err(err @ (AuthError::Expired | AuthError::Missing)) => return Err(err),
        Err(err) => err,
    };
    let passwords = match tokio::time::timeout(
        std::time::Duration::from_secs(15),
        keyring_passwords(&keyring_names_for_path(path)),
    )
    .await
    {
        Ok(Ok(passwords)) => passwords,
        Ok(Err(err)) => return Err(err),
        Err(_) => return Err(AuthError::Keyring),
    };
    bearer_after_keyring(&raw, v10_err, &passwords)
}

pub async fn load_accounts() -> Result<Vec<CursorAccount>, AuthError> {
    let roots = discover_config_roots();
    if roots.is_empty() {
        return load_accounts_from_path(&secrets_path(&grok_bot_config_dir())).await;
    }
    load_accounts_from_roots(&roots).await
}

pub async fn load_accounts_from_roots(roots: &[PathBuf]) -> Result<Vec<CursorAccount>, AuthError> {
    if roots.is_empty() {
        return Err(AuthError::Missing);
    }

    let mut loaded = Vec::new();
    let mut pending_keyring = Vec::new();
    let mut last_err = AuthError::Missing;

    for root in roots {
        let path = secrets_path(root);
        match std::fs::read_to_string(&path) {
            Ok(raw) => {
                match first_matching_accounts(&raw, [OSCRYPT_V10_PASSWORD, b"".as_slice()]) {
                    Ok(accounts) => loaded.extend(attach_root(accounts, root)),
                    Err(AuthError::Missing) => last_err = AuthError::Missing,
                    Err(err) => pending_keyring.push((root.clone(), raw, err)),
                }
            }
            Err(_) => last_err = AuthError::Missing,
        }
    }

    if !pending_keyring.is_empty() {
        let mut names: Vec<String> = pending_keyring
            .iter()
            .map(|(root, _, _)| keyring_app_name(root))
            .collect();
        if !names.iter().any(|n| n == DEFAULT_GROK_BOT_DIR_NAME) {
            names.push(DEFAULT_GROK_BOT_DIR_NAME.into());
        }
        names.sort();
        names.dedup();
        let passwords = match tokio::time::timeout(
            std::time::Duration::from_secs(15),
            keyring_passwords(&names),
        )
        .await
        {
            Ok(Ok(passwords)) => passwords,
            Ok(Err(err)) => return Err(err),
            Err(_) => return Err(AuthError::Keyring),
        };
        for (root, raw, v10_err) in pending_keyring {
            match accounts_after_keyring(&raw, v10_err, &passwords) {
                Ok(accounts) => loaded.extend(attach_root(accounts, &root)),
                Err(err) => {
                    tracing::warn!(
                        root = %keyring_app_name(&root),
                        error = %err,
                        "failed to read Grok Bot secrets"
                    );
                    last_err = err;
                }
            }
        }
    }

    if loaded.is_empty() {
        return Err(last_err);
    }
    Ok(merge_duplicate_accounts(loaded))
}

pub async fn load_accounts_from_path(path: &Path) -> Result<Vec<CursorAccount>, AuthError> {
    let raw = std::fs::read_to_string(path).map_err(|_| AuthError::Missing)?;
    let v10_err = match first_matching_accounts(&raw, [OSCRYPT_V10_PASSWORD, b"".as_slice()]) {
        Ok(accounts) => {
            return Ok(attach_root(
                accounts,
                path.parent().unwrap_or_else(|| Path::new(".")),
            ));
        }
        Err(err @ AuthError::Missing) => return Err(err),
        Err(err) => err,
    };
    let passwords = match tokio::time::timeout(
        std::time::Duration::from_secs(15),
        keyring_passwords(&keyring_names_for_path(path)),
    )
    .await
    {
        Ok(Ok(passwords)) => passwords,
        Ok(Err(err)) => return Err(err),
        Err(_) => return Err(AuthError::Keyring),
    };
    accounts_after_keyring(&raw, v10_err, &passwords)
        .map(|accounts| attach_root(accounts, path.parent().unwrap_or_else(|| Path::new("."))))
}

fn keyring_names_for_path(path: &Path) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(parent) = path.parent() {
        names.push(keyring_app_name(parent));
    }
    if !names.iter().any(|n| n == DEFAULT_GROK_BOT_DIR_NAME) {
        names.push(DEFAULT_GROK_BOT_DIR_NAME.into());
    }
    names
}

fn attach_root(mut accounts: Vec<CursorAccount>, root: &Path) -> Vec<CursorAccount> {
    let used_at = root_used_at(root);
    let running = super::roster::session_running(&session_marker_path(root));
    for account in &mut accounts {
        account.config_dir = root.to_path_buf();
        account.used_at = Some(used_at);
        account.running = running;
    }
    accounts
}

/// Same Cursor account in more than one userData dir: keep one chip.
/// Prefer a live token, then a running install, then the secrets-file active
/// account, then the most recently used root, then the default `Grok Bot` dir.
pub fn merge_duplicate_accounts(accounts: Vec<CursorAccount>) -> Vec<CursorAccount> {
    let mut by_key: HashMap<String, CursorAccount> = HashMap::new();
    for account in accounts {
        let key = account_merge_key(&account);
        match by_key.remove(&key) {
            Some(existing) if !prefer_account(&account, &existing) => {
                by_key.insert(key, existing);
            }
            _ => {
                by_key.insert(key, account);
            }
        }
    }
    let mut merged: Vec<CursorAccount> = by_key.into_values().collect();
    sort_accounts(&mut merged);
    uniquify_account_ids(&mut merged);
    merged
}

fn account_merge_key(account: &CursorAccount) -> String {
    account
        .identity
        .email
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|email| format!("email:{}", email.to_ascii_lowercase()))
        .unwrap_or_else(|| format!("id:{}", account.id.to_ascii_lowercase()))
}

fn prefer_account(candidate: &CursorAccount, current: &CursorAccount) -> bool {
    let cand_live = candidate.token.is_some();
    let cur_live = current.token.is_some();
    if cand_live != cur_live {
        return cand_live;
    }
    if candidate.running != current.running {
        return candidate.running;
    }
    if candidate.active != current.active {
        return candidate.active;
    }
    match (candidate.used_at, current.used_at) {
        (Some(left), Some(right)) if left != right => return left > right,
        (Some(_), None) => return true,
        (None, Some(_)) => return false,
        _ => {}
    }
    is_default_root(&candidate.config_dir) && !is_default_root(&current.config_dir)
}

fn is_default_root(dir: &Path) -> bool {
    dir.file_name()
        .is_some_and(|name| name == DEFAULT_GROK_BOT_DIR_NAME)
}

fn uniquify_account_ids(accounts: &mut [CursorAccount]) {
    let mut seen = HashMap::<String, usize>::new();
    for account in accounts.iter_mut() {
        let count = seen.entry(account.id.clone()).or_insert(0);
        *count += 1;
        if *count == 1 {
            continue;
        }
        let slug = account
            .config_dir
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("dir");
        account.id = format!("{}:{slug}", account.id);
    }
}

fn accounts_after_keyring(
    raw: &str,
    v10_err: AuthError,
    passwords: &[Zeroizing<Vec<u8>>],
) -> Result<Vec<CursorAccount>, AuthError> {
    if passwords.is_empty() {
        return Err(v10_err);
    }
    first_matching_accounts(raw, passwords.iter().map(|p| p.as_slice()))
}

pub fn first_matching_accounts(
    raw: &str,
    passwords: impl IntoIterator<Item = impl AsRef<[u8]>>,
) -> Result<Vec<CursorAccount>, AuthError> {
    let mut last_err = AuthError::Keyring;
    let mut tried = false;
    for password in passwords {
        tried = true;
        match accounts_from_secrets_json(raw, password.as_ref()) {
            Ok(accounts) => return Ok(accounts),
            Err(err @ AuthError::Missing) => return Err(err),
            Err(err) => last_err = err,
        }
    }
    if tried {
        Err(last_err)
    } else {
        Err(AuthError::Keyring)
    }
}

pub fn accounts_from_secrets_json(
    raw: &str,
    password: &[u8],
) -> Result<Vec<CursorAccount>, AuthError> {
    let map: HashMap<String, Value> = serde_json::from_str(raw).map_err(|_| AuthError::Invalid)?;
    let mut machine_id = decrypt_stored(
        json_str(&map, "cursor-machine-id").ok_or(AuthError::Missing)?,
        password,
    )?;
    if machine_id.is_empty() {
        machine_id.zeroize();
        return Err(AuthError::Invalid);
    }

    let mut accounts = Vec::new();
    let mut nested_err = None;
    if let Some(value) = map.get("cursor-accounts") {
        match parse_accounts_field(value) {
            Ok(file) => {
                let active = file.active.clone();
                for (id, secrets) in file.accounts {
                    let Some(stored) = secrets.access_token.as_deref().filter(|s| !s.is_empty())
                    else {
                        continue;
                    };
                    match account_from_stored(&id, &active, stored, password, &machine_id) {
                        Ok(account) => accounts.push(account),
                        Err(err) => nested_err = Some(err),
                    }
                }
            }
            Err(err) => nested_err = Some(err),
        }
    }

    if accounts.is_empty()
        && let Some(stored) = json_str(&map, "cursor-access-token")
        && let Ok(account) = account_from_stored(
            "default",
            &Some("default".into()),
            stored,
            password,
            &machine_id,
        )
    {
        accounts.push(account);
    }

    machine_id.zeroize();
    if accounts.is_empty() {
        return Err(nested_err.unwrap_or(AuthError::Missing));
    }
    sort_accounts(&mut accounts);
    Ok(accounts)
}

fn account_from_stored(
    id: &str,
    active: &Option<String>,
    stored: &str,
    password: &[u8],
    machine_id: &str,
) -> Result<CursorAccount, AuthError> {
    let mut token = decrypt_stored(stored, password)?;
    if token.is_empty() {
        token.zeroize();
        return Err(AuthError::Missing);
    }
    let identity = identity_from_access_token(&token);
    let expired = token_expired(&token);
    if expired {
        token.zeroize();
    }
    Ok(CursorAccount {
        id: id.to_string(),
        active: active.as_deref() == Some(id),
        identity,
        token: if expired { None } else { Some(token) },
        machine_id: machine_id.to_string(),
        error: expired.then_some(AuthError::Expired),
        config_dir: PathBuf::new(),
        used_at: None,
        running: false,
    })
}

fn sort_accounts(accounts: &mut [CursorAccount]) {
    accounts.sort_by(|a, b| match (a.active, b.active) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a
            .identity
            .display()
            .map(str::to_ascii_lowercase)
            .cmp(&b.identity.display().map(str::to_ascii_lowercase))
            .then_with(|| a.id.cmp(&b.id)),
    });
}

fn bearer_after_keyring(
    raw: &str,
    v10_err: AuthError,
    passwords: &[Zeroizing<Vec<u8>>],
) -> Result<CursorBearer, AuthError> {
    if passwords.is_empty() {
        return Err(v10_err);
    }
    first_matching_bearer(raw, passwords.iter().map(|p| p.as_slice()))
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

async fn keyring_passwords(app_names: &[String]) -> Result<Vec<Zeroizing<Vec<u8>>>, AuthError> {
    use secret_service::{EncryptionType, SecretService};
    let ss = SecretService::connect(EncryptionType::Dh)
        .await
        .map_err(|_| AuthError::Keyring)?;
    let names = if app_names.is_empty() {
        vec![DEFAULT_GROK_BOT_DIR_NAME.to_string()]
    } else {
        app_names.to_vec()
    };
    let mut passwords = Vec::new();
    let mut unlock_failed = false;
    for name in &names {
        let schema_search = ss
            .search_items(HashMap::from([
                ("application", name.as_str()),
                ("xdg:schema", "chrome_libsecret_os_crypt_password_v2"),
            ]))
            .await
            .map_err(|_| AuthError::Keyring)?;
        let search = if schema_search.unlocked.is_empty() && schema_search.locked.is_empty() {
            ss.search_items(HashMap::from([("application", name.as_str())]))
                .await
                .map_err(|_| AuthError::Keyring)?
        } else {
            schema_search
        };
        match secrets_from_search(search).await {
            Ok(mut found) => passwords.append(&mut found),
            Err(AuthError::Keyring) => unlock_failed = true,
            Err(err) => return Err(err),
        }
    }
    if passwords.is_empty() && unlock_failed {
        return Err(AuthError::Keyring);
    }
    Ok(passwords)
}

async fn secrets_from_search(
    search: secret_service::SearchItemsResult<secret_service::Item<'_>>,
) -> Result<Vec<Zeroizing<Vec<u8>>>, AuthError> {
    let mut passwords = Vec::new();
    let mut unlock_failed = false;
    for item in &search.unlocked {
        if let Ok(secret) = item.get_secret().await
            && !secret.is_empty()
        {
            passwords.push(Zeroizing::new(secret));
        }
    }
    for item in &search.locked {
        if item.unlock().await.is_err() {
            unlock_failed = true;
            continue;
        }
        if let Ok(secret) = item.get_secret().await
            && !secret.is_empty()
        {
            passwords.push(Zeroizing::new(secret));
        }
    }
    if passwords.is_empty() && unlock_failed {
        return Err(AuthError::Keyring);
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
    fn v10_peanuts_ignores_stale_keyring_password() {
        let ct = encrypt_oscrypt(PLAIN, OSCRYPT_V10_PASSWORD, b"v10");
        assert_eq!(
            decrypt_oscrypt(&ct, b"stale-libsecret-password").unwrap(),
            PLAIN
        );
        assert_eq!(decrypt_oscrypt(&ct, OSCRYPT_V10_PASSWORD).unwrap(), PLAIN);
    }

    #[test]
    fn v10_empty_key_fallback() {
        let ct = encrypt_oscrypt(PLAIN, b"", b"v10");
        assert_eq!(
            decrypt_oscrypt(&ct, b"stale-libsecret-password").unwrap(),
            PLAIN
        );
    }

    #[test]
    fn stale_keyring_still_reads_v10_peanuts_secrets() {
        let token_plain = sample_token();
        let mid_plain = "machine-id-uuid";
        let json = serde_json::json!({
            "cursor-access-token": BASE64.encode(&encrypt_oscrypt(
                token_plain.as_bytes(),
                OSCRYPT_V10_PASSWORD,
                b"v10",
            )),
            "cursor-machine-id": BASE64.encode(&encrypt_oscrypt(
                mid_plain.as_bytes(),
                OSCRYPT_V10_PASSWORD,
                b"v10",
            )),
        })
        .to_string();
        let bearer =
            first_matching_bearer(&json, [b"stale-libsecret-password".as_slice()]).unwrap();
        assert_eq!(bearer.token, token_plain);
        assert_eq!(bearer.machine_id, mid_plain);
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
    fn identity_from_jwt_prefers_email() {
        let payload = BASE64.encode(
            br#"{"email":"bot@example.com","name":"Bot","preferred_username":"botty","exp":4102444800}"#,
        );
        let token = format!("eyJhbGciOiJI.{payload}.sig");
        let identity = identity_from_access_token(&token);
        assert_eq!(identity.email.as_deref(), Some("bot@example.com"));
        assert_eq!(identity.name.as_deref(), Some("Bot"));
        assert_eq!(identity.display(), Some("bot@example.com"));
    }

    #[test]
    fn identity_from_jwt_uses_name_without_email() {
        let payload = BASE64.encode(br#"{"preferred_username":"botty","exp":4102444800}"#);
        let token = format!("eyJhbGciOiJI.{payload}.sig");
        let identity = identity_from_access_token(&token);
        assert_eq!(identity.email, None);
        assert_eq!(identity.name.as_deref(), Some("botty"));
        assert_eq!(identity.display(), Some("botty"));
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

    fn jwt(email: &str, exp: i64) -> String {
        let payload = BASE64.encode(format!(r#"{{"email":"{email}","exp":{exp}}}"#).as_bytes());
        format!("eyJhbGciOiJI.{payload}.sig")
    }

    #[test]
    fn load_accounts_returns_all_nested_and_active_first() {
        let active_id = SCOPE;
        let other_id = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
        let active_tok = jwt("active@example.com", 4_102_444_800);
        let other_tok = jwt("other@example.com", 4_102_444_800);
        let json = serde_json::json!({
            "cursor-accounts": {
                "active": active_id,
                "accounts": {
                    other_id: { "cursor-access-token": encrypt_field(&other_tok) },
                    active_id: { "cursor-access-token": encrypt_field(&active_tok) },
                }
            },
            "cursor-machine-id": encrypt_field("machine-id-uuid"),
        })
        .to_string();
        let accounts = accounts_from_secrets_json(&json, PASSWORD).unwrap();
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts[0].id, active_id);
        assert!(accounts[0].active);
        assert_eq!(
            accounts[0].identity.email.as_deref(),
            Some("active@example.com")
        );
        assert_eq!(accounts[0].token.as_deref(), Some(active_tok.as_str()));
        assert_eq!(accounts[1].id, other_id);
        assert!(!accounts[1].active);
        assert_eq!(
            accounts[1].identity.email.as_deref(),
            Some("other@example.com")
        );
    }

    #[test]
    fn load_accounts_keeps_expired_next_to_live() {
        let live_id = SCOPE;
        let stale_id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let live_tok = jwt("live@example.com", 4_102_444_800);
        let stale_tok = jwt("stale@example.com", 1);
        let json = serde_json::json!({
            "cursor-accounts": {
                "active": live_id,
                "accounts": {
                    stale_id: { "cursor-access-token": encrypt_field(&stale_tok) },
                    live_id: { "cursor-access-token": encrypt_field(&live_tok) },
                }
            },
            "cursor-machine-id": encrypt_field("machine-id-uuid"),
        })
        .to_string();
        let accounts = accounts_from_secrets_json(&json, PASSWORD).unwrap();
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts[0].id, live_id);
        assert!(accounts[0].token.is_some());
        let stale = accounts.iter().find(|a| a.id == stale_id).unwrap();
        assert!(stale.token.is_none());
        assert!(matches!(stale.error, Some(AuthError::Expired)));
        assert_eq!(stale.identity.email.as_deref(), Some("stale@example.com"));
    }

    #[test]
    fn load_accounts_legacy_top_level_is_one_default() {
        let token_plain = sample_token();
        let json = serde_json::json!({
            "cursor-access-token": encrypt_field(&token_plain),
            "cursor-machine-id": encrypt_field("machine-id-uuid"),
        })
        .to_string();
        let accounts = accounts_from_secrets_json(&json, PASSWORD).unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].id, "default");
        assert!(accounts[0].active);
        assert_eq!(accounts[0].token.as_deref(), Some(token_plain.as_str()));
        assert_eq!(
            accounts[0].identity.email.as_deref(),
            Some("bot@example.com")
        );
    }

    fn sample_account(
        id: &str,
        email: &str,
        active: bool,
        dir: &str,
        used: u64,
        live: bool,
        running: bool,
    ) -> CursorAccount {
        CursorAccount {
            id: id.into(),
            active,
            identity: Identity {
                email: Some(email.into()),
                name: None,
            },
            token: live.then(|| "tok".into()),
            machine_id: "m".into(),
            error: (!live).then_some(AuthError::Expired),
            config_dir: PathBuf::from(dir),
            used_at: Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(used)),
            running,
        }
    }

    #[test]
    fn merge_same_email_prefers_active_then_recent() {
        let stale = sample_account(
            "aaaa",
            "user@example.com",
            false,
            "/tmp/Grok Bot Work",
            200,
            true,
            false,
        );
        let active = sample_account(
            "aaaa",
            "user@example.com",
            true,
            "/tmp/Grok Bot",
            100,
            true,
            false,
        );
        let merged = merge_duplicate_accounts(vec![stale, active]);
        assert_eq!(merged.len(), 1);
        assert!(merged[0].active);
        assert_eq!(
            merged[0].config_dir.file_name().and_then(|n| n.to_str()),
            Some("Grok Bot")
        );
    }

    #[test]
    fn merge_same_email_prefers_running_over_older_active() {
        let running = sample_account(
            "bbbb",
            "work@example.com",
            false,
            "/tmp/Grok Bot Work",
            50,
            true,
            true,
        );
        let active = sample_account(
            "bbbb",
            "work@example.com",
            true,
            "/tmp/Grok Bot",
            400,
            true,
            false,
        );
        let merged = merge_duplicate_accounts(vec![active, running]);
        assert_eq!(merged.len(), 1);
        assert!(merged[0].running);
        assert_eq!(
            merged[0].config_dir.file_name().and_then(|n| n.to_str()),
            Some("Grok Bot Work")
        );
    }

    #[test]
    fn merge_keeps_distinct_emails() {
        let personal = sample_account(
            "default",
            "user@example.com",
            true,
            "/tmp/Grok Bot",
            10,
            true,
            false,
        );
        let work = sample_account(
            "default",
            "work@example.com",
            true,
            "/tmp/Grok Bot Work",
            20,
            true,
            false,
        );
        let merged = merge_duplicate_accounts(vec![personal, work]);
        assert_eq!(merged.len(), 2);
        let mut ids: Vec<_> = merged.iter().map(|a| a.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, vec!["default", "default:Grok Bot Work"]);
    }

    #[test]
    fn two_profile_v10_secrets_become_two_chips() {
        fn v10_blob(email: &str) -> String {
            let token = jwt(email, 4_102_444_800);
            serde_json::json!({
                "cursor-access-token": BASE64.encode(&encrypt_oscrypt(
                    token.as_bytes(),
                    OSCRYPT_V10_PASSWORD,
                    b"v10",
                )),
                "cursor-machine-id": BASE64.encode(&encrypt_oscrypt(
                    b"machine-id-uuid",
                    OSCRYPT_V10_PASSWORD,
                    b"v10",
                )),
            })
            .to_string()
        }
        let mut personal =
            first_matching_accounts(&v10_blob("user@example.com"), [OSCRYPT_V10_PASSWORD]).unwrap();
        let mut work =
            first_matching_accounts(&v10_blob("work@example.com"), [OSCRYPT_V10_PASSWORD]).unwrap();
        personal[0].config_dir = PathBuf::from("/tmp/Grok Bot");
        work[0].config_dir = PathBuf::from("/tmp/Grok Bot Work");
        let merged = merge_duplicate_accounts(personal.into_iter().chain(work).collect());
        assert_eq!(merged.len(), 2);
        let emails: Vec<_> = merged
            .iter()
            .filter_map(|a| a.identity.email.as_deref())
            .collect();
        assert!(emails.contains(&"user@example.com"));
        assert!(emails.contains(&"work@example.com"));
    }

    #[test]
    fn merge_prefers_live_token_over_expired() {
        let expired = sample_account(
            "cccc",
            "same@example.com",
            true,
            "/tmp/Grok Bot",
            500,
            false,
            true,
        );
        let live = sample_account(
            "cccc",
            "same@example.com",
            false,
            "/tmp/Grok Bot Work",
            1,
            true,
            false,
        );
        let merged = merge_duplicate_accounts(vec![expired, live]);
        assert_eq!(merged.len(), 1);
        assert!(merged[0].token.is_some());
        assert_eq!(
            merged[0].config_dir.file_name().and_then(|n| n.to_str()),
            Some("Grok Bot Work")
        );
    }

    #[test]
    fn stale_active_falls_back_to_top_level_token() {
        let top = sample_token();
        let json = serde_json::json!({
            "cursor-access-token": encrypt_field(&top),
            "cursor-accounts": {
                "active": "missing-id",
                "accounts": {}
            },
            "cursor-machine-id": encrypt_field("machine-id-uuid"),
        })
        .to_string();
        let bearer = bearer_from_secrets_json(&json, PASSWORD).unwrap();
        assert_eq!(bearer.token, top);
    }

    #[test]
    fn empty_keyring_keeps_v10_error() {
        assert!(matches!(
            bearer_after_keyring("not-json", AuthError::Invalid, &[]),
            Err(AuthError::Invalid)
        ));
    }
}
