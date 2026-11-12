//! COSMIC applet settings.
//!
//! Account icons are the `accounts` key under
//! `~/.config/cosmic/io.github.simple-systems-se.grok-mon/v1/accounts`.
//! The file is RON. Each entry has `label` and `cli_home`. `cli_home` is that
//! icon's Grok CLI home (`GROK_HOME`), not a path to a credential file.
//! `~` at the start is expanded for the current user.
//!
//! When the key is missing, the applet uses the two built-in icons. An empty
//! list shows no icons. A file that does not parse shows no icons.

use cosmic::cosmic_config::{
    self, ConfigGet, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const APP_ID: &str = "io.github.simple-systems-se.grok-mon";
pub const BOT_APP_ID: &str = "io.github.simple-systems-se.grok-mon-bot";
pub const API_APP_ID: &str = "io.github.simple-systems-se.grok-mon-api";
pub const USAGE_URL: &str = "https://grok.com/?_s=usage";
pub const CONSOLE_URL: &str = "https://console.x.ai";
pub const DEFAULT_ACCOUNT_ID: &str = "default";

pub const ACCOUNTS_CONFIG_PATH: &str =
    "~/.config/cosmic/io.github.simple-systems-se.grok-mon/v1/accounts";

fn default_true() -> bool {
    true
}

pub fn default_accounts() -> Vec<AccountConfig> {
    vec![
        AccountConfig {
            label: "Grok (sim sys)".into(),
            cli_home: "~/.grok".into(),
        },
        AccountConfig {
            label: "Grok (pers)".into(),
            cli_home: "~/.grok-personal".into(),
        },
    ]
}

/// One panel icon. `cli_home` is the Grok CLI home for that icon only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountConfig {
    pub label: String,
    pub cli_home: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, CosmicConfigEntry)]
