//! The viewer window: a 3-pane libadwaita UI (feeds | articles | reader) wired
//! to the shared database and the daemon over IPC.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use fodder_core::db::{articles, feeds, Db};
use fodder_core::ipc::IpcMessage;
use fodder_core::models::Article;
use fodder_core::paths;
use gtk4 as gtk;
use gtk::prelude::*;
use gtk::{gdk, gio, glib, pango};
use libadwaita as adw;
use adw::prelude::*;
use tokio::sync::mpsc::{self, UnboundedSender};

use crate::ipc_client::{self, FromDaemon};
use crate::reader;
use crate::runtime::{self, DbHandle};

/// An initial navigation target (from CLI args or an `OpenAt` message).
#[derive(Clone, Copy, Default)]
pub struct Target {
    pub feed: Option<i64>,
    pub article: Option<i64>,
}

/// All viewer state and widgets. Held in an `Rc`; signal handlers and the async
/// event loop hold clones, which keeps it alive for the window's lifetime.
pub struct App {
    window: adw::ApplicationWindow,
    inner_split: adw::NavigationSplitView,

    feeds_list: gtk::ListBox,
    articles_list: gtk::ListBox,
    articles_stack: gtk::Stack,
    articles_title: adw::WindowTitle,

    reader_stack: gtk::Stack,
    reader_title: gtk::Label,
    reader_meta: gtk::Label,
    reader_body: gtk::Label,
    reader_error: adw::StatusPage,

    // Row → id maps, parallel to the list rows.
    feed_ids: RefCell<Vec<Option<i64>>>,
    article_ids: RefCell<Vec<i64>>,
    article_titles: RefCell<Vec<gtk::Label>>,

    selected_feed: Cell<Option<i64>>,
    current_article: Cell<Option<i64>>,
    current_url: RefCell<Option<String>>,
    suppress: Cell<bool>,

    db: DbHandle,
    rt: tokio::runtime::Runtime,
    cmd_tx: UnboundedSender<IpcMessage>,
}

/// Build the window, wire everything, and present it.
pub fn build(app: &adw::Application, target: Target) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("building tokio runtime");

    // Open (and migrate, in case we're run before the daemon) the shared DB.
    let db = match open_db() {
        Ok(db) => Arc::new(Mutex::new(db)),
        Err(e) => {
            present_fatal(app, &format!("Cannot open the database:\n{e}"));
            return;
        }
    };

    let (ui_tx, mut ui_rx) = mpsc::unbounded_channel::<FromDaemon>();
    let cmd_tx = ipc_client::start(rt.handle(), ui_tx);

    let this = assemble(app, db, rt, cmd_tx);

    // Pump daemon events on the GTK main thread.
    let events_app = this.clone();
    glib::spawn_future_local(async move {
        while let Some(event) = ui_rx.recv().await {
            events_app.handle_daemon(event);
        }
    });

    this.window.present();
    this.load_feeds(Some(target));
}

fn open_db() -> anyhow::Result<Db> {
    let mut db = Db::open(&paths::db_path()?)?;
    db.migrate()?;
    Ok(db)
}

/// Last-resort window when the DB can't be opened.
fn present_fatal(app: &adw::Application, message: &str) {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .default_width(560)
        .default_height(360)
        .build();
    let page = adw::StatusPage::builder()
        .icon_name("dialog-error-symbolic")
        .title("Fodder Reader")
        .description(message)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&page));
    window.set_content(Some(&toolbar));
    window.present();
}

