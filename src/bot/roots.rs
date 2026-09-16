use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub const DEFAULT_GROK_BOT_DIR_NAME: &str = "Grok Bot";
pub const SECRETS_FILE: &str = "sand-secrets.json";
pub const SESSION_MARKER_FILE: &str = "sand-session-marker.json";
pub const PERSISTENCE_DIR: &str = "sand-client-persistence";
/// Colon-separated extra Grok Bot userData dirs (same shape as PATH).
pub const CONFIG_DIRS_ENV: &str = "GROK_BOT_CONFIG_DIRS";

pub fn grok_bot_config_dir() -> PathBuf {
    grok_bot_config_dir_from(std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
}

pub fn grok_bot_config_dir_from(xdg: Option<PathBuf>) -> PathBuf {
    xdg_config_home_from(xdg).join(DEFAULT_GROK_BOT_DIR_NAME)
}

pub fn xdg_config_home() -> PathBuf {
    xdg_config_home_from(std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
}

pub fn xdg_config_home_from(xdg: Option<PathBuf>) -> PathBuf {
    xdg.filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| home_dir().join(".config"))
}

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Electron / Chromium OSCrypt libsecret `application=` label. Electron uses
/// `app.getName()` (the product name) for both userData (`~/.config/<name>`)
/// and the Safe Storage keyring item, so the directory basename is the right
/// label: `Grok Bot` vs `Grok Bot Work`.
pub fn keyring_app_name(config_dir: &Path) -> String {
    config_dir
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_GROK_BOT_DIR_NAME)
        .to_string()
}

pub fn discover_config_roots() -> Vec<PathBuf> {
    discover_config_roots_from(&xdg_config_home(), std::env::var_os(CONFIG_DIRS_ENV))
}

pub fn discover_config_roots_from(
    config_home: &Path,
    extra_dirs: Option<OsString>,
) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(entries) = std::fs::read_dir(config_home) {
        let mut found = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !is_discoverable_root(&path) {
                continue;
            }
            found.push(path);
        }
        sort_roots(&mut found);
        roots.extend(found);
    }
    if let Some(extra) = extra_dirs.filter(|s| !s.is_empty()) {
        for raw in std::env::split_paths(&extra) {
            if raw.as_os_str().is_empty() {
                continue;
            }
            let path = expand_user(raw);
            if !path.join(SECRETS_FILE).is_file() {
                continue;
            }
            if !roots.iter().any(|existing| same_path(existing, &path)) {
                roots.push(path);
            }
        }
    }
    roots
}

pub fn is_grok_bot_dir_name(name: &OsStr) -> bool {
    name.to_str()
        .is_some_and(|s| s == DEFAULT_GROK_BOT_DIR_NAME || s.starts_with("Grok Bot"))
}

pub fn root_used_at(dir: &Path) -> SystemTime {
    [
        dir.join(SECRETS_FILE),
        dir.join(SESSION_MARKER_FILE),
        dir.join(PERSISTENCE_DIR),
        dir.to_path_buf(),
    ]
    .into_iter()
    .filter_map(|p| std::fs::metadata(p).ok()?.modified().ok())
    .max()
    .unwrap_or(SystemTime::UNIX_EPOCH)
}

pub fn secrets_path(dir: &Path) -> PathBuf {
    dir.join(SECRETS_FILE)
}

pub fn session_marker_path(dir: &Path) -> PathBuf {
    dir.join(SESSION_MARKER_FILE)
}

pub fn persistence_path(dir: &Path) -> PathBuf {
    dir.join(PERSISTENCE_DIR)
}

fn is_discoverable_root(path: &Path) -> bool {
    path.is_dir()
        && path.file_name().is_some_and(is_grok_bot_dir_name)
        && path.join(SECRETS_FILE).is_file()
}

fn sort_roots(roots: &mut [PathBuf]) {
    roots.sort_by(|a, b| {
        let an = a.file_name().unwrap_or_default();
        let bn = b.file_name().unwrap_or_default();
        match (
            an == OsStr::new(DEFAULT_GROK_BOT_DIR_NAME),
            bn == OsStr::new(DEFAULT_GROK_BOT_DIR_NAME),
        ) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => an.cmp(bn),
        }
    });
}

fn expand_user(path: PathBuf) -> PathBuf {
    let Some(raw) = path.to_str() else {
        return path;
    };
    if raw == "~" {
        return home_dir();
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return home_dir().join(rest);
    }
    path
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process;
    use std::sync::atomic::{AtomicU32, Ordering};

    static NEXT: AtomicU32 = AtomicU32::new(0);

    fn temp_xdg() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grok-mon-bot-roots-{}-{}",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch_secrets(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(SECRETS_FILE), "{}\n").unwrap();
    }

    #[test]
    fn grok_bot_config_dir_uses_xdg() {
        assert_eq!(
            grok_bot_config_dir_from(Some(PathBuf::from("/tmp/xdg"))),
            PathBuf::from("/tmp/xdg/Grok Bot")
        );
        assert_eq!(
            grok_bot_config_dir_from(Some(PathBuf::from(""))),
            home_dir().join(".config/Grok Bot")
        );
    }

    #[test]
    fn keyring_name_is_directory_basename() {
        assert_eq!(
            keyring_app_name(Path::new("/home/user/.config/Grok Bot")),
            "Grok Bot"
        );
        assert_eq!(
            keyring_app_name(Path::new("/home/user/.config/Grok Bot Work")),
            "Grok Bot Work"
        );
        assert_eq!(keyring_app_name(Path::new("/")), "Grok Bot");
    }

    #[test]
    fn discovers_default_and_prefixed_dirs_with_secrets() {
        let xdg = temp_xdg();
        touch_secrets(&xdg.join("Grok Bot"));
        touch_secrets(&xdg.join("Grok Bot Work"));
        touch_secrets(&xdg.join("Other App"));
        fs::create_dir_all(xdg.join("Grok Bot Cache")).unwrap();
        fs::write(xdg.join("Grok Bot").join("not-secrets"), "").unwrap();

        let roots = discover_config_roots_from(&xdg, None);
        let names: Vec<_> = roots
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            vec!["Grok Bot".to_string(), "Grok Bot Work".to_string()]
        );
        let _ = fs::remove_dir_all(&xdg);
    }

    #[test]
    fn env_override_adds_non_prefixed_dir_and_dedups() {
        let xdg = temp_xdg();
        let extra_home = temp_xdg();
        touch_secrets(&xdg.join("Grok Bot"));
        let extra = extra_home.join("custom-bot");
        touch_secrets(&extra);

        let roots = discover_config_roots_from(
            &xdg,
            Some(OsString::from(format!(
                "{}:{}",
                extra.display(),
                xdg.join("Grok Bot").display()
            ))),
        );
        assert_eq!(roots.len(), 2);
        assert_eq!(
            roots[0].file_name().and_then(|n| n.to_str()),
            Some("Grok Bot")
        );
        assert_eq!(roots[1], extra);
        let _ = fs::remove_dir_all(&xdg);
        let _ = fs::remove_dir_all(&extra_home);
    }

    #[test]
    fn missing_secrets_are_not_roots() {
        let xdg = temp_xdg();
        fs::create_dir_all(xdg.join("Grok Bot")).unwrap();
        assert!(discover_config_roots_from(&xdg, None).is_empty());
        let _ = fs::remove_dir_all(&xdg);
    }
}
