use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4 as gtk;
use libadwaita as adw;

use adw::prelude::*;
use gtk::glib::{self, BoxedAnyObject};
use gtk::{gio, pango};

use fodder_core::models::ItemSummary;
use fodder_core::{now_unix, Db};

use crate::article_view::ArticleView;
use crate::daemon_client::DaemonClient;

pub struct Ui {
    pub db: RefCell<Db>,
    pub daemon: DaemonClient,
    pub window: adw::ApplicationWindow,
    pub toasts: adw::ToastOverlay,
    pub outer_split: adw::NavigationSplitView,
    pub inner_split: adw::NavigationSplitView,
    pub feed_list: gtk::ListBox,
    /// Feeds in the same order as `feed_list` rows.
    pub feeds: RefCell<Vec<fodder_core::models::Feed>>,
    pub item_store: gio::ListStore,
    pub item_selection: gtk::SingleSelection,
    pub items_page: adw::NavigationPage,
    pub article: ArticleView,
    pub selected_feed_id: Cell<Option<i64>>,
    pub showing_item_id: Cell<Option<i64>>,
    pub open_browser_button: gtk::Button,
    pub current_link: RefCell<Option<String>>,
}

pub fn build(app: &adw::Application) {
    let db = match Db::open_default() {
        Ok(db) => db,
        Err(e) => {
            eprintln!("cannot open database: {e}");
            std::process::exit(1);
        }
    };
    let daemon = DaemonClient::connect();
    daemon.ensure_daemon_running();

    // --- feeds pane ---
    let feed_list = gtk::ListBox::builder()
        .css_classes(["navigation-sidebar"])
        .build();
    let feed_scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&feed_list)
        .vexpand(true)
        .build();
    let add_button = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        .tooltip_text("Add feed")
        .build();
    let remove_button = gtk::Button::builder()
        .icon_name("list-remove-symbolic")
        .tooltip_text("Remove selected feed")
        .build();
    let menu = gio::Menu::new();
    menu.append(Some("Preferences"), Some("app.preferences"));
    menu.append(Some("About Fodder"), Some("app.about"));
    let menu_button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .menu_model(&menu)
        .build();
    let feeds_header = adw::HeaderBar::new();
    feeds_header.pack_start(&add_button);
    feeds_header.pack_start(&remove_button);
    feeds_header.pack_end(&menu_button);
    let feeds_view = adw::ToolbarView::new();
    feeds_view.add_top_bar(&feeds_header);
    feeds_view.set_content(Some(&feed_scroller));
    let feeds_page = adw::NavigationPage::builder()
        .title("Feeds")
        .child(&feeds_view)
        .build();

    // --- items pane ---
    let item_store = gio::ListStore::new::<BoxedAnyObject>();
    let item_selection = gtk::SingleSelection::builder()
        .model(&item_store)
        .autoselect(false)
        .can_unselect(true)
        .build();
    let item_view = gtk::ListView::builder()
        .model(&item_selection)
        .factory(&item_factory())
        .css_classes(["navigation-sidebar"])
        .build();
    let item_scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&item_view)
        .vexpand(true)
        .build();
    let refresh_button = gtk::Button::builder()
        .icon_name("view-refresh-symbolic")
        .tooltip_text("Refresh feeds")
        .build();
    let mark_read_button = gtk::Button::builder()
        .icon_name("object-select-symbolic")
        .tooltip_text("Mark all as read")
        .build();
    let items_header = adw::HeaderBar::new();
    items_header.pack_start(&refresh_button);
    items_header.pack_end(&mark_read_button);
    let items_view_tb = adw::ToolbarView::new();
    items_view_tb.add_top_bar(&items_header);
    items_view_tb.set_content(Some(&item_scroller));
    let items_page = adw::NavigationPage::builder()
        .title("Articles")
        .child(&items_view_tb)
        .build();

    // --- article pane ---
    let article = ArticleView::new();
    let open_browser_button = gtk::Button::builder()
        .icon_name("external-link-symbolic")
        .tooltip_text("Open in browser")
        .sensitive(false)
        .build();
    let article_header = adw::HeaderBar::new();
    article_header.pack_end(&open_browser_button);
    let article_view_tb = adw::ToolbarView::new();
    article_view_tb.add_top_bar(&article_header);
    article_view_tb.set_content(Some(&article.root));
    let article_page = adw::NavigationPage::builder()
        .title("Article")
        .child(&article_view_tb)
        .build();

    // --- nested split views: feeds | (items | article) ---
    let inner_split = adw::NavigationSplitView::builder()
        .sidebar(&items_page)
        .content(&article_page)
        .min_sidebar_width(280.0)
        .max_sidebar_width(360.0)
        .build();
    let inner_page = adw::NavigationPage::builder()
        .title("Fodder")
        .child(&inner_split)
        .build();
    let outer_split = adw::NavigationSplitView::builder()
        .sidebar(&feeds_page)
        .content(&inner_page)
        .min_sidebar_width(180.0)
        .max_sidebar_width(240.0)
        .build();

    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&outer_split));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Fodder")
        .default_width(1100)
        .default_height(720)
        .width_request(360)
        .height_request(400)
        .content(&toasts)
        .build();

    // Responsive collapse. Breakpoints don't stack: the narrow one must set
    // everything the medium one sets.
    let medium = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
        adw::BreakpointConditionLengthType::MaxWidth,
        860.0,
        adw::LengthUnit::Sp,
    ));
    medium.add_setter(&inner_split, "collapsed", Some(&true.to_value()));
    window.add_breakpoint(medium);
    let narrow = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
        adw::BreakpointConditionLengthType::MaxWidth,
        560.0,
        adw::LengthUnit::Sp,
    ));
    narrow.add_setter(&inner_split, "collapsed", Some(&true.to_value()));
    narrow.add_setter(&outer_split, "collapsed", Some(&true.to_value()));
    window.add_breakpoint(narrow);

    let ui = Rc::new(Ui {
        db: RefCell::new(db),
        daemon,
        window: window.clone(),
        toasts,
        outer_split,
        inner_split,
        feed_list: feed_list.clone(),
        feeds: RefCell::new(Vec::new()),
        item_store,
        item_selection: item_selection.clone(),
        items_page,
        article,
        selected_feed_id: Cell::new(None),
        showing_item_id: Cell::new(None),
        open_browser_button: open_browser_button.clone(),
        current_link: RefCell::new(None),
    });

    // --- wiring ---
    feed_list.connect_row_selected(glib::clone!(
        #[weak]
        ui,
        move |_, row| {
            let Some(row) = row else { return };
            let feed = ui.feeds.borrow().get(row.index() as usize).cloned();
            if let Some(feed) = feed {
                select_feed(&ui, feed.id);
                ui.outer_split.set_show_content(true);
            }
        }
    ));

    item_selection.connect_selected_notify(glib::clone!(
        #[weak]
        ui,
        move |selection| {
            let pos = selection.selected();
            if pos == gtk::INVALID_LIST_POSITION {
                return;
            }
            show_item_at(&ui, pos);
        }
    ));

    add_button.connect_clicked(glib::clone!(
        #[weak]
        ui,
        move |_| add_feed_dialog(&ui)
    ));
    remove_button.connect_clicked(glib::clone!(
        #[weak]
        ui,
        move |_| remove_feed_dialog(&ui)
    ));
    refresh_button.connect_clicked(glib::clone!(
        #[weak]
        ui,
        move |_| {
            ui.daemon.poll_now();
            ui.toasts.add_toast(adw::Toast::new("Refreshing feeds…"));
        }
    ));
    mark_read_button.connect_clicked(glib::clone!(
        #[weak]
        ui,
        move |_| {
            if let Some(feed_id) = ui.selected_feed_id.get() {
                if let Err(e) = ui.db.borrow().mark_feed_read(feed_id) {
                    log::warn!("mark read failed: {e}");
                }
                reload_items(&ui);
                refresh_feeds(&ui);
            }
        }
    ));
    open_browser_button.connect_clicked(glib::clone!(
        #[weak]
        ui,
        move |_| {
            let link = ui.current_link.borrow().clone();
            if let Some(link) = link {
                gtk::UriLauncher::new(&link).launch(
                    Some(&ui.window),
                    None::<&gio::Cancellable>,
                    |result| {
                        if let Err(e) = result {
                            log::warn!("could not open browser: {e}");
                        }
                    },
                );
            }
        }
    ));

    // Daemon → viewer: new items arrived; refresh what is on screen.
    ui.daemon.on_items_added(glib::clone!(
        #[weak]
        ui,
        move |_new, _total| {
            refresh_feeds(&ui);
            reload_items(&ui);
        }
    ));

    // Close: take the daemon down with us unless running in background.
    window.connect_close_request(glib::clone!(
        #[weak]
        ui,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_| {
            let run_in_background = ui
                .db
                .borrow()
                .settings()
                .map(|s| s.run_in_background)
                .unwrap_or(true);
            if !run_in_background {
                ui.daemon.quit_daemon();
            }
            glib::Propagation::Proceed
        }
    ));

    refresh_feeds(&ui);
    ui.article.clear();
    window.present();
}