/// Construct all widgets and the `App`.
fn assemble(
    app: &adw::Application,
    db: DbHandle,
    rt: tokio::runtime::Runtime,
    cmd_tx: UnboundedSender<IpcMessage>,
) -> Rc<App> {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .default_width(1100)
        .default_height(720)
        .title("Fodder Reader")
        .build();

    // --- Feeds pane (left) ---
    let feeds_list = gtk::ListBox::new();
    feeds_list.set_selection_mode(gtk::SelectionMode::Single);
    feeds_list.add_css_class("navigation-sidebar");
    let feeds_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&feeds_list)
        .build();

    let add_btn = icon_button("list-add-symbolic", "Add feed");
    let remove_btn = icon_button("list-remove-symbolic", "Remove selected feed");
    let refresh_btn = icon_button("view-refresh-symbolic", "Refresh all feeds");
    let feeds_header = adw::HeaderBar::new();
    feeds_header.set_title_widget(Some(&adw::WindowTitle::new("Feeds", "")));
    feeds_header.pack_start(&add_btn);
    feeds_header.pack_start(&remove_btn);
    feeds_header.pack_end(&refresh_btn);
    let feeds_pane = adw::ToolbarView::new();
    feeds_pane.add_top_bar(&feeds_header);
    feeds_pane.set_content(Some(&feeds_scroll));

    // --- Article list pane (middle) ---
    let articles_list = gtk::ListBox::new();
    articles_list.set_selection_mode(gtk::SelectionMode::Single);
    articles_list.add_css_class("navigation-sidebar");
    let articles_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&articles_list)
        .build();
    let articles_stack = gtk::Stack::new();
    articles_stack.add_named(&articles_scroll, Some("list"));
    articles_stack.add_named(&status_page("view-list-symbolic", "No articles", "This feed has no articles yet."), Some("empty"));
    articles_stack.add_named(&centered_spinner(), Some("loading"));
    articles_stack.set_visible_child_name("empty");

    let mark_all_btn = icon_button("checkbox-checked-symbolic", "Mark all as read");
    let articles_title = adw::WindowTitle::new("All Articles", "");
    let articles_header = adw::HeaderBar::new();
    articles_header.set_title_widget(Some(&articles_title));
    articles_header.pack_end(&mark_all_btn);
    let articles_pane = adw::ToolbarView::new();
    articles_pane.add_top_bar(&articles_header);
    articles_pane.set_content(Some(&articles_stack));

    // --- Reader pane (right) ---
    let reader_title = gtk::Label::new(None);
    reader_title.set_wrap(true);
    reader_title.set_xalign(0.0);
    reader_title.add_css_class("title-2");
    let reader_meta = gtk::Label::new(None);
    reader_meta.set_xalign(0.0);
    reader_meta.add_css_class("dim-label");
    let reader_body = reader::body_label();

    let reader_box = gtk::Box::new(gtk::Orientation::Vertical, 12);
    reader_box.set_margin_top(18);
    reader_box.set_margin_bottom(18);
    reader_box.set_margin_start(18);
    reader_box.set_margin_end(18);
    reader_box.append(&reader_title);
    reader_box.append(&reader_meta);
    reader_box.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    reader_box.append(&reader_body);
    let reader_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&reader_box)
        .build();

    let reader_error = adw::StatusPage::builder()
        .icon_name("dialog-error-symbolic")
        .title("Something went wrong")
        .build();
    let reader_stack = gtk::Stack::new();
    reader_stack.add_named(&reader_scroll, Some("content"));
    reader_stack.add_named(&status_page("emblem-documents-symbolic", "Select an article", "Choose an article from the list to read it here."), Some("empty"));
    reader_stack.add_named(&reader_error, Some("error"));
    reader_stack.set_visible_child_name("empty");

    let open_btn = icon_button(browser_icon_name(), "Open in browser");
    let reader_header = adw::HeaderBar::new();
    reader_header.set_title_widget(Some(&adw::WindowTitle::new("Reader", "")));
    reader_header.pack_end(&open_btn);
    let reader_pane = adw::ToolbarView::new();
    reader_pane.add_top_bar(&reader_header);
    reader_pane.set_content(Some(&reader_stack));

    // --- Split views ---
    let inner_split = adw::NavigationSplitView::new();
    inner_split.set_sidebar(Some(&adw::NavigationPage::new(&articles_pane, "Articles")));
    inner_split.set_content(Some(&adw::NavigationPage::new(&reader_pane, "Reader")));
    inner_split.set_min_sidebar_width(280.0);

    let outer_split = adw::OverlaySplitView::new();
    outer_split.set_sidebar(Some(&feeds_pane));
    outer_split.set_content(Some(&inner_split));
    outer_split.set_max_sidebar_width(320.0);
    outer_split.set_min_sidebar_width(220.0);

    window.set_content(Some(&outer_split));

    let this = Rc::new(App {
        window,
        inner_split,
        feeds_list,
        articles_list,
        articles_stack,
        articles_title,
        reader_stack,
        reader_title,
        reader_meta,
        reader_body,
        reader_error,
        feed_ids: RefCell::new(Vec::new()),
        article_ids: RefCell::new(Vec::new()),
        article_titles: RefCell::new(Vec::new()),
        selected_feed: Cell::new(None),
        current_article: Cell::new(None),
        current_url: RefCell::new(None),
        suppress: Cell::new(false),
        db,
        rt,
        cmd_tx,
    });

    wire_signals(&this, &add_btn, &remove_btn, &refresh_btn, &mark_all_btn, &open_btn);
    this
}

