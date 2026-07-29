//! The viewer window: a 3-pane libadwaita UI (feeds | articles | reader) wired
//! to the shared database and the daemon over IPC.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use fodder_core::db::{articles, feeds, Db};
use fodder_core::discovery::{self, DiscoveredFeed, DiscoveryResult};
use fodder_core::ipc::IpcMessage;
use fodder_core::models::Article;
use fodder_core::{paths, Config};
use gtk4 as gtk;
use gtk::prelude::*;
use gtk::{gdk, gio, glib, pango};
use libadwaita as adw;
use adw::prelude::*;
use tokio::sync::mpsc::{self, UnboundedSender};
use webkit6::prelude::*;

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
    webkit_toggle: gtk::ToggleButton,
    webkit_holder: gtk::Box,
    webview: RefCell<Option<webkit6::WebView>>,

    // Row → id maps, parallel to the list rows.
    feed_ids: RefCell<Vec<Option<i64>>>,
    article_ids: RefCell<Vec<i64>>,
    article_titles: RefCell<Vec<gtk::Label>>,

    selected_feed: Cell<Option<i64>>,
    current_article: Cell<Option<i64>>,
    current_url: RefCell<Option<String>>,
    current_content: RefCell<Option<String>>,
    suppress: Cell<bool>,

    db: DbHandle,
    rt: tokio::runtime::Runtime,
    http: reqwest::Client,
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
    let prefs_btn = icon_button("preferences-system-symbolic", "Preferences");
    let feeds_header = adw::HeaderBar::new();
    feeds_header.set_title_widget(Some(&adw::WindowTitle::new("Feeds", "")));
    feeds_header.pack_start(&add_btn);
    feeds_header.pack_start(&remove_btn);
    feeds_header.pack_end(&refresh_btn);
    feeds_header.pack_end(&prefs_btn);
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
    // Full WebKit view lives on its own stack page; the WebView is created and
    // destroyed on demand so its subprocesses only exist while in use.
    let webkit_holder = gtk::Box::new(gtk::Orientation::Vertical, 0);
    webkit_holder.set_vexpand(true);
    webkit_holder.set_hexpand(true);

    let reader_stack = gtk::Stack::new();
    reader_stack.add_named(&reader_scroll, Some("content"));
    reader_stack.add_named(&webkit_holder, Some("webkit"));
    reader_stack.add_named(&status_page("emblem-documents-symbolic", "Select an article", "Choose an article from the list to read it here."), Some("empty"));
    reader_stack.add_named(&reader_error, Some("error"));
    reader_stack.set_visible_child_name("empty");

    let webkit_toggle = gtk::ToggleButton::new();
    webkit_toggle.set_icon_name("globe-symbolic");
    webkit_toggle.set_tooltip_text(Some("Full web view (JavaScript disabled)"));
    let open_btn = icon_button(browser_icon_name(), "Open in browser");
    let reader_header = adw::HeaderBar::new();
    reader_header.set_title_widget(Some(&adw::WindowTitle::new("Reader", "")));
    reader_header.pack_end(&open_btn);
    reader_header.pack_end(&webkit_toggle);
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

    let http = reqwest::Client::builder()
        .user_agent(concat!("FodderReader/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .unwrap_or_default();

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
        webkit_toggle: webkit_toggle.clone(),
        webkit_holder,
        webview: RefCell::new(None),
        feed_ids: RefCell::new(Vec::new()),
        article_ids: RefCell::new(Vec::new()),
        article_titles: RefCell::new(Vec::new()),
        selected_feed: Cell::new(None),
        current_article: Cell::new(None),
        current_url: RefCell::new(None),
        current_content: RefCell::new(None),
        suppress: Cell::new(false),
        db,
        rt,
        http,
        cmd_tx,
    });

    wire_signals(
        &this, &add_btn, &remove_btn, &refresh_btn, &mark_all_btn, &open_btn, &prefs_btn,
        &webkit_toggle,
    );
    this
}

