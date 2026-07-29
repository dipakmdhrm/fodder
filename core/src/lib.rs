//! fodder-core — shared library for the Fodder Reader daemon and viewer.
//!
//! Holds everything both binaries need: filesystem paths, the TOML config,
//! the SQLite store + migrations, the feed poller (conditional GET), feed
//! discovery, and the IPC message protocol.

pub mod config;
pub mod db;
pub mod discovery;
pub mod ipc;
pub mod models;
pub mod paths;
pub mod poller;

pub use config::Config;
pub use models::{Article, Feed, NewArticle};