fn wire_signals(
    this: &Rc<App>,
    add_btn: &gtk::Button,
    remove_btn: &gtk::Button,
    refresh_btn: &gtk::Button,
    mark_all_btn: &gtk::Button,
    open_btn: &gtk::Button,
) {
    let a = this.clone();
    this.feeds_list.connect_row_selected(move |_, row| {
        if a.suppress.get() {
            return;
        }
        if let Some(row) = row {
            let idx = row.index() as usize;
            if let Some(feed_id) = a.feed_ids.borrow().get(idx).copied() {
                a.select_feed(feed_id, None);
            }
        }
    });

    let a = this.clone();
    this.articles_list.connect_row_selected(move |_, row| {
        if a.suppress.get() {
            return;
        }
        if let Some(row) = row {
            let idx = row.index() as usize;
            if let Some(id) = a.article_ids.borrow().get(idx).copied() {
                a.open_article(id);
            }
        }
    });

    let a = this.clone();
    refresh_btn.connect_clicked(move |_| {
        let _ = a.cmd_tx.send(IpcMessage::RefreshNow { feed_id: None });
    });

    let a = this.clone();
    mark_all_btn.connect_clicked(move |_| a.mark_all_read());

    let a = this.clone();
    open_btn.connect_clicked(move |_| a.open_in_browser());

    let a = this.clone();
    add_btn.connect_clicked(move |_| a.add_feed_dialog());

    let a = this.clone();
    remove_btn.connect_clicked(move |_| a.remove_selected_feed());
}

impl App {
    /// Load feeds + unread counts, rebuild the sidebar, then navigate.
    fn load_feeds(self: &Rc<Self>, target: Option<Target>) {
        let this = self.clone();
        runtime::run_db(
            self.rt.handle(),
            self.db.clone(),
            |c| {
                let feeds = feeds::list_feeds(c)?;
                let unread = articles::unread_counts(c)?;
                let total = articles::total_unread(c)?;
                Ok((feeds, unread, total))
            },
            move |res| match res {
                Ok((feeds, unread, total)) => this.on_feeds_loaded(feeds, unread, total, target),
                Err(e) => this.show_reader_error(&format!("Failed to load feeds: {e}")),
            },
        );
    }

    fn on_feeds_loaded(
        self: &Rc<Self>,
        feeds: Vec<fodder_core::models::Feed>,
        unread: std::collections::HashMap<i64, i64>,
        total: i64,
        target: Option<Target>,
    ) {
        self.suppress.set(true);
        clear_list(&self.feeds_list);
        let mut ids: Vec<Option<i64>> = Vec::with_capacity(feeds.len() + 1);

        // "All Articles" aggregate row.
        self.feeds_list.append(&feed_row("All Articles", total, false));
        ids.push(None);

        for feed in &feeds {
            let count = unread.get(&feed.id).copied().unwrap_or(0);
            let has_error = feed.last_error.is_some();
            self.feeds_list.append(&feed_row(&feed.title, count, has_error));
            ids.push(Some(feed.id));
        }
        *self.feed_ids.borrow_mut() = ids;
        self.suppress.set(false);

        // Decide which feed to show.
        let want_feed = target.and_then(|t| t.feed).or_else(|| self.selected_feed.get());
        let idx = self
            .feed_ids
            .borrow()
            .iter()
            .position(|f| *f == want_feed)
            .unwrap_or(0);
        let feed_id = self.feed_ids.borrow().get(idx).copied().flatten();
        self.select_feed_row(idx);
        self.select_feed(feed_id, target.and_then(|t| t.article));
    }

    /// Show articles for a feed (or all feeds when `None`).
    fn select_feed(self: &Rc<Self>, feed_id: Option<i64>, then_article: Option<i64>) {
        self.selected_feed.set(feed_id);
        self.articles_stack.set_visible_child_name("loading");

        let this = self.clone();
        runtime::run_db(
            self.rt.handle(),
            self.db.clone(),
            move |c| {
                let title = match feed_id {
                    Some(id) => feeds::get_feed(c, id)?.map(|f| f.title),
                    None => None,
                };
                let arts = articles::articles_for(c, feed_id, 300)?;
                Ok((title, arts))
            },
            move |res| match res {
                Ok((title, arts)) => this.on_articles_loaded(title, arts, then_article),
                Err(e) => this.show_reader_error(&format!("Failed to load articles: {e}")),
            },
        );
    }

