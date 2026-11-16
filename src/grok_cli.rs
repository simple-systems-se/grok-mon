//! Find the `grok` executable without changing the panel process environment.
//!
//! Search order: optional `grok_path`, then `PATH`, then `~/.grok/bin/grok`,
//! then the newest nvm Node version's `bin/grok`, then `~/.bun/bin/grok`,
//! then `~/.local/bin/grok`. A hit must be a regular file that is executable
//! after symlinks are followed.

use crate::accounts::{self, user_home_dir};
use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub const NOT_FOUND_PREFIX: &str = "The grok CLI was not found.";

pub struct Lookup<'a> {
    pub configured: &'a str,
    pub path_env: Option<&'a OsStr>,
    pub home: Option<&'a Path>,
}

/// Read `PATH` and `HOME` and resolve `grok`. Does not write either variable,
/// and does not write `GROK_HOME`.
pub fn resolve_from_env(configured: &str) -> Result<PathBuf, String> {
    let path_env = std::env::var_os("PATH");
    let home = user_home_dir();
    resolve(&Lookup {
        configured,
        path_env: path_env.as_deref(),
        home: home.as_deref(),
    })
}

pub fn resolve(lookup: &Lookup<'_>) -> Result<PathBuf, String> {
    let mut checked = Vec::new();
    if let Some(path) = take_configured(lookup, &mut checked) {
        return Ok(path);
    }
    if let Some(path) = take_from_path(lookup, &mut checked) {
        return Ok(path);
    }
    if let Some(path) = take_from_home(lookup, &mut checked) {
        return Ok(path);
    }
    Err(not_found_message(&checked))
}

pub fn not_found_message(checked: &[String]) -> String {
    let mut message = format!("{NOT_FOUND_PREFIX} Places checked:");
    if checked.is_empty() {
        message.push_str("\n- (none)");
    }
    for place in checked {
        message.push('\n');
        message.push_str("- ");
        message.push_str(place);
    }
    message
}

fn take_configured(lookup: &Lookup<'_>, checked: &mut Vec<String>) -> Option<PathBuf> {
    let raw = lookup.configured.trim();
    if raw.is_empty() {
        return None;
    }
    let path = match accounts::expand_cli_home(raw, lookup.home) {
        Ok(path) => path,
        Err(_) => PathBuf::from(raw),
    };
    checked.push(format!("configured grok_path {}", path.display()));
    accept(&path)
}

fn take_from_path(lookup: &Lookup<'_>, checked: &mut Vec<String>) -> Option<PathBuf> {
    let Some(path_env) = lookup.path_env else {
        checked.push("PATH (unset)".into());
        return None;
    };
    if path_env.is_empty() {
        checked.push("PATH (empty)".into());
        return None;
    }
    for dir in std::env::split_paths(path_env) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let candidate = dir.join("grok");
        checked.push(candidate.display().to_string());
        if let Some(path) = accept(&candidate) {
            return Some(path);
        }
    }
    None
}

fn take_from_home(lookup: &Lookup<'_>, checked: &mut Vec<String>) -> Option<PathBuf> {
    let Some(home) = lookup.home else {
        checked.push(
            "HOME is unset, so ~/.grok/bin/grok, ~/.nvm/versions/node/*/bin/grok, ~/.bun/bin/grok, and ~/.local/bin/grok were not checked"
                .into(),
        );
        return None;
    };
    let official = home.join(".grok/bin/grok");
    checked.push(official.display().to_string());
    if let Some(path) = accept(&official) {
        return Some(path);
    }
    if let Some(path) = newest_nvm(home, checked) {
        return Some(path);
    }
    for relative in [".bun/bin/grok", ".local/bin/grok"] {
        let candidate = home.join(relative);
        checked.push(candidate.display().to_string());
        if let Some(path) = accept(&candidate) {
            return Some(path);
        }
    }
    None
}

struct NvmHit {
    version: NodeVersion,
    name: String,
    path: PathBuf,
}

fn newest_nvm(home: &Path, checked: &mut Vec<String>) -> Option<PathBuf> {
    let root = home.join(".nvm/versions/node");
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(_) => {
            checked.push(format!("{}/*/bin/grok", root.display()));
            return None;
        }
    };
    let mut found = Vec::new();
    let mut saw = false;
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        saw = true;
        let candidate = entry.path().join("bin/grok");
        checked.push(candidate.display().to_string());
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let Some(version) = parse_node_version(&name) else {
            continue;
        };
        if is_regular_executable(&candidate) {
            found.push(NvmHit {
                version,
                name,
                path: candidate,
            });
        }
    }
    if !saw {
        checked.push(format!("{}/*/bin/grok", root.display()));
    }
    found.sort_by(|a, b| b.version.cmp(&a.version).then_with(|| b.name.cmp(&a.name)));
    found.into_iter().next().map(|hit| hit.path)
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct NodeVersion {
    major: u64,
    minor: u64,
    patch: u64,
    /// A release sorts above a prerelease with the same numbers.
    stable: bool,
}