fn item_factory() -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, list_item| {
        let list_item = list_item.downcast_ref::<gtk::ListItem>().unwrap();
        let title = gtk::Label::builder()
            .halign(gtk::Align::Start)
            .wrap(true)
            .wrap_mode(pango::WrapMode::WordChar)
            .lines(2)
            .ellipsize(pango::EllipsizeMode::End)
            .css_classes(["heading"])
            .build();
        let dot = gtk::Label::builder()
            .label("●")
            .css_classes(["accent"])
            .build();
        let meta = gtk::Label::builder()
            .halign(gtk::Align::Start)
            .hexpand(true)
            .ellipsize(pango::EllipsizeMode::End)
            .css_classes(["dim-label", "caption"])
            .build();
        let meta_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .build();
        meta_row.append(&meta);
        meta_row.append(&dot);
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(3)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(4)
            .margin_end(4)
            .build();
        root.append(&title);
        root.append(&meta_row);
        list_item.set_child(Some(&root));
    });
    factory.connect_bind(|_, list_item| {
        let list_item = list_item.downcast_ref::<gtk::ListItem>().unwrap();
        let object = list_item.item().and_downcast::<BoxedAnyObject>().unwrap();
        let item = object.borrow::<ItemSummary>();
        let root = list_item.child().and_downcast::<gtk::Box>().unwrap();
        let title = root.first_child().and_downcast::<gtk::Label>().unwrap();
        let meta_row = title.next_sibling().and_downcast::<gtk::Box>().unwrap();
        let meta = meta_row.first_child().and_downcast::<gtk::Label>().unwrap();
        let dot = meta.next_sibling().and_downcast::<gtk::Label>().unwrap();

        title.set_text(if item.title.is_empty() {
            "(untitled)"
        } else {
            &item.title
        });
        meta.set_text(&item_meta(&item));
        dot.set_visible(!item.is_read);
    });
    factory
}

