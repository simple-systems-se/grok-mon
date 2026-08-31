use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use serde::{Deserialize, Serialize};

pub const APP_ID: &str = "io.github.simple-systems-se.grok-mon";
pub const BOT_APP_ID: &str = "io.github.simple-systems-se.grok-mon-bot";
pub const API_APP_ID: &str = "io.github.simple-systems-se.grok-mon-api";
pub const USAGE_URL: &str = "https://grok.com/?_s=usage";
pub const CONSOLE_URL: &str = "https://console.x.ai";

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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_secs: 60,
            show_sparkline: false,
            show_percent: true,
            show_remaining: false,
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
}