fn parse_node_version(name: &str) -> Option<NodeVersion> {
    let rest = name.strip_prefix('v').unwrap_or(name);
    if !rest.chars().next()?.is_ascii_digit() {
        return None;
    }
    let (numeric, stable) = match rest.split_once('-') {
        Some((numeric, _)) => (numeric, false),
        None => (rest, true),
    };
    let mut parts = numeric.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = match parts.next() {
        Some(part) => part.parse().ok()?,
        None => 0,
    };
    let patch = match parts.next() {
        Some(part) => part.parse().ok()?,
        None => 0,
    };
    if parts.next().is_some() {
        return None;
    }
    Some(NodeVersion {
        major,
        minor,
        patch,
        stable,
    })
}

fn accept(path: &Path) -> Option<PathBuf> {
    is_regular_executable(path).then(|| path.to_path_buf())
}

/// `metadata` follows symlinks. The target must be a regular file with an
/// execute bit. Directories, broken links, and mode `0644` files are rejected.
fn is_regular_executable(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    meta.file_type().is_file() && meta.permissions().mode() & 0o111 != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::symlink;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[test]
    fn path_hit_wins_over_later_install_locations() {
        let root = temp_dir("path");
        let bin = root.join("bin");
        let other = root.join("other");
        let home = root.join("home");
        write_exec(&bin.join("grok"), "path");
        write_exec(&other.join("grok"), "other");
        write_exec(&home.join(".grok/bin/grok"), "home");
        let path_env = std::env::join_paths([&bin, &other]).unwrap();
        let found = resolve(&Lookup {
            configured: "",
            path_env: Some(path_env.as_os_str()),
            home: Some(&home),
        })
        .unwrap();
        assert_eq!(found, bin.join("grok"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn grok_bin_fallback_follows_the_installer_symlink() {
        let root = temp_dir("fallback");
        let path_dir = root.join("empty-path");
        fs::create_dir_all(&path_dir).unwrap();
        let home = root.join("home");
        let bin = home.join(".grok/bin");
        fs::create_dir_all(&bin).unwrap();
        write_exec(&bin.join("grok-1.0.77"), "official");
        symlink("grok-1.0.77", bin.join("grok")).unwrap();
        let found = resolve(&Lookup {
            configured: "  ",
            path_env: Some(path_dir.as_os_str()),
            home: Some(&home),
        })
        .unwrap();
        assert_eq!(found, bin.join("grok"));
        let meta = fs::metadata(&found).unwrap();
        assert!(meta.is_file());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn nvm_picks_the_newest_semantic_version_not_mtime() {
        let root = temp_dir("nvm");
        let path_dir = root.join("empty-path");
        fs::create_dir_all(&path_dir).unwrap();
        let home = root.join("home");
        let versions = [
            ("v18.19.1", 1_700_000_000),
            ("v20.9.0", 1_700_000_000),
            ("v20.20.2", 1_700_000_000),
            ("v20.20.10", 10),
            ("v20.20.10-rc.1", 1_800_000_000),
        ];
        for (name, secs) in versions {
            let grok = home.join(".nvm/versions/node").join(name).join("bin/grok");
            write_exec(&grok, name);
            set_mtime(&grok, secs);
        }
        write_exec(
            &home.join(".nvm/versions/node/not-a-version/bin/grok"),
            "skip",
        );
        let found = resolve(&Lookup {
            configured: "",
            path_env: Some(path_dir.as_os_str()),
            home: Some(&home),
        })
        .unwrap();
        assert_eq!(
            found,
            home.join(".nvm/versions/node/v20.20.10/bin/grok"),
            "v20.20.10 is newer than v20.20.2 and v20.9.0, and a prerelease does not win"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn non_executable_and_non_files_are_rejected() {
        let root = temp_dir("mode");
        let path_dir = root.join("path");
        let home = root.join("home");
        write_file(&path_dir.join("grok"), "not-exec");
        write_file(&home.join(".grok/bin/grok"), "not-exec");
        let broken = home.join(".bun/bin/grok");
        fs::create_dir_all(broken.parent().unwrap()).unwrap();
        symlink("missing-target", &broken).unwrap();
        write_exec(&home.join(".local/bin/grok"), "local");
        let extra = root.join("extra");
        fs::create_dir_all(extra.join("grok")).unwrap();
        let path_env = std::env::join_paths([&path_dir, &extra]).unwrap();
        let found = resolve(&Lookup {
            configured: "",
            path_env: Some(path_env.as_os_str()),
            home: Some(&home),
        })
        .unwrap();
        assert_eq!(found, home.join(".local/bin/grok"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn configured_path_overrides_path_and_home() {
        let root = temp_dir("override");
        let path_dir = root.join("path");
        let home = root.join("home");
        let custom = root.join("custom/grok");
        write_exec(&path_dir.join("grok"), "path");
        write_exec(&home.join(".grok/bin/grok"), "home");
        write_exec(&custom, "custom");
        let configured = custom.display().to_string();
        let found = resolve(&Lookup {
            configured: &configured,
            path_env: Some(path_dir.as_os_str()),
            home: Some(&home),
        })
        .unwrap();
        assert_eq!(found, custom);

        let tilde_home = root.join("tilde");
        let tilde_bin = tilde_home.join("bin/grok");
        write_exec(&tilde_bin, "tilde");
        let found = resolve(&Lookup {
            configured: "~/bin/grok",
            path_env: Some(path_dir.as_os_str()),
            home: Some(&tilde_home),
        })
        .unwrap();
        assert_eq!(found, tilde_bin);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn nothing_found_lists_places_and_hides_the_os_error() {
        let root = temp_dir("missing");
        let path_dir = root.join("path");
        fs::create_dir_all(&path_dir).unwrap();
        let home = root.join("home");
        fs::create_dir_all(&home).unwrap();
        write_file(&home.join(".grok/bin/grok"), "not-exec");
        let err = resolve(&Lookup {
            configured: "/no/such/grok",
            path_env: Some(path_dir.as_os_str()),
            home: Some(&home),
        })
        .unwrap_err();
        assert!(err.starts_with(NOT_FOUND_PREFIX), "{err}");
        assert!(err.contains("configured grok_path /no/such/grok"), "{err}");
        assert!(
            err.contains(&path_dir.join("grok").display().to_string()),
            "{err}"
        );
        assert!(
            err.contains(&home.join(".grok/bin/grok").display().to_string()),
            "{err}"
        );
        assert!(
            err.contains(
                &home
                    .join(".nvm/versions/node/*/bin/grok")
                    .display()
                    .to_string()
            ),
            "{err}"
        );
        assert!(
            err.contains(&home.join(".bun/bin/grok").display().to_string()),
            "{err}"
        );
        assert!(
            err.contains(&home.join(".local/bin/grok").display().to_string()),
            "{err}"
        );
        assert!(!err.contains("os error"), "{err}");
        assert!(!err.to_ascii_lowercase().contains("no such file"), "{err}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_does_not_change_path_home_or_grok_home() {
        let _guard = crate::login::PROCESS_ENV_LOCK.lock().unwrap();
        let path = std::env::var_os("PATH");
        let home = std::env::var_os("HOME");
        let grok_home = std::env::var_os("GROK_HOME");
        let _ = resolve_from_env("");
        assert_eq!(std::env::var_os("PATH"), path);
        assert_eq!(std::env::var_os("HOME"), home);
        assert_eq!(std::env::var_os("GROK_HOME"), grok_home);
    }

    #[test]
    fn node_versions_compare_by_number() {
        let older = parse_node_version("v20.9.0").unwrap();
        let newer = parse_node_version("v20.20.10").unwrap();
        let same = parse_node_version("v20.20.10").unwrap();
        let pre = parse_node_version("v20.20.10-rc.1").unwrap();
        assert!(newer > older);
        assert!(same > pre);
        assert!(parse_node_version("current").is_none());
        assert!(parse_node_version("v20.20.10.1").is_none());
    }

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "grok-mon-cli-{name}-{}-{nanos}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_exec(path: &Path, body: &str) {
        write_file(path, body);
        let mut perms = fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).unwrap();
    }

    fn write_file(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut file = fs::File::create(path).unwrap();
        file.write_all(body.as_bytes()).unwrap();
        let mut perms = fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o644);
        fs::set_permissions(path, perms).unwrap();
    }

    fn set_mtime(path: &Path, secs: u64) {
        let when = UNIX_EPOCH + Duration::from_secs(secs);
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(when)
            .unwrap();
    }
}
