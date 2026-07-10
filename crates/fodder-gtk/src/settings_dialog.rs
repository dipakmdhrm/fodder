use std::rc::Rc;

use gtk4 as gtk;
use libadwaita as adw;

use adw::prelude::*;
use gtk::glib;

use fodder_core::schedule::INTERVAL_CHOICES_MINUTES;
use fodder_core::{paths, Settings};

use crate::window::Ui;

const INTERVAL_LABELS: [&str; 6] = [
    "Every 15 minutes",
    "Every 30 minutes",
    "Every hour",
    "Every 4 hours",
    "Every 12 hours",
    "Daily",
];

pub fn show(ui: &Rc<Ui>) {
    let settings = ui.db.borrow().settings().unwrap_or_default();

    let interval = adw::ComboRow::builder()
        .title("Update interval")
        .subtitle("How often feeds are checked")
        .model(&gtk::StringList::new(&INTERVAL_LABELS))
        .build();
    let selected = INTERVAL_CHOICES_MINUTES
        .iter()
        .position(|&m| m == settings.poll_interval_minutes)
        .unwrap_or(2);
    interval.set_selected(selected as u32);

    let background = adw::SwitchRow::builder()
        .title("Run in background")
        .subtitle("Keep checking feeds after the window closes (adds a tray icon)")
        .active(settings.run_in_background)
        .build();
    let autostart = adw::SwitchRow::builder()
        .title("Autostart on login")
        .subtitle("Start the background poller when you log in")
        .active(settings.autostart)
        .build();
    let notifications = adw::SwitchRow::builder()
        .title("Notifications")
        .subtitle("Announce new articles")
        .active(settings.notifications)
        .build();

    let group = adw::PreferencesGroup::new();
    group.add(&interval);
    group.add(&background);
    group.add(&autostart);
    group.add(&notifications);
    let page = adw::PreferencesPage::new();
    page.add(&group);
    let dialog = adw::PreferencesDialog::builder()
        .title("Preferences")
        .build();
    dialog.add(&page);

    let persist = glib::clone!(
        #[weak]
        ui,
        #[weak]
        interval,
        #[weak]
        background,
        #[weak]
        autostart,
        #[weak]
        notifications,
        move || {
            let new_settings = Settings {
                poll_interval_minutes: INTERVAL_CHOICES_MINUTES
                    [(interval.selected() as usize).min(INTERVAL_CHOICES_MINUTES.len() - 1)],
                run_in_background: background.is_active(),
                autostart: autostart.is_active(),
                notifications: notifications.is_active(),
            };
            if let Err(e) = ui.db.borrow().save_settings(&new_settings) {
                log::warn!("saving settings failed: {e}");
                ui.toasts
                    .add_toast(adw::Toast::new("Could not save settings"));
                return;
            }
            if let Err(e) = paths::set_autostart(new_settings.autostart) {
                log::warn!("autostart update failed: {e}");
                ui.toasts
                    .add_toast(adw::Toast::new("Could not update autostart entry"));
            }
            // Daemon re-reads settings and shows/hides the tray live.
            ui.daemon.ensure_daemon_running();
            ui.daemon.settings_changed();
        }
    );

    let p = persist.clone();
    interval.connect_selected_notify(move |_| p());
    let p = persist.clone();
    background.connect_active_notify(move |_| p());
    let p = persist.clone();
    autostart.connect_active_notify(move |_| p());
    notifications.connect_active_notify(move |_| persist());

    dialog.present(Some(&ui.window));
}