    fn on_articles_loaded(
        self: &Rc<Self>,
        feed_title: Option<String>,
        arts: Vec<Article>,
        then_article: Option<i64>,
    ) {
        self.articles_title
            .set_title(feed_title.as_deref().unwrap_or("All Articles"));

        self.suppress.set(true);
        clear_list(&self.articles_list);
        let mut ids = Vec::with_capacity(arts.len());
        let mut labels = Vec::with_capacity(arts.len());
        for art in &arts {
            let (row, title_label) = article_row(art);
            self.articles_list.append(&row);
            ids.push(art.id);
            labels.push(title_label);
        }
        *self.article_ids.borrow_mut() = ids;
        *self.article_titles.borrow_mut() = labels;
        self.suppress.set(false);

        if arts.is_empty() {
            self.articles_stack.set_visible_child_name("empty");
            self.clear_reader();
            return;
        }
        self.articles_stack.set_visible_child_name("list");

        // Restore/select an article if requested and present.
        if let Some(want) = then_article {
            if let Some(pos) = self.article_ids.borrow().iter().position(|id| *id == want) {
                self.select_article_row(pos);
                self.open_article(want);
                return;
            }
        }
        self.clear_reader();
    }

    /// Open one article in the reader; mark it read on open.
    fn open_article(self: &Rc<Self>, id: i64) {
        self.current_article.set(Some(id));
        let this = self.clone();
        runtime::run_db(
            self.rt.handle(),
            self.db.clone(),
            move |c| articles::get_article(c, id),
            move |res| match res {
                Ok(Some(article)) => this.show_article(article),
                Ok(None) => this.clear_reader(),
                Err(e) => this.show_reader_error(&format!("Failed to load article: {e}")),
            },
        );
    }

    fn show_article(self: &Rc<Self>, article: Article) {
        self.reader_title.set_text(&article.title);
        self.reader_meta.set_text(&format_meta(&article));
        *self.current_url.borrow_mut() = article.url.clone();

        let markup = reader::html_to_pango(article.content.as_deref().unwrap_or(""));
        if markup.is_empty() {
            self.reader_body
                .set_text("(No content — use “Open in browser”.)");
        } else {
            self.reader_body.set_markup(&markup);
        }
        self.reader_stack.set_visible_child_name("content");
        self.inner_split.set_show_content(true);

        if !article.is_read {
            self.mark_read(article.id);
        }
    }

    /// Mark one article read: update the DB, un-bold its row, refresh badges.
    fn mark_read(self: &Rc<Self>, id: i64) {
        // Un-bold the row in place if it's the selected one.
        if let Some(pos) = self.article_ids.borrow().iter().position(|a| *a == id) {
            if let Some(label) = self.article_titles.borrow().get(pos) {
                label.set_text(&label.text());
            }
        }
        let this = self.clone();
        runtime::run_db(
            self.rt.handle(),
            self.db.clone(),
            move |c| articles::mark_read(c, id),
            move |res| {
                if let Err(e) = res {
                    tracing::warn!("mark_read failed: {e}");
                }
                // Refresh unread badges, preserving the current view.
                this.refresh_badges();
            },
        );
    }

    fn mark_all_read(self: &Rc<Self>) {
        let feed_id = self.selected_feed.get();
        let this = self.clone();
        runtime::run_db(
            self.rt.handle(),
            self.db.clone(),
            move |c| articles::mark_all_read(c, feed_id),
            move |res| match res {
                Ok(()) => this.load_feeds(Some(Target {
                    feed: this.selected_feed.get(),
                    article: this.current_article.get(),
                })),
                Err(e) => tracing::warn!("mark_all_read failed: {e}"),
            },
        );
    }

    /// Reload only feed unread badges without disturbing the article view.
    fn refresh_badges(self: &Rc<Self>) {
        let this = self.clone();
        runtime::run_db(
            self.rt.handle(),
            self.db.clone(),
            |c| {
                let feeds = feeds::list_feeds(c)?;
                let unread = articles::unread_counts(c)?;
                let total = articles::total_unread(c)?;
                Ok((feeds, unread, total))
            },
            move |res| {
                if let Ok((feeds, unread, total)) = res {
                    this.rebuild_feed_badges(feeds, unread, total);
                }
            },
        );
    }

