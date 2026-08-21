use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use serde::{Deserialize, Serialize};

pub const APP_ID: &str = "io.github.simple-systems-se.grok-mon";
pub const USAGE_URL: &str = "https://grok.com/?_s=usage";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, CosmicConfigEntry)]
#[version = 1]
pub struct Config {
    pub poll_secs: u64,
    pub show_sparkline: bool,
    pub warn_percent: u8,
    pub critical_percent: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_secs: 60,
            show_sparkline: false,
            warn_percent: 70,
            critical_percent: 90,
        }
    }
}

impl Config {
    pub fn poll_duration(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.poll_secs.clamp(15, 600))
    }
}
