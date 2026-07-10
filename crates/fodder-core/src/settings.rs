use rusqlite::params;

use crate::{Db, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// One of `schedule::INTERVAL_CHOICES_MINUTES`.
    pub poll_interval_minutes: u32,
    pub run_in_background: bool,
    pub autostart: bool,
    pub notifications: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            poll_interval_minutes: 60,
            run_in_background: true,
            autostart: false,
            notifications: true,
        }
    }
}

impl Db {
    pub fn settings(&self) -> Result<Settings> {
        let defaults = Settings::default();
        let get = |key: &str| -> Result<Option<String>> {
            use rusqlite::OptionalExtension;
            Ok(self
                .conn
                .query_row(
                    "SELECT value FROM settings WHERE key = ?1",
                    params![key],
                    |r| r.get(0),
                )
                .optional()?)
        };
        let bool_of = |v: Option<String>, default: bool| match v.as_deref() {
            Some("1") => true,
            Some("0") => false,
            _ => default,
        };
        Ok(Settings {
            poll_interval_minutes: get("poll_interval_minutes")?
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.poll_interval_minutes),
            run_in_background: bool_of(get("run_in_background")?, defaults.run_in_background),
            autostart: bool_of(get("autostart")?, defaults.autostart),
            notifications: bool_of(get("notifications")?, defaults.notifications),
        })
    }

    pub fn save_settings(&self, s: &Settings) -> Result<()> {
        let mut stmt = self
            .conn
            .prepare("INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)")?;
        let flag = |b: bool| if b { "1" } else { "0" };
        stmt.execute(params![
            "poll_interval_minutes",
            s.poll_interval_minutes.to_string()
        ])?;
        stmt.execute(params!["run_in_background", flag(s.run_in_background)])?;
        stmt.execute(params!["autostart", flag(s.autostart)])?;
        stmt.execute(params!["notifications", flag(s.notifications)])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_table_empty() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.settings().unwrap(), Settings::default());
    }

    #[test]
    fn round_trip() {
        let db = Db::open_in_memory().unwrap();
        let s = Settings {
            poll_interval_minutes: 240,
            run_in_background: false,
            autostart: true,
            notifications: false,
        };
        db.save_settings(&s).unwrap();
        assert_eq!(db.settings().unwrap(), s);
    }

    #[test]
    fn garbage_values_fall_back_to_defaults() {
        let db = Db::open_in_memory().unwrap();
        db.conn
            .execute_batch(
                "INSERT INTO settings VALUES ('poll_interval_minutes', 'soon'),
                                             ('notifications', 'yes please');",
            )
            .unwrap();
        assert_eq!(db.settings().unwrap(), Settings::default());
    }
}