    fn rebuild_feed_badges(
        self: &Rc<Self>,
        feeds: Vec<fodder_core::models::Feed>,
        unread: std::collections::HashMap<i64, i64>,
        total: i64,
    ) {
        // The feed set is unchanged here (badges only), so rebuild rows in place.
        let selected = self.selected_feed.get();
        self.suppress.set(true);
        clear_list(&self.feeds_list);
        let mut ids: Vec<Option<i64>> = Vec::with_capacity(feeds.len() + 1);
        self.feeds_list.append(&feed_row("All Articles", total, false));
        ids.push(None);
        for feed in &feeds {
            let count = unread.get(&feed.id).copied().unwrap_or(0);
            self.feeds_list
                .append(&feed_row(&feed.title, count, feed.last_error.is_some()));
            ids.push(Some(feed.id));
        }
        *self.feed_ids.borrow_mut() = ids;
        let idx = self
            .feed_ids
            .borrow()
            .iter()
            .position(|f| *f == selected)
            .unwrap_or(0);
        self.select_feed_row(idx);
        self.suppress.set(false);
    }

    fn open_in_browser(self: &Rc<Self>) {
        if let Some(url) = self.current_url.borrow().clone() {
            let launcher = gtk::UriLauncher::new(&url);
            launcher.launch(
                Some(&self.window),
                gio::Cancellable::NONE,
                |res| {
                    if let Err(e) = res {
                        tracing::warn!("open in browser failed: {e}");
                    }
                },
            );
        }
    }

    fn add_feed_dialog(self: &Rc<Self>) {
        let entry = gtk::Entry::builder()
            .placeholder_text("https://example.com/feed.xml")
            .activates_default(true)
            .build();
        let dialog = adw::AlertDialog::new(Some("Add feed"), Some("Enter a feed URL."));
        dialog.set_extra_child(Some(&entry));
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("add", "Add");
        dialog.set_response_appearance("add", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("add"));
        dialog.set_close_response("cancel");

        let this = self.clone();
        dialog.connect_response(None, move |_, response| {
            if response == "add" {
                let url = entry.text().trim().to_string();
                if !url.is_empty() {
                    // Discovery/validation lands in M5; for now subscribe the
                    // raw URL through the daemon (which polls it).
                    let _ = this.cmd_tx.send(IpcMessage::SubscribeResolved {
                        feed_url: url.clone(),
                        title: url,
                    });
                }
            }
        });
        dialog.present(Some(&self.window));
    }

    fn remove_selected_feed(self: &Rc<Self>) {
        let Some(feed_id) = self.selected_feed.get() else {
            return; // "All Articles" selected — nothing to remove.
        };
        let dialog = adw::AlertDialog::new(
            Some("Remove feed?"),
            Some("This unsubscribes the feed and deletes its stored articles."),
        );
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("remove", "Remove");
        dialog.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");

        let this = self.clone();
        dialog.connect_response(None, move |_, response| {
            if response == "remove" {
                let inner = this.clone();
                runtime::run_db(
                    this.rt.handle(),
                    this.db.clone(),
                    move |c| feeds::delete_feed(c, feed_id),
                    move |res| match res {
                        Ok(()) => {
                            inner.selected_feed.set(None);
                            inner.current_article.set(None);
                            inner.load_feeds(None);
                        }
                        Err(e) => tracing::warn!("remove feed failed: {e}"),
                    },
                );
            }
        });
        dialog.present(Some(&self.window));
    }

    fn handle_daemon(self: &Rc<Self>, event: FromDaemon) {
        match event {
            FromDaemon::Open => self.window.present(),
            FromDaemon::OpenAt { feed_id, article_id } => {
                self.window.present();
                self.load_feeds(Some(Target {
                    feed: Some(feed_id),
                    article: article_id,
                }));
            }
            FromDaemon::FeedsChanged => self.load_feeds(Some(Target {
                feed: self.selected_feed.get(),
                article: self.current_article.get(),
            })),
            FromDaemon::Duplicate => {
                tracing::info!("another viewer is already open; exiting");
                if let Some(app) = self.window.application() {
                    app.quit();
                }
            }
            FromDaemon::Disconnected => {
                tracing::warn!("daemon connection unavailable; live updates paused");
            }
        }
    }

