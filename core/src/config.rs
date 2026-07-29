//! TOML configuration at `~/.config/fodder/config.toml`.

use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The minimum allowed poll interval, in minutes. Enforced on load and save so
/// a hand-edited config can never poll more aggressively than this.
pub const MIN_POLL_INTERVAL_MINUTES: u32 = 5;
/// Default poll interval when no config exists yet.
pub const DEFAULT_POLL_INTERVAL_MINUTES: u32 = 30;
/// Default daily-reminder time (local, 24-hour).
pub const DEFAULT_REMINDER_TIME: &str = "10:00";

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("poll interval must be at least {MIN_POLL_INTERVAL_MINUTES} minutes, got {0}")]
    PollIntervalTooSmall(u32),
    #[error("reading config: {0}")]
    Io(#[from] std::io::Error),
    #[error("parsing config TOML: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("serializing config TOML: {0}")]
    Serialize(#[from] toml::ser::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Poll interval in minutes; validated to be >= [`MIN_POLL_INTERVAL_MINUTES`].
    pub poll_interval_minutes: u32,
    /// Mirror of whether the autostart `.desktop` file is installed. The file
    /// on disk is the source of truth; this is a convenience cache for the UI.
    pub autostart: bool,
    /// Max concurrent feed fetches per poll cycle.
    pub poll_concurrency: usize,
    /// Master switch for all desktop notifications.
    pub notifications_enabled: bool,
    /// Notify (per feed) when polling finds new articles.
    pub notify_new_articles: bool,
    /// Send a once-a-day reminder to read, if there are unread articles.
    pub daily_reminder_enabled: bool,
    /// Local time of the daily reminder, as `"HH:MM"` (24-hour).
    pub daily_reminder_time: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval_minutes: DEFAULT_POLL_INTERVAL_MINUTES,
            autostart: false,
            poll_concurrency: 8,
            notifications_enabled: true,
            notify_new_articles: true,
            daily_reminder_enabled: false,
            daily_reminder_time: DEFAULT_REMINDER_TIME.to_string(),
        }
    }
}

impl Config {
    /// Clamp/validate invariants. Returns an error if the interval is below the
    /// minimum rather than silently clamping, so the UI can surface it.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.poll_interval_minutes < MIN_POLL_INTERVAL_MINUTES {
            return Err(ConfigError::PollIntervalTooSmall(
                self.poll_interval_minutes,
            ));
        }
        Ok(())
    }

    /// Load from `path`. A missing file yields [`Config::default`] (and is not
    /// an error). A present-but-invalid interval is clamped up to the minimum
    /// so a bad on-disk value can never poll too aggressively.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let mut cfg: Config = toml::from_str(&text)?;
                if cfg.poll_interval_minutes < MIN_POLL_INTERVAL_MINUTES {
                    cfg.poll_interval_minutes = MIN_POLL_INTERVAL_MINUTES;
                }
                if cfg.poll_concurrency == 0 {
                    cfg.poll_concurrency = 1;
                }
                if cfg.reminder_hm().is_none() {
                    cfg.daily_reminder_time = DEFAULT_REMINDER_TIME.to_string();
                }
                Ok(cfg)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(ConfigError::Io(e)),
        }
    }

    /// Validate then write to `path` atomically (write temp + rename).
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        self.validate()?;
        let text = toml::to_string_pretty(self)?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text.as_bytes())?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// The poll interval as a [`std::time::Duration`].
    pub fn poll_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(u64::from(self.poll_interval_minutes) * 60)
    }

    /// Parse [`Config::daily_reminder_time`] into `(hour, minute)`, or `None` if
    /// it isn't a valid 24-hour `"HH:MM"`.
    pub fn reminder_hm(&self) -> Option<(u32, u32)> {
        let (h, m) = self.daily_reminder_time.split_once(':')?;
        let hour: u32 = h.trim().parse().ok()?;
        let minute: u32 = m.trim().parse().ok()?;
        (hour < 24 && minute < 60).then_some((hour, minute))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_valid() {
        assert!(Config::default().validate().is_ok());
    }

    #[test]
    fn rejects_below_minimum_on_validate() {
        let cfg = Config {
            poll_interval_minutes: 1,
            ..Config::default()
        };
        assert!(matches!(
            cfg.validate(),
            Err(ConfigError::PollIntervalTooSmall(1))
        ));
    }

    #[test]
    fn load_missing_file_is_default() {
        let path = std::path::Path::new("/nonexistent/fodder/does-not-exist.toml");
        let cfg = Config::load(path).unwrap();
        assert_eq!(cfg.poll_interval_minutes, DEFAULT_POLL_INTERVAL_MINUTES);
    }

    #[test]
    fn load_clamps_too_small_interval() {
        let dir = std::env::temp_dir().join("fodder-cfg-test-clamp");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "poll_interval_minutes = 2\n").unwrap();
        let cfg = Config::load(&path).unwrap();
        assert_eq!(cfg.poll_interval_minutes, MIN_POLL_INTERVAL_MINUTES);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_then_load_roundtrips() {
        let dir = std::env::temp_dir().join("fodder-cfg-test-roundtrip");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let cfg = Config {
            poll_interval_minutes: 45,
            autostart: true,
            poll_concurrency: 4,
            daily_reminder_enabled: true,
            daily_reminder_time: "07:30".to_string(),
            notify_new_articles: false,
            ..Config::default()
        };
        cfg.save(&path).unwrap();
        let back = Config::load(&path).unwrap();
        assert_eq!(back.poll_interval_minutes, 45);
        assert!(back.autostart);
        assert_eq!(back.poll_concurrency, 4);
        assert!(back.daily_reminder_enabled);
        assert_eq!(back.daily_reminder_time, "07:30");
        assert!(!back.notify_new_articles);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reminder_time_parsing() {
        let mut cfg = Config::default();
        assert_eq!(cfg.reminder_hm(), Some((10, 0)));
        cfg.daily_reminder_time = "07:05".into();
        assert_eq!(cfg.reminder_hm(), Some((7, 5)));
        cfg.daily_reminder_time = "25:00".into();
        assert_eq!(cfg.reminder_hm(), None);
        cfg.daily_reminder_time = "garbage".into();
        assert_eq!(cfg.reminder_hm(), None);
    }

    #[test]
    fn load_resets_invalid_reminder_time() {
        let dir = std::env::temp_dir().join("fodder-cfg-test-reminder");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "daily_reminder_time = \"99:99\"\n").unwrap();
        let cfg = Config::load(&path).unwrap();
        assert_eq!(cfg.daily_reminder_time, DEFAULT_REMINDER_TIME);
        std::fs::remove_dir_all(&dir).ok();
    }
}