fn item_meta(item: &ItemSummary) -> String {
    let date = chrono::DateTime::from_timestamp(item.published_at.unwrap_or(item.fetched_at), 0)
        .map(|dt| dt.format("%b %e, %Y").to_string())
        .unwrap_or_default();
    match &item.author {
        Some(author) => format!("{author} · {date}"),
        None => date,
    }
}

pub fn refresh_feeds(ui: &Rc<Ui>) {
    let feeds = match ui.db.borrow().list_feeds() {
        Ok(feeds) => feeds,
        Err(e) => {
            log::warn!("listing feeds failed: {e}");
            return;
        }
    };
    let selected = ui.selected_feed_id.get();
    ui.feed_list.remove_all();
    for feed in &feeds {
        let title = gtk::Label::builder()
            .label(if feed.title.is_empty() {
                feed.url.as_str()
            } else {
                feed.title.as_str()
            })
            .halign(gtk::Align::Start)
            .hexpand(true)
            .ellipsize(pango::EllipsizeMode::End)
            .build();
        let row_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .build();
        row_box.append(&title);
        if feed.unread_count > 0 {
            let badge = gtk::Label::builder()
                .label(feed.unread_count.to_string())
                .css_classes(["numeric", "dim-label", "caption"])
                .build();
            row_box.append(&badge);
        }
        ui.feed_list.append(&row_box);
    }
    *ui.feeds.borrow_mut() = feeds;
    // Restore the previous selection without re-triggering a reload.
    if let Some(feed_id) = selected {
        let index = ui.feeds.borrow().iter().position(|f| f.id == feed_id);
        if let Some(index) = index {
            let row = ui.feed_list.row_at_index(index as i32);
            ui.feed_list.select_row(row.as_ref());
        }
    }
}

