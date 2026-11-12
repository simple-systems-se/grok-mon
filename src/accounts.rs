//! Panel icons come from configuration, in the order they are written.
//!
//! Nothing in this module scans CLI homes, Grok Bot profiles, or auth files.
//! Two icons that name the same home stay two icons. Build and Bot share one
//! weekly pool, so each icon carries one percent and those percents are not
//! added together.

use crate::config::AccountConfig;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedIcon {
    pub index: usize,
    pub id: String,
    pub panel_label: String,
    pub cli_home: PathBuf,
    pub cli_home_display: String,
    pub home_error: Option<String>,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
pub struct UsageReading {
    pub id: String,
    pub percent: f32,
}

pub fn user_home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

/// Expand a configured CLI home. `GROK_HOME` in the process environment is ignored.
pub fn expand_cli_home(raw: &str, user_home: Option<&Path>) -> Result<PathBuf, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("CLI home is empty".into());
    }
    let expanded = if trimmed == "~" {
        user_home
            .map(Path::to_path_buf)
            .ok_or_else(|| "CLI home starts with ~ but the user home is unknown".to_string())?
    } else if let Some(rest) = trimmed.strip_prefix("~/") {
        let base = user_home
            .ok_or_else(|| "CLI home starts with ~ but the user home is unknown".to_string())?;
        base.join(rest)
    } else if trimmed.starts_with('~') {
        return Err("CLI home can use ~/ for the current user only".into());
    } else {
        PathBuf::from(trimmed)
    };
    Ok(trim_trailing_slash(expanded))
}

pub fn resolve_icons(accounts: &[AccountConfig], user_home: Option<&Path>) -> Vec<ResolvedIcon> {
    accounts
        .iter()
        .enumerate()
        .map(|(index, account)| {
            let display = account.cli_home.trim().to_string();
            let (cli_home, home_error) = match expand_cli_home(&account.cli_home, user_home) {
                Ok(path) => (path, None),
                Err(err) => (PathBuf::new(), Some(err)),
            };
            ResolvedIcon {
                index,
                id: format!("{index}:{display}"),
                panel_label: panel_label(&account.label, &display),
                cli_home,
                cli_home_display: display,
                home_error,
            }
        })
        .collect()
}

pub fn panel_label(label: &str, cli_home_display: &str) -> String {
    let label = label.trim();
    if !label.is_empty() {
        return label.to_string();
    }
    let home = cli_home_display.trim();
    if home.is_empty() {
        "Grok".into()
    } else {
        home.to_string()
    }
}

/// The percent for one icon. A second reading is not added.
#[cfg(test)]
pub fn percent_for(id: &str, readings: &[UsageReading]) -> Option<f32> {
    readings
        .iter()
        .find(|reading| reading.id == id)
        .map(|reading| reading.percent)
}

