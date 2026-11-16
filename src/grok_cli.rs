//! Locate the `grok` executable without changing the panel process environment.
//!
//! Sign-in and sign-out both use [`resolve_grok`]. Usage polling does not spawn
//! `grok`; it calls the billing HTTPS API. An empty `grok_path` searches, in
//! order: `PATH`, `$HOME/.grok/bin/grok`, the newest
//! `$HOME/.nvm/versions/node/*/bin/grok` (semantic version, not mtime),
//! `$HOME/.bun/bin/grok`, then `$HOME/.local/bin/grok`. Only a regular
//! executable file is accepted. Symlinks are followed for that check.

use std::cmp::Ordering;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

const BIN_NAME: &str = "grok";

/// Resolve `grok` from the current process `PATH` and `HOME`.
///
/// Reads those variables and does not write them. `GROK_HOME` is not read.
pub fn resolve_grok_from_env(configured: &str) -> Result<PathBuf, String> {
    let path = std::env::var_os("PATH");
    let home = crate::accounts::user_home_dir();
    resolve_grok(configured, path.as_deref(), home.as_deref())
}

/// Resolve `grok`. `configured` is the `grok_path` setting. Empty means search.
/// `path_env` is the `PATH` value. `home` is the user home directory.
pub fn resolve_grok(
    configured: &str,
    path_env: Option<&OsStr>,
    home: Option<&Path>,
) -> Result<PathBuf, String> {
    let configured = configured.trim();
    if !configured.is_empty() {
        return resolve_configured(configured, home);
    }
    let mut checked = Vec::new();
    if let Some(found) = search_path(path_env, &mut checked) {
        return Ok(found);
    }
    if let Some(found) = search_home(home, &mut checked) {
        return Ok(found);
    }
    Err(not_found(&checked))
}

fn resolve_configured(configured: &str, home: Option<&Path>) -> Result<PathBuf, String> {
    match expand_user_path(configured, home) {
        Ok(path) => {
            if is_executable_file(&path) {
                Ok(path)
            } else {
                Err(not_found(&[path.display().to_string()]))
            }
        }
        Err(reason) => Err(format!(
            "The grok CLI was not found. The grok_path setting could not be used ({reason}). Places checked:\n- {configured}"
        )),
    }
}

fn search_path(path_env: Option<&OsStr>, checked: &mut Vec<String>) -> Option<PathBuf> {
    let Some(path_env) = path_env else {
        checked.push("PATH (not set)".into());
        return None;
    };
    let mut saw_dir = false;
    for dir in std::env::split_paths(path_env) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        saw_dir = true;
        let candidate = dir.join(BIN_NAME);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
        checked.push(candidate.display().to_string());
    }
    if !saw_dir {
        checked.push("PATH (empty)".into());
    }
    None
}

fn search_home(home: Option<&Path>, checked: &mut Vec<String>) -> Option<PathBuf> {
    let Some(home) = home.filter(|path| !path.as_os_str().is_empty()) else {
        checked.push("$HOME/.grok/bin/grok (HOME is not set)".into());
        checked.push("$HOME/.nvm/versions/node/*/bin/grok (HOME is not set)".into());
        checked.push("$HOME/.bun/bin/grok (HOME is not set)".into());
        checked.push("$HOME/.local/bin/grok (HOME is not set)".into());
        return None;
    };

    let official = home.join(".grok/bin").join(BIN_NAME);
    if is_executable_file(&official) {
        return Some(official);
    }
    checked.push(official.display().to_string());

    if let Some(found) = newest_nvm(home, checked) {
        return Some(found);
    }

    for relative in [".bun/bin", ".local/bin"] {
        let candidate = home.join(relative).join(BIN_NAME);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
        checked.push(candidate.display().to_string());
    }
    None
}