#[version = 1]
pub struct Config {
    pub poll_secs: u64,
    pub show_sparkline: bool,
    #[serde(default = "default_true")]
    pub show_percent: bool,
    #[serde(default)]
    pub show_remaining: bool,
    #[serde(default = "default_true")]
    pub show_pace: bool,
    #[serde(default)]
    pub account_labels: BTreeMap<String, String>,
    /// Panel icons in display order. Missing key means [`default_accounts`].
    #[serde(default = "default_accounts")]
    pub accounts: Vec<AccountConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_secs: 60,
            show_sparkline: false,
            show_percent: true,
            show_remaining: false,
            show_pace: true,
            account_labels: BTreeMap::new(),
            accounts: default_accounts(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedConfig {
    pub config: Config,
    /// The accounts file exists but could not be parsed. Icons are not guessed.
    pub accounts_unreadable: bool,
}

impl Config {
    pub fn poll_duration(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.poll_secs.clamp(15, 600))
    }

    pub fn display_percent(&self, used: f32) -> f32 {
        let used = used.clamp(0.0, 100.0).round();
        if self.show_remaining {
            100.0 - used
        } else {
            used
        }
    }

    pub fn account_label(&self, id: &str) -> Option<&str> {
        self.account_labels
            .get(id)
            .map(String::as_str)
            .filter(|s| !s.trim().is_empty())
    }

    pub fn set_account_label(&mut self, id: String, value: String) {
        if value.is_empty() {
            self.account_labels.remove(&id);
        } else {
            self.account_labels.insert(id, value);
        }
    }

    pub fn chip_identity(&self, id: &str, fallback: Option<&str>) -> Option<String> {
        self.account_label(id).map(str::to_string).or_else(|| {
            fallback
                .map(str::to_string)
                .filter(|value| !value.trim().is_empty())
        })
    }
}

/// Load COSMIC config. A missing `accounts` key keeps the built-in icons.
/// An unreadable `accounts` file yields no icons.
pub fn load_from(handler: &cosmic_config::Config) -> LoadedConfig {
    let mut config = match Config::get_entry(handler) {
        Ok(config) => config,
        Err((_errors, config)) => config,
    };
    match handler.get::<Vec<AccountConfig>>("accounts") {
        Ok(accounts) => {
            config.accounts = accounts;
            LoadedConfig {
                config,
                accounts_unreadable: false,
            }
        }
        Err(cosmic_config::Error::NotFound | cosmic_config::Error::NoConfigDirectory) => {
            LoadedConfig {
                config,
                accounts_unreadable: false,
            }
        }
        Err(_) => {
            config.accounts.clear();
            LoadedConfig {
                config,
                accounts_unreadable: true,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn display_percent_used_and_remaining() {
        let used = Config {
            show_remaining: false,
            ..Config::default()
        };
        let remaining = Config {
            show_remaining: true,
            ..Config::default()
        };
        assert_eq!(used.display_percent(23.4), 23.0);
        assert_eq!(used.display_percent(23.5), 24.0);
        assert_eq!(remaining.display_percent(23.4), 77.0);
        assert_eq!(remaining.display_percent(23.5), 76.0);
        assert_eq!(remaining.display_percent(100.0), 0.0);
        assert_eq!(remaining.display_percent(0.0), 100.0);
    }

    #[test]
    fn account_label_empty_is_absent() {
        let mut config = Config::default();
        assert_eq!(config.account_label("a"), None);
        config.set_account_label("a".into(), "Work".into());
        assert_eq!(config.account_label("a"), Some("Work"));
        config.set_account_label("a".into(), String::new());
        assert_eq!(config.account_label("a"), None);
        config.set_account_label("a".into(), "   ".into());
        assert_eq!(config.account_label("a"), None);
    }

    #[test]
    fn pace_defaults_on() {
        assert!(Config::default().show_pace);
    }

    #[test]
    fn default_config_is_the_two_named_homes_in_order() {
        let accounts = Config::default().accounts;
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts[0].label, "Grok (sim sys)");
        assert_eq!(accounts[0].cli_home, "~/.grok");
        assert_eq!(accounts[1].label, "Grok (pers)");
        assert_eq!(accounts[1].cli_home, "~/.grok-personal");
    }

    #[test]
    fn missing_accounts_field_uses_the_two_defaults() {
        let raw = r#"(
            poll_secs: 30,
            show_sparkline: false,
            show_percent: true,
            show_remaining: false,
            show_pace: true,
            account_labels: {},
        )"#;
        let config: Config = ron::from_str(raw).unwrap();
        assert_eq!(config.poll_secs, 30);
        assert_eq!(config.accounts, default_accounts());
    }

    #[test]
    fn empty_accounts_list_stays_empty() {
        let raw = r#"(
            poll_secs: 60,
            show_sparkline: false,
            show_percent: true,
            show_remaining: false,
            show_pace: true,
            account_labels: {},
            accounts: [],
        )"#;
        let config: Config = ron::from_str(raw).unwrap();
        assert!(config.accounts.is_empty());
    }

    #[test]
    fn documented_accounts_ron_keeps_count_and_order() {
        let raw = r#"[
            (label: "Grok (pers)", cli_home: "~/.grok-personal"),
            (label: "Grok (sim sys)", cli_home: "~/.grok"),
            (label: "Desk", cli_home: "/srv/grok-desk"),
        ]"#;
        let accounts: Vec<AccountConfig> = ron::from_str(raw).unwrap();
        assert_eq!(accounts.len(), 3);
        assert_eq!(accounts[0].label, "Grok (pers)");
        assert_eq!(accounts[0].cli_home, "~/.grok-personal");
        assert_eq!(accounts[1].cli_home, "~/.grok");
        assert_eq!(accounts[2].label, "Desk");
        assert_eq!(accounts[2].cli_home, "/srv/grok-desk");
    }

    #[test]
    fn file_accounts_list_is_what_the_panel_loads() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("grok-mon-config-{nanos}"));
        let _ = fs::remove_dir_all(&root);
        let ctx =
            cosmic_config::Config::with_custom_path(APP_ID, Config::VERSION, root.clone()).unwrap();
        let missing = load_from(&ctx);
        assert!(!missing.accounts_unreadable);
        assert_eq!(missing.config.accounts, default_accounts());

        let accounts_path = root.join("cosmic").join(APP_ID).join("v1").join("accounts");
        fs::write(
            &accounts_path,
            r#"[
    (label: "Grok (pers)", cli_home: "~/.grok-personal"),
    (label: "Grok (sim sys)", cli_home: "~/.grok"),
]
"#,
        )
        .unwrap();
        let loaded = load_from(&ctx);
        assert!(!loaded.accounts_unreadable);
        assert_eq!(loaded.config.accounts.len(), 2);
        assert_eq!(loaded.config.accounts[0].label, "Grok (pers)");
        assert_eq!(loaded.config.accounts[0].cli_home, "~/.grok-personal");
        assert_eq!(loaded.config.accounts[1].label, "Grok (sim sys)");
        assert_eq!(loaded.config.poll_secs, 60);

        fs::write(&accounts_path, "[]\n").unwrap();
        let empty = load_from(&ctx);
        assert!(!empty.accounts_unreadable);
        assert!(empty.config.accounts.is_empty());

        fs::write(&accounts_path, "this is not a list\n").unwrap();
        let broken = load_from(&ctx);
        assert!(broken.accounts_unreadable);
        assert!(broken.config.accounts.is_empty());

        let _ = fs::remove_dir_all(&root);
    }
}