fn wire_signals(
    this: &Rc<App>,
    add_btn: &gtk::Button,
    remove_btn: &gtk::Button,
    refresh_btn: &gtk::Button,
    mark_all_btn: &gtk::Button,
    open_btn: &gtk::Button,
    prefs_btn: &gtk::Button,
    webkit_toggle: &gtk::ToggleButton,
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

    let a = this.clone();
    prefs_btn.connect_clicked(move |_| a.open_settings());

    let a = this.clone();
    webkit_toggle.connect_toggled(move |btn| {
        if btn.is_active() {
            a.enable_webkit();
        } else {
            a.disable_webkit();
        }
    });
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
        *self.current_content.borrow_mut() = article.content.clone();

        // Light (Pango) rendering — always kept up to date on the content page.
        let markup = reader::html_to_pango(article.content.as_deref().unwrap_or(""));
        if markup.is_empty() {
            self.reader_body
                .set_text("(No content — use “Open in browser”.)");
        } else {
            self.reader_body.set_markup(&markup);
        }

        // Show the chosen renderer.
        if self.webkit_toggle.is_active() {
            self.enable_webkit();
        } else {
            self.reader_stack.set_visible_child_name("content");
        }
        self.inner_split.set_show_content(true);

        if !article.is_read {
            self.mark_read(article.id);
        }
    }

    /// Switch the reader to the full WebKit view of the current article. Creates
    /// a fresh WebView (JS off, remote images gated, ephemeral session),
    /// dropping any previous one so its subprocesses are released.
    fn enable_webkit(self: &Rc<Self>) {
        let Some(html) = self.current_content.borrow().clone() else {
            // Nothing to show; revert the toggle.
            self.webkit_toggle.set_active(false);
            return;
        };

        if let Some(old) = self.webview.borrow_mut().take() {
            self.webkit_holder.remove(&old);
        }
        let webview = build_webview(&html);
        self.webkit_holder.append(&webview);
        *self.webview.borrow_mut() = Some(webview);
        self.reader_stack.set_visible_child_name("webkit");
    }

    /// Return to the light renderer and fully destroy the WebView.
    fn disable_webkit(self: &Rc<Self>) {
        if let Some(webview) = self.webview.borrow_mut().take() {
            self.webkit_holder.remove(&webview);
            // `webview` drops here → last ref gone → WebKit subprocesses exit.
        }
        if self.current_article.get().is_some() {
            self.reader_stack.set_visible_child_name("content");
        } else {
            self.reader_stack.set_visible_child_name("empty");
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

    /// Step 1: ask for a URL, then run feed discovery on it.
    fn add_feed_dialog(self: &Rc<Self>) {
        let entry = gtk::Entry::builder()
            .placeholder_text("https://example.com  or  …/feed.xml")
            .activates_default(true)
            .build();
        let dialog = adw::AlertDialog::new(
            Some("Add feed"),
            Some("Enter a website or feed URL. Fodder will look for its feed."),
        );
        dialog.set_extra_child(Some(&entry));
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("find", "Find");
        dialog.set_response_appearance("find", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("find"));
        dialog.set_close_response("cancel");

        let this = self.clone();
        dialog.connect_response(None, move |_, response| {
            if response == "find" {
                let url = entry.text().trim().to_string();
                if !url.is_empty() {
                    this.discover(url);
                }
            }
        });
        dialog.present(Some(&self.window));
    }

    /// Step 2: resolve the URL to a feed (direct, candidates, or none).
    fn discover(self: &Rc<Self>, url: String) {
        let client = self.http.clone();
        let this = self.clone();
        let user_url = url.clone();
        runtime::run_async(
            self.rt.handle(),
            async move {
                discovery::resolve_feed(&client, &url)
                    .await
                    .map_err(|e| e.to_string())
            },
            move |res| match res {
                // Direct feed: resolve_feed already parsed the feed's title.
                Ok(DiscoveryResult::DirectFeed { url, title }) => this.confirm_subscribe(url, title),
                Ok(DiscoveryResult::Candidates(mut candidates)) => {
                    if candidates.len() == 1 {
                        // Exactly one feed — no picker, just confirm it.
                        let only = candidates.remove(0);
                        let title = title_for(&only, &user_url);
                        this.confirm_subscribe(only.url, title);
                    } else {
                        this.pick_candidate(candidates, user_url);
                    }
                }
                Ok(DiscoveryResult::None) => this.info_dialog(
                    "No feed found",
                    &format!("Couldn't find an RSS/Atom/JSON feed at:\n{user_url}"),
                ),
                Err(e) => this.info_dialog("Couldn't fetch that URL", &e),
            },
        );
    }

    /// Step 3a: a single feed — preview its title and confirm.
    fn confirm_subscribe(self: &Rc<Self>, feed_url: String, title: String) {
        let dialog = adw::AlertDialog::new(
            Some("Subscribe to this feed?"),
            Some(&format!("{title}\n\n{feed_url}")),
        );
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("subscribe", "Subscribe");
        dialog.set_response_appearance("subscribe", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("subscribe"));
        dialog.set_close_response("cancel");

        let this = self.clone();
        dialog.connect_response(None, move |_, response| {
            if response == "subscribe" {
                let _ = this.cmd_tx.send(IpcMessage::SubscribeResolved {
                    feed_url: feed_url.clone(),
                    title: title.clone(),
                });
            }
        });
        dialog.present(Some(&self.window));
    }

    /// Step 3b: several candidate feeds — let the user pick one.
    fn pick_candidate(self: &Rc<Self>, candidates: Vec<DiscoveredFeed>, user_url: String) {
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.add_css_class("boxed-list");
        for feed in &candidates {
            let title = feed.title.clone().unwrap_or_else(|| feed.url.clone());
            let row = adw::ActionRow::builder()
                .title(glib::markup_escape_text(&title).as_str())
                .subtitle(glib::markup_escape_text(&feed.url).as_str())
                .build();
            list.append(&row);
        }
        if let Some(first) = list.row_at_index(0) {
            list.select_row(Some(&first));
        }
        let scroller = gtk::ScrolledWindow::builder()
            .min_content_height(180)
            .child(&list)
            .build();

        let dialog = adw::AlertDialog::new(
            Some("Choose a feed"),
            Some("This page offers more than one feed."),
        );
        dialog.set_extra_child(Some(&scroller));
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("subscribe", "Subscribe");
        dialog.set_response_appearance("subscribe", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("subscribe"));
        dialog.set_close_response("cancel");

        let this = self.clone();
        dialog.connect_response(None, move |_, response| {
            if response == "subscribe" {
                if let Some(row) = list.selected_row() {
                    if let Some(feed) = candidates.get(row.index() as usize) {
                        let title = title_for(feed, &user_url);
                        let _ = this.cmd_tx.send(IpcMessage::SubscribeResolved {
                            feed_url: feed.url.clone(),
                            title,
                        });
                    }
                }
            }
        });
        dialog.present(Some(&self.window));
    }

    /// A simple informational dialog with a single dismiss button.
    fn info_dialog(self: &Rc<Self>, heading: &str, body: &str) {
        let dialog = adw::AlertDialog::new(Some(heading), Some(body));
        dialog.add_response("ok", "OK");
        dialog.set_default_response(Some("ok"));
        dialog.set_close_response("ok");
        dialog.present(Some(&self.window));
    }

    /// The preferences dialog: autostart toggle + poll interval.
    fn open_settings(self: &Rc<Self>) {
        let group = adw::PreferencesGroup::new();
        group.set_title("General");

        let autostart = adw::SwitchRow::new();
        autostart.set_title("Start daemon at login");
        autostart.set_subtitle("Installs an autostart entry for the poller");
        autostart.set_active(fodder_core::autostart::is_enabled());
        autostart.connect_active_notify(|row| {
            if let Err(e) = fodder_core::autostart::set_enabled(row.is_active()) {
                tracing::warn!("autostart toggle failed: {e}");
            }
        });
        group.add(&autostart);

        let interval = adw::SpinRow::with_range(5.0, 1440.0, 5.0);
        interval.set_title("Poll interval (minutes)");
        interval.set_subtitle("Minimum 5. Applies after the daemon restarts.");
        let current = paths::config_path()
            .ok()
            .map(|p| Config::load(&p).unwrap_or_default())
            .unwrap_or_default();
        interval.set_value(f64::from(current.poll_interval_minutes));
        interval.connect_value_notify(|row| {
            if let Ok(path) = paths::config_path() {
                let mut cfg = Config::load(&path).unwrap_or_default();
                cfg.poll_interval_minutes = (row.value() as u32).max(5);
                if let Err(e) = cfg.save(&path) {
                    tracing::warn!("saving config failed: {e}");
                }
            }
        });
        group.add(&interval);

        let page = adw::PreferencesPage::new();
        page.add(&group);
        let dialog = adw::PreferencesDialog::new();
        dialog.add(&page);
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
        *self.current_content.borrow_mut() = None;
        if let Some(webview) = self.webview.borrow_mut().take() {
            self.webkit_holder.remove(&webview);
        }
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

/// Pick a display title for a discovered feed: the advertised feed title, else
/// the feed URL, else the URL the user typed.
fn title_for(feed: &DiscoveredFeed, user_url: &str) -> String {
    feed.title
        .clone()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| {
            if feed.url.trim().is_empty() {
                user_url.to_string()
            } else {
                feed.url.clone()
            }
        })
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

/// Build a locked-down WebView for the full reader: JavaScript disabled, remote
/// images/content gated, and an ephemeral session (no cookies/cache persisted).
fn build_webview(html: &str) -> webkit6::WebView {
    let settings = webkit6::Settings::new();
    settings.set_enable_javascript(false);
    settings.set_enable_javascript_markup(false);
    settings.set_auto_load_images(false); // gate remote images/trackers
    settings.set_enable_webgl(false);
    settings.set_enable_html5_local_storage(false);

    let session = webkit6::NetworkSession::new_ephemeral();
    let webview = webkit6::WebView::builder()
        .network_session(&session)
        .settings(&settings)
        .build();
    webview.set_vexpand(true);
    webview.set_hexpand(true);
    // No base URI: relative subresources don't resolve to a remote origin.
    webview.load_html(html, None);
    webview
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