fn select_feed(ui: &Rc<Ui>, feed_id: i64) {
    if ui.selected_feed_id.get() == Some(feed_id) {
        return;
    }
    ui.selected_feed_id.set(Some(feed_id));
    ui.showing_item_id.set(None);
    let title = ui
        .feeds
        .borrow()
        .iter()
        .find(|f| f.id == feed_id)
        .map(|f| {
            if f.title.is_empty() {
                f.url.clone()
            } else {
                f.title.clone()
            }
        })
        .unwrap_or_default();
    ui.items_page.set_title(&title);
    reload_items(ui);
    ui.article.clear();
    ui.open_browser_button.set_sensitive(false);
}

pub fn reload_items(ui: &Rc<Ui>) {
    let Some(feed_id) = ui.selected_feed_id.get() else {
        ui.item_store.remove_all();
        return;
    };
    let items = match ui.db.borrow().list_items(feed_id) {
        Ok(items) => items,
        Err(e) => {
            log::warn!("listing items failed: {e}");
            return;
        }
    };
    let showing = ui.showing_item_id.get();
    let reselect = items.iter().position(|i| Some(i.id) == showing);
    ui.item_store.remove_all();
    for item in items {
        ui.item_store.append(&BoxedAnyObject::new(item));
    }
    match reselect {
        Some(pos) => ui.item_selection.set_selected(pos as u32),
        None => ui.item_selection.set_selected(gtk::INVALID_LIST_POSITION),
    }
}

fn show_item_at(ui: &Rc<Ui>, position: u32) {
    let Some(object) = ui
        .item_store
        .item(position)
        .and_downcast::<BoxedAnyObject>()
    else {
        return;
    };
    let item_id = object.borrow::<ItemSummary>().id;
    if ui.showing_item_id.get() == Some(item_id) {
        return;
    }
    let item = match ui.db.borrow().get_item(item_id) {
        Ok(Some(item)) => item,
        Ok(None) => return,
        Err(e) => {
            log::warn!("loading item failed: {e}");
            return;
        }
    };
    ui.showing_item_id.set(Some(item_id));

    let feed_title = ui
        .feeds
        .borrow()
        .iter()
        .find(|f| f.id == item.feed_id)
        .map(|f| f.title.clone())
        .unwrap_or_default();
    ui.article.show(&item, &feed_title);
    *ui.current_link.borrow_mut() = item.link.clone();
    ui.open_browser_button.set_sensitive(item.link.is_some());
    ui.inner_split.set_show_content(true);

    if !item.is_read {
        if let Err(e) = ui.db.borrow().set_item_read(item_id, true) {
            log::warn!("marking item read failed: {e}");
        }
        // Update the row in place so the unread dot clears.
        {
            let mut summary = object.borrow_mut::<ItemSummary>();
            summary.is_read = true;
        }
        ui.item_store.items_changed(position, 1, 1);
        ui.item_selection.set_selected(position);
        refresh_feeds(ui);
    }
}

fn add_feed_dialog(ui: &Rc<Ui>) {
    let entry = gtk::Entry::builder()
        .placeholder_text("https://example.org/feed.xml")
        .activates_default(true)
        .build();
    let dialog = adw::AlertDialog::new(Some("Add Feed"), Some("Enter the feed or Atom URL"));
    dialog.set_extra_child(Some(&entry));
    dialog.add_responses(&[("cancel", "Cancel"), ("add", "Add")]);
    dialog.set_response_appearance("add", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("add"));
    dialog.set_close_response("cancel");
    dialog.connect_response(
        Some("add"),
        glib::clone!(
            #[weak]
            ui,
            move |_, _| {
                let url = entry.text().trim().to_string();
                if !url.is_empty() {
                    subscribe(&ui, url);
                }
            }
        ),
    );
    dialog.present(Some(&ui.window));
}

