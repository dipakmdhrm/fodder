mod article_view;
mod daemon_client;
mod discover;
mod render;
mod settings_dialog;
mod window;

use gtk4 as gtk;
use libadwaita as adw;

use adw::prelude::*;
use fodder_core::ipc;

fn main() -> gtk::glib::ExitCode {
    // Handled before GApplication sees (and rejects) the argument.
    if std::env::args().any(|a| a == "--version" || a == "-V") {
        println!("fodder {}", env!("CARGO_PKG_VERSION"));
        return gtk::glib::ExitCode::SUCCESS;
    }
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let app = adw::Application::builder()
        .application_id(ipc::APP_ID)
        .build();
    app.connect_activate(|app| {
        // GApplication single-instancing: a second launch lands here in the
        // primary instance — just raise the existing window.
        if let Some(window) = app.active_window() {
            window.present();
            return;
        }
        window::build(app);
    });
    app.run()
}