fn newest_nvm(home: &Path, checked: &mut Vec<String>) -> Option<PathBuf> {
    let versions_dir = home.join(".nvm/versions/node");
    let read = match fs::read_dir(&versions_dir) {
        Ok(entries) => entries,
        Err(_) => {
            checked.push(nvm_glob(&versions_dir));
            return None;
        }
    };

    let mut versions = Vec::new();
    for entry in read {
        let Ok(entry) = entry else {
            continue;
        };
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_dir() {
            continue;
        }
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let Some(version) = parse_node_version(&name) else {
            continue;
        };
        versions.push((version, entry.path().join("bin").join(BIN_NAME)));
    }
    if versions.is_empty() {
        checked.push(nvm_glob(&versions_dir));
        return None;
    }
    versions.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));

    let mut chosen = None;
    for (_, candidate) in &versions {
        if chosen.is_none() && is_executable_file(candidate) {
            chosen = Some(candidate.clone());
        }
    }
    if chosen.is_some() {
        return chosen;
    }
    for (_, candidate) in versions {
        checked.push(candidate.display().to_string());
    }
    None
}

fn nvm_glob(versions_dir: &Path) -> String {
    format!("{}/bin/{BIN_NAME}", versions_dir.join("*").display())
}

fn not_found(checked: &[String]) -> String {
    let mut message = String::from("The grok CLI was not found. Places checked:");
    if checked.is_empty() {
        message.push_str("\n- (no locations)");
        return message;
    }
    for place in checked {
        message.push('\n');
        message.push_str("- ");
        message.push_str(place);
    }
    message
}

fn expand_user_path(raw: &str, home: Option<&Path>) -> Result<PathBuf, String> {
    if raw == "~" {
        return home
            .map(Path::to_path_buf)
            .ok_or_else(|| "HOME is not set".to_string());
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        let base = home.ok_or_else(|| "HOME is not set".to_string())?;
        return Ok(base.join(rest));
    }
    if raw.starts_with('~') {
        return Err("only ~/ is expanded, for the current user".into());
    }
    Ok(PathBuf::from(raw))
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(meta) = path.metadata() else {
        return false;
    };
    meta.is_file() && executable_mode(&meta)
}

#[cfg(unix)]
fn executable_mode(meta: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn executable_mode(_meta: &fs::Metadata) -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NodeVersion {
    major: u64,
    minor: u64,
    patch: u64,
    pre: Vec<PreIdent>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PreIdent {
    Num(u64),
    Text(String),
}

impl PartialOrd for NodeVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for NodeVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        self.major
            .cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
            .then(self.patch.cmp(&other.patch))
            .then_with(|| cmp_pre(&self.pre, &other.pre))
    }
}

fn cmp_pre(left: &[PreIdent], right: &[PreIdent]) -> Ordering {
    match (left.is_empty(), right.is_empty()) {
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        _ => {
            for (left_ident, right_ident) in left.iter().zip(right.iter()) {
                let ord = cmp_ident(left_ident, right_ident);
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            left.len().cmp(&right.len())
        }
    }
}

fn cmp_ident(left: &PreIdent, right: &PreIdent) -> Ordering {
    match (left, right) {
        (PreIdent::Num(left), PreIdent::Num(right)) => left.cmp(right),
        (PreIdent::Text(left), PreIdent::Text(right)) => left.cmp(right),
        (PreIdent::Num(_), PreIdent::Text(_)) => Ordering::Less,
        (PreIdent::Text(_), PreIdent::Num(_)) => Ordering::Greater,
    }
}

fn parse_node_version(name: &str) -> Option<NodeVersion> {
    let rest = name
        .strip_prefix('v')
        .or_else(|| name.strip_prefix('V'))
        .unwrap_or(name);
    let core_and_pre = rest.split_once('+').map(|(head, _)| head).unwrap_or(rest);
    let (core, pre) = match core_and_pre.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (core_and_pre, None),
    };
    let mut numbers = core.split('.');
    let major = parse_numeric(numbers.next()?)?;
    let minor = parse_numeric(numbers.next()?)?;
    let patch = parse_numeric(numbers.next()?)?;
    if numbers.next().is_some() {
        return None;
    }
    let pre = match pre {
        None => Vec::new(),
        Some(pre) => parse_pre(pre)?,
    };
    Some(NodeVersion {
        major,
        minor,
        patch,
        pre,
    })
}

fn parse_numeric(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if text.len() > 1 && text.starts_with('0') {
        return None;
    }
    text.parse().ok()
}

