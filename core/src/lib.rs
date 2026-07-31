//! fodder-core — shared library for the Fodder Reader daemon and viewer.
//!
//! Holds everything both binaries need: filesystem paths, the TOML config,
//! the SQLite store + migrations, the feed poller (conditional GET), feed
//! discovery, and the IPC message protocol.

/// The application ID: GApplication id, D-Bus/SNI name, and the basename of the
/// installed `.desktop` file and icon.
pub const APP_ID: &str = "io.github.dipakmdhrm.Fodder";

/// Human-facing application name.
pub const APP_NAME: &str = "Fodder";

/// One-line description of what Fodder is.
pub const APP_DESCRIPTION: &str = "A lightweight RSS/Atom/JSON-Feed reader for Linux desktops.";

/// The workspace version. All three crates share it, so `fodder-core`'s own
/// `CARGO_PKG_VERSION` is the canonical value the daemon and viewer report too.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Project homepage / source repository (inherited from the workspace manifest).
pub const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

/// SPDX license identifier (inherited from the workspace manifest).
pub const LICENSE: &str = env!("CARGO_PKG_LICENSE");

/// A multi-line version/about blurb shared by the `fodder`/`fodderd` `--version`
/// output. `bin` is the invoked binary name (e.g. `"fodderd"`), so the first
/// line reads like conventional CLI `--version` output.
pub fn version_blurb(bin: &str) -> String {
    format!(
        "{bin} {VERSION}\n\
         {APP_NAME} — {APP_DESCRIPTION}\n\
         Homepage: {REPOSITORY}\n\
         License:  {LICENSE}",
    )
}

#[cfg(test)]
mod version_tests {
    use super::*;

    #[test]
    fn blurb_leads_with_binary_and_version() {
        let out = version_blurb("fodderd");
        // Conventional `--version` first line: "<bin> <semver>".
        let first = out.lines().next().unwrap();
        assert_eq!(first, format!("fodderd {VERSION}"));
    }

    #[test]
    fn blurb_carries_app_metadata() {
        let out = version_blurb("fodder");
        assert!(out.contains(APP_NAME));
        assert!(out.contains(APP_DESCRIPTION));
        assert!(out.contains(REPOSITORY));
        assert!(out.contains(LICENSE));
        // The binary name is honored rather than hard-coded.
        assert!(out.starts_with("fodder "));
    }
}

/// Install rustls's `ring` crypto provider as the process default. Must be
/// called once, before any `reqwest` client is built (we use
/// `reqwest`'s `rustls-no-provider`, so no provider is installed automatically).
/// Idempotent: a second call is a no-op.
pub fn install_default_crypto() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

pub mod autostart;
pub mod config;
pub mod db;
pub mod discovery;
pub mod ipc;
pub mod models;
pub mod paths;
pub mod poller;

pub use config::Config;
pub use models::{Article, Feed, NewArticle};
