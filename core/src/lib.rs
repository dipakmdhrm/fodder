//! fodder-core — shared library for the Fodder Reader daemon and viewer.
//!
//! Holds everything both binaries need: filesystem paths, the TOML config,
//! the SQLite store + migrations, the feed poller (conditional GET), feed
//! discovery, and the IPC message protocol.

/// The application ID: GApplication id, D-Bus/SNI name, and the basename of the
/// installed `.desktop` file and icon.
pub const APP_ID: &str = "io.github.dipakmdhrm.Fodder";

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