fn parse_pre(text: &str) -> Option<Vec<PreIdent>> {
    if text.is_empty() {
        return None;
    }
    let mut idents = Vec::new();
    for ident in text.split('.') {
        if ident.is_empty() || !ident.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return None;
        }
        if ident.bytes().all(|byte| byte.is_ascii_digit()) {
            idents.push(PreIdent::Num(parse_numeric(ident)?));
        } else {
            idents.push(PreIdent::Text(ident.to_string()));
        }
    }
    Some(idents)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    struct TempTree(PathBuf);

    impl TempTree {
        fn new() -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root =
                std::env::temp_dir().join(format!("grok-mon-cli-{nanos}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_file(path: &Path, mode: u32) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut file = fs::File::create(path).unwrap();
        file.write_all(b"#!/bin/sh\nexit 0\n").unwrap();
        let mut perms = file.metadata().unwrap().permissions();
        perms.set_mode(mode);
        fs::set_permissions(path, perms).unwrap();
    }

    fn set_mtime(path: &Path, when: SystemTime) {
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(when)
            .unwrap();
    }

    #[test]
    fn path_hit_wins_over_later_installs() {
        let tree = TempTree::new();
        let path_dir = tree.path().join("path-bin");
        let home = tree.path().join("home");
        write_file(&path_dir.join("grok"), 0o755);
        write_file(&home.join(".grok/bin/grok"), 0o755);
        let path_env = std::env::join_paths([&path_dir]).unwrap();

        let found = resolve_grok("  ", Some(path_env.as_os_str()), Some(&home)).unwrap();

        assert_eq!(found, path_dir.join("grok"));
    }

    #[test]
    fn grok_bin_fallback_follows_the_installer_symlink() {
        let tree = TempTree::new();
        let path_dir = tree.path().join("path-bin");
        let home = tree.path().join("home");
        fs::create_dir_all(&path_dir).unwrap();
        let bin = home.join(".grok/bin");
        write_file(&bin.join("grok-1.0.77"), 0o755);
        symlink("grok-1.0.77", bin.join("grok")).unwrap();
        write_file(&home.join(".nvm/versions/node/v22.0.0/bin/grok"), 0o755);
        let path_env = std::env::join_paths([&path_dir]).unwrap();

        let found = resolve_grok("", Some(path_env.as_os_str()), Some(&home)).unwrap();

        assert_eq!(found, bin.join("grok"));
        assert!(found.metadata().unwrap().is_file());
    }

    #[test]
    fn nvm_picks_the_newest_semantic_version_not_mtime() {
        let tree = TempTree::new();
        let path_dir = tree.path().join("path-bin");
        let home = tree.path().join("home");
        fs::create_dir_all(&path_dir).unwrap();
        let node = home.join(".nvm/versions/node");
        let old_lexical = node.join("v20.9.0/bin/grok");
        let older_patch = node.join("v20.20.2/bin/grok");
        let newest = node.join("v20.20.10/bin/grok");
        let prerelease = node.join("v20.20.10-rc.1/bin/grok");
        let ancient = node.join("v18.20.0/bin/grok");
        let not_executable = node.join("v22.1.0/bin/grok");
        let ignored = node.join("not-a-version/bin/grok");
        for path in [
            &old_lexical,
            &older_patch,
            &newest,
            &prerelease,
            &ancient,
            &ignored,
        ] {
            write_file(path, 0o755);
        }
        write_file(&not_executable, 0o644);
        let now = SystemTime::now();
        set_mtime(&newest, UNIX_EPOCH + Duration::from_secs(10));
        set_mtime(&ancient, now + Duration::from_secs(2 * 60 * 60));
        set_mtime(&older_patch, now + Duration::from_secs(3 * 60 * 60));
        set_mtime(&old_lexical, now + Duration::from_secs(4 * 60 * 60));
        set_mtime(&not_executable, now + Duration::from_secs(5 * 60 * 60));
        let path_env = std::env::join_paths([&path_dir]).unwrap();

        let found = resolve_grok("", Some(path_env.as_os_str()), Some(&home)).unwrap();

        assert_eq!(found, newest);
    }

    #[test]
    fn non_executable_candidates_are_rejected() {
        let tree = TempTree::new();
        let path_dir = tree.path().join("path-bin");
        let home = tree.path().join("home");
        fs::create_dir_all(path_dir.join("grok")).unwrap();
        write_file(&path_dir.join("other"), 0o755);
        let bin = home.join(".grok/bin");
        write_file(&bin.join("grok-1.0.77"), 0o644);
        symlink("grok-1.0.77", bin.join("grok")).unwrap();
        write_file(&home.join(".nvm/versions/node/v20.20.2/bin/grok"), 0o644);
        fs::create_dir_all(home.join(".bun/bin/grok")).unwrap();
        let local = home.join(".local/bin/grok");
        write_file(&local, 0o755);
        let path_env = std::env::join_paths([&path_dir]).unwrap();

        let found = resolve_grok("", Some(path_env.as_os_str()), Some(&home)).unwrap();

        assert_eq!(found, local);
    }

    #[test]
    fn config_override_is_the_only_candidate() {
        let tree = TempTree::new();
        let path_dir = tree.path().join("path-bin");
        let home = tree.path().join("home");
        write_file(&path_dir.join("grok"), 0o755);
        write_file(&home.join(".grok/bin/grok"), 0o755);
        let chosen_dir = tree.path().join("chosen");
        write_file(&chosen_dir.join("grok-1.0.77"), 0o755);
        let chosen = chosen_dir.join("grok");
        symlink("grok-1.0.77", &chosen).unwrap();
        let path_env = std::env::join_paths([&path_dir]).unwrap();

        let found = resolve_grok(
            chosen.to_str().unwrap(),
            Some(path_env.as_os_str()),
            Some(&home),
        )
        .unwrap();
        assert_eq!(found, chosen);

        write_file(&home.join("bin/grok"), 0o755);
        let tilde = resolve_grok("~/bin/grok", Some(path_env.as_os_str()), Some(&home)).unwrap();
        assert_eq!(tilde, home.join("bin/grok"));

        let missing = resolve_grok(
            tree.path().join("missing").join("grok").to_str().unwrap(),
            Some(path_env.as_os_str()),
            Some(&home),
        )
        .unwrap_err();
        assert!(missing.starts_with("The grok CLI was not found."));
        assert!(missing.contains(tree.path().join("missing").join("grok").to_str().unwrap()));
        assert!(!missing.contains(path_dir.join("grok").to_str().unwrap()));
        assert!(!missing.contains("os error 2"));
        assert!(!missing.contains("No such file or directory"));
    }

    #[test]
    fn nothing_found_lists_places_without_the_os_error() {
        let tree = TempTree::new();
        let first = tree.path().join("first");
        let second = tree.path().join("second");
        let home = tree.path().join("home");
        fs::create_dir_all(&first).unwrap();
        write_file(&second.join("grok"), 0o644);
        fs::create_dir_all(home.join(".grok")).unwrap();
        fs::write(
            home.join(".grok/auth.json"),
            r#"{"access_token":"SECRETTOKENVALUE1234567890","email":"person@example.com","account_id":"acct_123"}"#,
        )
        .unwrap();
        let path_env = std::env::join_paths([&first, &second]).unwrap();

        let message = resolve_grok("", Some(path_env.as_os_str()), Some(&home)).unwrap_err();

        let expected = format!(
            "The grok CLI was not found. Places checked:\n- {first}/grok\n- {second}/grok\n- {home}/.grok/bin/grok\n- {home}/.nvm/versions/node/*/bin/grok\n- {home}/.bun/bin/grok\n- {home}/.local/bin/grok",
            first = first.display(),
            second = second.display(),
            home = home.display(),
        );
        assert_eq!(message, expected);
        assert!(!message.contains("os error 2"));
        assert!(!message.contains("No such file or directory"));
        assert!(!message.contains("SECRETTOKENVALUE"));
        assert!(!message.contains("person@example.com"));
        assert!(!message.contains("acct_123"));
        assert!(!message.contains("access_token"));
    }

    #[test]
    fn from_env_does_not_change_path_or_grok_home() {
        let _guard = crate::login::PROCESS_ENV_LOCK.lock().unwrap();
        let path = std::env::var_os("PATH");
        let grok_home = std::env::var_os("GROK_HOME");
        let _ = resolve_grok_from_env("");
        assert_eq!(std::env::var_os("PATH"), path);
        assert_eq!(std::env::var_os("GROK_HOME"), grok_home);
    }
}