/// Validate by fetching+parsing off the main thread, then store everything
/// from that same response so the feed appears populated immediately.
fn subscribe(ui: &Rc<Ui>, url: String) {
    let (tx, rx) = async_channel::bounded(1);
    let fetch_url = url.clone();
    std::thread::spawn(move || {
        let agent = fodder_core::fetch::agent();
        let result =
            fodder_core::fetch::fetch_feed(&agent, &fetch_url, None, None).and_then(|outcome| {
                match outcome {
                    fodder_core::fetch::FetchOutcome::Fetched(fetched) => {
                        let parsed = fodder_core::parse::parse_feed(&fetched.body)?;
                        Ok((parsed, fetched.etag, fetched.last_modified))
                    }
                    fodder_core::fetch::FetchOutcome::NotModified => {
                        Err(fodder_core::Error::Http("empty 304 response".into()))
                    }
                }
            });
        let _ = tx.send_blocking(result);
    });
    glib::spawn_future_local(glib::clone!(
        #[weak]
        ui,
        async move {
            let Ok(result) = rx.recv().await else { return };
            match result {
                Ok((parsed, etag, last_modified)) => {
                    let outcome = {
                        let mut db = ui.db.borrow_mut();
                        let now = now_unix();
                        db.add_feed(&url, now).and_then(|feed_id| {
                            db.update_feed_metadata(
                                feed_id,
                                &parsed.title,
                                parsed.site_url.as_deref(),
                                parsed.description.as_deref(),
                            )?;
                            db.insert_items(feed_id, &parsed.items, now)?;
                            db.record_poll(
                                feed_id,
                                etag.as_deref(),
                                last_modified.as_deref(),
                                "ok",
                                now,
                            )?;
                            Ok(feed_id)
                        })
                    };
                    match outcome {
                        Ok(_) => {
                            refresh_feeds(&ui);
                            ui.toasts.add_toast(adw::Toast::new(&format!(
                                "Subscribed to {}",
                                parsed.title
                            )));
                        }
                        Err(e) => {
                            ui.toasts
                                .add_toast(adw::Toast::new(&format!("Could not add feed: {e}")));
                        }
                    }
                }
                Err(e) => {
                    ui.toasts
                        .add_toast(adw::Toast::new(&format!("Not a valid feed: {e}")));
                }
            }
        }
    ));
}

fn remove_feed_dialog(ui: &Rc<Ui>) {
    let Some(feed_id) = ui.selected_feed_id.get() else {
        ui.toasts
            .add_toast(adw::Toast::new("Select a feed to remove"));
        return;
    };
    let title = ui
        .feeds
        .borrow()
        .iter()
        .find(|f| f.id == feed_id)
        .map(|f| f.title.clone())
        .unwrap_or_default();
    let dialog = adw::AlertDialog::new(
        Some("Remove Feed?"),
        Some(&format!(
            "Unsubscribe from “{title}” and delete its articles."
        )),
    );
    dialog.add_responses(&[("cancel", "Cancel"), ("remove", "Remove")]);
    dialog.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
    dialog.set_close_response("cancel");
    dialog.connect_response(
        Some("remove"),
        glib::clone!(
            #[weak]
            ui,
            move |_, _| {
                if let Err(e) = ui.db.borrow().remove_feed(feed_id) {
                    log::warn!("removing feed failed: {e}");
                    return;
                }
                ui.selected_feed_id.set(None);
                ui.showing_item_id.set(None);
                ui.item_store.remove_all();
                ui.article.clear();
                ui.open_browser_button.set_sensitive(false);
                refresh_feeds(&ui);
            }
        ),
    );
    dialog.present(Some(&ui.window));
}
