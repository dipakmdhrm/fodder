//! fodder — the GTK4 + libadwaita Fodder Reader viewer.
//!
//! Spawned on demand by the daemon; terminated on close so its memory is freed
//! while the daemon and tray stay resident. The full 3-pane UI lands in M4/M5.
//!
//! M1 scaffold: a trivial libadwaita application that smoke-tests that the
//! gtk4 / libadwaita / webkit6 0.11 trio links against the installed C libs.

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

const APP_ID: &str = "io.github.dipakmdhrm.Fodder";

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    // Don't take over stdin/args parsing in the scaffold.
    let empty: [&str; 0] = [];
    app.run_with_args(&empty);
    Ok(())
}

fn build_ui(app: &adw::Application) {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .default_width(960)
        .default_height(640)
        .title("Fodder Reader")
        .build();

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());

    let status = adw::StatusPage::builder()
        .icon_name("application-rss+xml-symbolic")
        .title("Fodder Reader")
        .description("Scaffold — the 3-pane reader lands in M4.")
        .build();
    toolbar.set_content(Some(&status));

    window.set_content(Some(&toolbar));
    window.present();

    // Prove the webkit6 binding is linked in (constructed then dropped so its
    // subprocesses exit immediately). The real reader toggle lands in M5.
    let _ = webkit6::WebView::new();
}
