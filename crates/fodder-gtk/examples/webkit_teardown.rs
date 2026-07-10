//! Empirical check of the WebKit resource-reclamation contract used by
//! ArticleView: create WebView + ephemeral NetworkSession, then
//! terminate_web_process + drop must return the process count to baseline.
//!
//! Run: cargo run -p fodder-gtk --example webkit_teardown
//! Exits 0 when reclamation works.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4 as gtk;

use gtk::glib;
use gtk::prelude::*;
use webkit6::prelude::*;

fn count_webkit_processes() -> usize {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return 0;
    };
    entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .chars()
                .all(|c| c.is_ascii_digit())
        })
        .filter_map(|e| std::fs::read(e.path().join("cmdline")).ok())
        .filter(|cmdline| {
            let s = String::from_utf8_lossy(cmdline);
            s.contains("WebKitWebProcess") || s.contains("WebKitNetworkProcess")
        })
        .count()
}

fn main() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("io.github.dipakmdhrm.FodderWebkitCheck")
        .build();
    app.connect_activate(|app| {
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .default_width(400)
            .default_height(300)
            .title("webkit teardown check")
            .build();

        let baseline = count_webkit_processes();
        let session = webkit6::NetworkSession::new_ephemeral();
        let view = webkit6::WebView::builder()
            .network_session(&session)
            .build();
        view.load_html("<html><body><p>teardown probe</p></body></html>", None);
        window.set_child(Some(&view));
        window.present();

        let state = Rc::new(RefCell::new(Some((view, session))));
        let window = window.clone();
        let app = app.clone();
        glib::timeout_add_seconds_local_once(3, move || {
            let after_create = count_webkit_processes();
            // The teardown sequence under test (mirrors ArticleView):
            if let Some((view, session)) = state.borrow_mut().take() {
                view.terminate_web_process();
                window.set_child(gtk::Widget::NONE);
                drop(view);
                drop(session);
            }
            glib::timeout_add_seconds_local_once(3, move || {
                let after_teardown = count_webkit_processes();
                println!(
                    "baseline={baseline} after_create={after_create} after_teardown={after_teardown}"
                );
                let ok = after_create > baseline && after_teardown <= baseline;
                println!("{}", if ok { "RECLAIMED" } else { "LEAKED" });
                app.quit();
                if !ok {
                    std::process::exit(1);
                }
            });
        });
    });
    app.run()
}