    // --- small helpers ---

    fn clear_reader(&self) {
        self.current_article.set(None);
        *self.current_url.borrow_mut() = None;
        self.reader_stack.set_visible_child_name("empty");
    }

    fn show_reader_error(&self, message: &str) {
        self.reader_error.set_description(Some(message));
        self.reader_stack.set_visible_child_name("error");
    }

    fn select_feed_row(&self, idx: usize) {
        if let Some(row) = self.feeds_list.row_at_index(idx as i32) {
            self.feeds_list.select_row(Some(&row));
        }
    }

    fn select_article_row(&self, idx: usize) {
        if let Some(row) = self.articles_list.row_at_index(idx as i32) {
            self.articles_list.select_row(Some(&row));
        }
    }
}

// --- Free widget builders ---

fn icon_button(icon: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::from_icon_name(icon);
    button.set_tooltip_text(Some(tooltip));
    button
}

/// Icon for the "open in browser" button: prefer the Firefox logo, but fall
/// back to the generic web-browser icon if the theme doesn't provide `firefox`
/// (so it never renders as a broken/missing icon).
fn browser_icon_name() -> &'static str {
    if let Some(display) = gdk::Display::default() {
        if gtk::IconTheme::for_display(&display).has_icon("firefox") {
            return "firefox";
        }
    }
    "web-browser-symbolic"
}

fn status_page(icon: &str, title: &str, description: &str) -> adw::StatusPage {
    adw::StatusPage::builder()
        .icon_name(icon)
        .title(title)
        .description(description)
        .build()
}

fn centered_spinner() -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 0);
    b.set_valign(gtk::Align::Center);
    b.set_halign(gtk::Align::Center);
    b.set_vexpand(true);
    let spinner = gtk::Spinner::new();
    spinner.start();
    spinner.set_size_request(32, 32);
    b.append(&spinner);
    b
}

fn feed_row(title: &str, unread: i64, has_error: bool) -> gtk::Widget {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.set_margin_top(6);
    row.set_margin_bottom(6);
    row.set_margin_start(6);
    row.set_margin_end(6);

    if has_error {
        let warn = gtk::Image::from_icon_name("dialog-warning-symbolic");
        warn.set_tooltip_text(Some("This feed had a fetch error"));
        row.append(&warn);
    }

    let name = gtk::Label::new(None);
    name.set_xalign(0.0);
    name.set_ellipsize(pango::EllipsizeMode::End);
    name.set_hexpand(true);
    if unread > 0 {
        name.set_markup(&format!("<b>{}</b>", glib::markup_escape_text(title)));
    } else {
        name.set_text(title);
    }
    row.append(&name);

    if unread > 0 {
        let badge = gtk::Label::new(Some(&unread.to_string()));
        badge.add_css_class("dim-label");
        badge.add_css_class("numeric");
        row.append(&badge);
    }
    row.upcast()
}

/// Returns the row widget and its title label (so it can be un-bolded on read).
fn article_row(article: &Article) -> (gtk::Widget, gtk::Label) {
    let container = gtk::Box::new(gtk::Orientation::Vertical, 3);
    container.set_margin_top(8);
    container.set_margin_bottom(8);
    container.set_margin_start(8);
    container.set_margin_end(8);

    let title = gtk::Label::new(None);
    title.set_xalign(0.0);
    title.set_wrap(true);
    title.set_wrap_mode(pango::WrapMode::WordChar);
    title.set_lines(2);
    title.set_ellipsize(pango::EllipsizeMode::End);
    if article.is_read {
        title.set_text(&article.title);
    } else {
        title.set_markup(&format!("<b>{}</b>", glib::markup_escape_text(&article.title)));
    }
    container.append(&title);

    if let Some(published) = article.published {
        let meta = gtk::Label::new(Some(&published.format("%b %d, %Y · %H:%M").to_string()));
        meta.set_xalign(0.0);
        meta.add_css_class("dim-label");
        meta.add_css_class("caption");
        container.append(&meta);
    }

    (container.upcast(), title)
}

fn format_meta(article: &Article) -> String {
    match article.published {
        Some(p) => p.format("%A, %B %-d, %Y at %H:%M").to_string(),
        None => "Unknown date".to_string(),
    }
}

fn clear_list(list: &gtk::ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}
