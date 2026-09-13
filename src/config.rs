use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const APP_ID: &str = "io.github.simple-systems-se.grok-mon";
pub const BOT_APP_ID: &str = "io.github.simple-systems-se.grok-mon-bot";
pub const API_APP_ID: &str = "io.github.simple-systems-se.grok-mon-api";
pub const USAGE_URL: &str = "https://grok.com/?_s=usage";
pub const CONSOLE_URL: &str = "https://console.x.ai";
pub const DEFAULT_ACCOUNT_ID: &str = "default";

fn default_true() -> bool {
    true
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
    #[serde(default)]
    pub account_labels: BTreeMap<String, String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_secs: 60,
            show_sparkline: false,
            show_percent: true,
            show_remaining: false,
            account_labels: BTreeMap::new(),
        }
    }
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
                .filter(|s| !s.trim().is_empty())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn account_label_empty_is_fallback() {
        let mut config = Config::default();
        assert_eq!(config.account_label("a"), None);
        assert_eq!(
            config.chip_identity("a", Some("mail@x.ai")).as_deref(),
            Some("mail@x.ai")
        );
        config.set_account_label("a".into(), "Work".into());
        assert_eq!(config.account_label("a"), Some("Work"));
        assert_eq!(
            config.chip_identity("a", Some("mail@x.ai")).as_deref(),
            Some("Work")
        );
        config.set_account_label("a".into(), String::new());
        assert_eq!(config.account_label("a"), None);
        config.set_account_label("a".into(), "   ".into());
        assert_eq!(config.account_label("a"), None);
    }
}
