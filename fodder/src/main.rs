//! fodder — the GTK4 + libadwaita Fodder Reader viewer.
//!
//! Spawned on demand by the daemon; terminated on close so its memory is freed
//! while the daemon and tray stay resident.

mod app;
mod ipc_client;
mod reader;
mod runtime;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;

use app::Target;
use fodder_core::APP_ID;

fn main() -> glib::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    fodder_core::install_default_crypto();

    // Parse our own navigation args before GTK sees them.
    let target = parse_args();

    // NON_UNIQUE: the daemon arbitrates single-instance, not GApplication.
    let application = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();

    application.connect_activate(move |app| app::build(app, target));

    // We already consumed argv ourselves; don't let GApplication reparse it.
    let no_args: [&str; 0] = [];
    application.run_with_args(&no_args)
}

/// Parse `--feed <id>` / `--article <id>` from the command line.
fn parse_args() -> Target {
    let mut target = Target::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--feed" => target.feed = args.next().and_then(|v| v.parse().ok()),
            "--article" => target.article = args.next().and_then(|v| v.parse().ok()),
            "--webkit" => target.webkit = true,
            _ => {}
        }
    }
    target
}