fn trim_trailing_slash(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if text.len() > 1 && text.ends_with('/') {
        PathBuf::from(text.trim_end_matches('/'))
    } else {
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::default_accounts;

    #[test]
    fn default_icons_follow_config_order_and_ignore_process_home() {
        let _guard = crate::login::PROCESS_ENV_LOCK.lock().unwrap();
        let previous = std::env::var_os("GROK_HOME");
        // SAFETY: tests serialize environment changes with PROCESS_ENV_LOCK.
        unsafe { std::env::set_var("GROK_HOME", "/env/should-not-be-used") };
        let icons = resolve_icons(&default_accounts(), Some(Path::new("/users/jeff")));
        assert_eq!(icons.len(), 2);
        assert_eq!(icons[0].panel_label, "Grok (sim sys)");
        assert_eq!(icons[0].cli_home, PathBuf::from("/users/jeff/.grok"));
        assert_eq!(icons[0].cli_home_display, "~/.grok");
        assert!(icons[0].home_error.is_none());
        assert_eq!(icons[1].panel_label, "Grok (pers)");
        assert_eq!(
            icons[1].cli_home,
            PathBuf::from("/users/jeff/.grok-personal")
        );
        assert_ne!(icons[0].id, icons[1].id);
        assert_ne!(icons[0].cli_home, icons[1].cli_home);
        restore_var("GROK_HOME", previous);
    }

    #[test]
    fn custom_list_keeps_count_order_and_duplicate_homes() {
        let accounts = vec![
            AccountConfig {
                label: "Third".into(),
                cli_home: "/srv/grok-c".into(),
            },
            AccountConfig {
                label: "First".into(),
                cli_home: "/srv/grok-a".into(),
            },
            AccountConfig {
                label: "Also first".into(),
                cli_home: "/srv/grok-a".into(),
            },
        ];
        let icons = resolve_icons(&accounts, None);
        assert_eq!(icons.len(), 3);
        assert_eq!(
            icons
                .iter()
                .map(|icon| icon.panel_label.as_str())
                .collect::<Vec<_>>(),
            ["Third", "First", "Also first"]
        );
        assert_eq!(icons[1].cli_home, icons[2].cli_home);
        assert_ne!(icons[1].id, icons[2].id);
        assert!(icons.iter().all(|icon| icon.home_error.is_none()));
    }

    #[test]
    fn empty_list_does_not_invent_icons() {
        assert!(resolve_icons(&[], Some(Path::new("/users/jeff"))).is_empty());
    }

    #[test]
    fn empty_home_is_still_an_icon_with_an_error() {
        let accounts = [AccountConfig {
            label: "Broken".into(),
            cli_home: "  ".into(),
        }];
        let icons = resolve_icons(&accounts, Some(Path::new("/users/jeff")));
        assert_eq!(icons.len(), 1);
        assert_eq!(icons[0].panel_label, "Broken");
        assert!(icons[0].home_error.is_some());
    }

    #[test]
    fn tilde_expands_only_for_the_current_user() {
        let home = Path::new("/users/jeff");
        assert_eq!(
            expand_cli_home("~/.grok/", Some(home)).unwrap(),
            PathBuf::from("/users/jeff/.grok")
        );
        assert_eq!(
            expand_cli_home("~", Some(home)).unwrap(),
            PathBuf::from("/users/jeff")
        );
        assert_eq!(
            expand_cli_home("/srv/grok", Some(home)).unwrap(),
            PathBuf::from("/srv/grok")
        );
        assert!(expand_cli_home("~other/.grok", Some(home)).is_err());
        assert!(expand_cli_home("~/.grok", None).is_err());
    }

    #[test]
    fn shared_weekly_readings_are_not_added_across_icons() {
        // 12.5 is a shared-pool headline. 8 and 3 would be product rows inside
        // that same pool; they are not separate readings and are not added.
        let readings = [
            UsageReading {
                id: "0:~/.grok".into(),
                percent: 12.5,
            },
            UsageReading {
                id: "1:~/.grok-personal".into(),
                percent: 40.0,
            },
        ];
        assert_eq!(percent_for("0:~/.grok", &readings), Some(12.5));
        assert_eq!(percent_for("1:~/.grok-personal", &readings), Some(40.0));
        assert_eq!(percent_for("missing", &readings), None);
        assert_ne!(percent_for("0:~/.grok", &readings), Some(12.5 + 40.0));
        assert_ne!(percent_for("0:~/.grok", &readings), Some(8.0 + 3.0));
        assert_ne!(percent_for("0:~/.grok", &readings), Some(12.5 + 8.0 + 3.0));
    }

    fn restore_var(key: &str, previous: Option<std::ffi::OsString>) {
        match previous {
            // SAFETY: tests serialize environment changes with PROCESS_ENV_LOCK.
            Some(value) => unsafe { std::env::set_var(key, value) },
            None => unsafe { std::env::remove_var(key) },
        }
    }
}
