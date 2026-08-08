//! The viewer window: a 3-pane libadwaita UI (feeds | articles | reader) wired
//! to the shared database and the daemon over IPC.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use adw::prelude::*;
use fodder_core::db::{articles, feeds, Db};
use fodder_core::discovery::{self, DiscoveredFeed, DiscoveryResult};
use fodder_core::ipc::IpcMessage;
use fodder_core::models::Article;
use fodder_core::{paths, Config};
use gtk::{gdk, gio, glib, pango};
use gtk4 as gtk;
use libadwaita as adw;
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
    /// Restore straight into the full WebKit view.
    pub webkit: bool,
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
    web_back: gtk::Button,
    web_forward: gtk::Button,

    // Row → data maps, parallel to the list rows.
    feed_ids: RefCell<Vec<Option<i64>>>,
    feed_urls: RefCell<Vec<Option<String>>>,
    article_ids: RefCell<Vec<i64>>,
    article_titles: RefCell<Vec<gtk::Label>>,
    article_urls: RefCell<Vec<Option<String>>>,
    article_read: RefCell<Vec<bool>>,

    selected_feed: Cell<Option<i64>>,
    current_article: Cell<Option<i64>>,
    current_url: RefCell<Option<String>>,
    current_content: RefCell<Option<String>>,
    suppress: Cell<bool>,
    /// Set from a restore target: activate the web view on the first article.
    pending_webkit: Cell<bool>,

    // Right-click context menus and the row they target.
    feed_popover: gtk::PopoverMenu,
    article_popover: gtk::PopoverMenu,
    ctx_feed: Cell<Option<i64>>,
    ctx_feed_url: RefCell<Option<String>>,
    ctx_article: Cell<Option<i64>>,
    ctx_article_url: RefCell<Option<String>>,
    ctx_article_read: Cell<bool>,

    /// When set, closing the window exits the process to reclaim its memory
    /// (mirrors `Config::low_memory_mode`); otherwise closing hides it and keeps
    /// the process resident for an instant reopen.
    low_memory: Cell<bool>,

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

    this.pending_webkit.set(target.webkit);
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
    // The gear now opens a menu (Preferences / About Fodder) rather than the
    // preferences dialog directly. Its actions live in the `appmenu` group
    // registered on the window (see `setup_app_menu`).
    let menu_btn = gtk::MenuButton::builder()
        .icon_name("preferences-system-symbolic")
        .tooltip_text("Menu")
        .menu_model(&build_app_menu())
        .build();
    let feeds_header = adw::HeaderBar::new();
    feeds_header.set_title_widget(Some(&adw::WindowTitle::new("Feeds", "")));
    feeds_header.pack_start(&add_btn);
    feeds_header.pack_start(&remove_btn);
    feeds_header.pack_end(&refresh_btn);
    feeds_header.pack_end(&menu_btn);
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
    articles_stack.add_named(
        &status_page(
            "view-list-symbolic",
            "No articles",
            "This feed has no articles yet.",
        ),
        Some("empty"),
    );
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
    reader_stack.add_named(&centered_spinner(), Some("webloading"));
    reader_stack.add_named(
        &status_page(
            "emblem-documents-symbolic",
            "Select an article",
            "Choose an article from the list to read it here.",
        ),
        Some("empty"),
    );
    reader_stack.add_named(&reader_error, Some("error"));
    reader_stack.set_visible_child_name("empty");

    // Back/forward for in-view navigation; only shown in web mode.
    let web_back = icon_button("go-previous-symbolic", "Back");
    let web_forward = icon_button("go-next-symbolic", "Forward");
    web_back.set_visible(false);
    web_forward.set_visible(false);

    let webkit_toggle = gtk::ToggleButton::new();
    webkit_toggle.set_icon_name("globe-symbolic");
    webkit_toggle.set_tooltip_text(Some("Full web page (loads the live site; JavaScript off)"));
    let open_btn = icon_button(browser_icon_name(), "Open in browser");
    let reader_header = adw::HeaderBar::new();
    reader_header.set_title_widget(Some(&adw::WindowTitle::new("Reader", "")));
    reader_header.pack_start(&web_back);
    reader_header.pack_start(&web_forward);
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

    // Context-menu popovers. Parent them to the panes (not the lists): a popover
    // parented inside the ScrolledWindow gets its height clamped to the visible
    // area and shows a scrollbar. The panes sit outside the scroll.
    let feed_popover = gtk::PopoverMenu::from_model(Some(&gio::Menu::new()));
    feed_popover.set_parent(&feeds_pane);
    feed_popover.set_has_arrow(false);
    let article_popover = gtk::PopoverMenu::from_model(Some(&gio::Menu::new()));
    article_popover.set_parent(&articles_pane);
    article_popover.set_has_arrow(false);

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
        web_back: web_back.clone(),
        web_forward: web_forward.clone(),
        feed_ids: RefCell::new(Vec::new()),
        feed_urls: RefCell::new(Vec::new()),
        article_ids: RefCell::new(Vec::new()),
        article_titles: RefCell::new(Vec::new()),
        article_urls: RefCell::new(Vec::new()),
        article_read: RefCell::new(Vec::new()),
        selected_feed: Cell::new(None),
        current_article: Cell::new(None),
        current_url: RefCell::new(None),
        current_content: RefCell::new(None),
        suppress: Cell::new(false),
        pending_webkit: Cell::new(false),
        feed_popover,
        article_popover,
        ctx_feed: Cell::new(None),
        ctx_feed_url: RefCell::new(None),
        ctx_article: Cell::new(None),
        ctx_article_url: RefCell::new(None),
        ctx_article_read: Cell::new(false),
        low_memory: Cell::new(
            paths::config_path()
                .ok()
                .map(|p| Config::load(&p).unwrap_or_default().low_memory_mode)
                .unwrap_or(false),
        ),
        db,
        rt,
        http,
        cmd_tx,
    });

    // Closing the window keeps the process resident by default (hide it) for an
    // instant reopen. The live WebView is deliberately kept too, so returning to
    // a full-view article is instant rather than a fresh WebKit spawn + reload -
    // holding its subprocesses is the memory cost this default accepts. Low-memory
    // mode exits on close instead, freeing everything (its escape hatch for that
    // WebKit memory mid-session is toggling back to the light reader, which still
    // destroys the view).
    let close_app = this.clone();
    this.window.connect_close_request(move |window| {
        if close_app.low_memory.get() {
            glib::Propagation::Proceed
        } else {
            window.set_visible(false);
            glib::Propagation::Stop
        }
    });

    wire_signals(
        &this,
        &add_btn,
        &remove_btn,
        &refresh_btn,
        &mark_all_btn,
        &open_btn,
        &webkit_toggle,
    );
    setup_app_menu(&this);

    // In-view navigation buttons.
    let a = this.clone();
    web_back.connect_clicked(move |_| {
        if let Some(webview) = a.webview.borrow().as_ref() {
            webview.go_back();
        }
    });
    let a = this.clone();
    web_forward.connect_clicked(move |_| {
        if let Some(webview) = a.webview.borrow().as_ref() {
            webview.go_forward();
        }
    });

    setup_context_menus(&this);
    this
}

/// Install right-click context menus (and their action groups) on both lists.
fn setup_context_menus(this: &Rc<App>) {
    // --- Feed list ---
    let group = gio::SimpleActionGroup::new();
    add_action(&group, "refresh", this, |app| {
        let _ = app.cmd_tx.send(IpcMessage::RefreshNow {
            feed_id: app.ctx_feed.get(),
        });
    });
    add_action(&group, "markread", this, |app| {
        app.do_mark_all_read(app.ctx_feed.get());
    });
    add_action(&group, "copyurl", this, |app| {
        if let Some(url) = app.ctx_feed_url.borrow().clone() {
            app.window.clipboard().set_text(&url);
        }
    });
    add_action(&group, "rename", this, |app| {
        if let Some(id) = app.ctx_feed.get() {
            app.rename_feed_dialog(id);
        }
    });
    add_action(&group, "remove", this, |app| {
        if let Some(id) = app.ctx_feed.get() {
            app.remove_feed(id);
        }
    });
    // Action groups go on the window so the pane-parented popovers can resolve
    // them (action lookup walks up from the popover).
    this.window.insert_action_group("feedctx", Some(&group));

    let gesture = gtk::GestureClick::new();
    gesture.set_button(gdk::BUTTON_SECONDARY);
    let app = this.clone();
    gesture.connect_pressed(move |_, _, x, y| {
        let Some(row) = app.feeds_list.row_at_y(y as i32) else {
            return;
        };
        app.feeds_list.select_row(Some(&row));
        let idx = row.index() as usize;
        let feed_id = app.feed_ids.borrow().get(idx).copied().flatten();
        let url = app.feed_urls.borrow().get(idx).cloned().flatten();
        app.ctx_feed.set(feed_id);
        *app.ctx_feed_url.borrow_mut() = url;
        app.feed_popover
            .set_menu_model(Some(&build_feed_menu(feed_id.is_some())));
        point_popover_at(&app.feed_popover, &app.feeds_list, x, y);
        app.feed_popover.popup();
    });
    this.feeds_list.add_controller(gesture);

    // --- Article list ---
    let group = gio::SimpleActionGroup::new();
    add_action(&group, "toggleread", this, |app| {
        app.toggle_ctx_article_read()
    });
    add_action(&group, "openbrowser", this, |app| {
        if let Some(url) = app.ctx_article_url.borrow().clone() {
            open_uri(&app.window, &url);
        }
    });
    add_action(&group, "copylink", this, |app| {
        if let Some(url) = app.ctx_article_url.borrow().clone() {
            app.window.clipboard().set_text(&url);
        }
    });
    this.window.insert_action_group("artctx", Some(&group));

    let gesture = gtk::GestureClick::new();
    gesture.set_button(gdk::BUTTON_SECONDARY);
    let app = this.clone();
    gesture.connect_pressed(move |_, _, x, y| {
        let Some(row) = app.articles_list.row_at_y(y as i32) else {
            return;
        };
        app.articles_list.select_row(Some(&row));
        let idx = row.index() as usize;
        let Some(id) = app.article_ids.borrow().get(idx).copied() else {
            return;
        };
        let url = app.article_urls.borrow().get(idx).cloned().flatten();
        let is_read = app.article_read.borrow().get(idx).copied().unwrap_or(false);
        app.ctx_article.set(Some(id));
        *app.ctx_article_url.borrow_mut() = url;
        app.ctx_article_read.set(is_read);
        app.article_popover
            .set_menu_model(Some(&build_article_menu(is_read)));
        point_popover_at(&app.article_popover, &app.articles_list, x, y);
        app.article_popover.popup();
    });
    this.articles_list.add_controller(gesture);
}

/// Point a popover at (x, y) given in `source`'s coordinate space, translating
/// into the popover's parent coordinate space (the pane it's parented to).
fn point_popover_at(popover: &gtk::PopoverMenu, source: &gtk::ListBox, x: f64, y: f64) {
    let point = gtk::graphene::Point::new(x as f32, y as f32);
    let (px, py) = popover
        .parent()
        .and_then(|parent| source.compute_point(&parent, &point))
        .map(|p| (p.x() as i32, p.y() as i32))
        .unwrap_or((x as i32, y as i32));
    popover.set_pointing_to(Some(&gdk::Rectangle::new(px, py, 1, 1)));
}

/// Register a `SimpleAction` whose activation runs `f` with the app.
fn add_action(
    group: &gio::SimpleActionGroup,
    name: &str,
    this: &Rc<App>,
    f: impl Fn(&Rc<App>) + 'static,
) {
    let action = gio::SimpleAction::new(name, None);
    let app = this.clone();
    action.connect_activate(move |_, _| f(&app));
    group.add_action(&action);
}

fn build_feed_menu(is_real_feed: bool) -> gio::Menu {
    let menu = gio::Menu::new();
    menu.append(Some("Refresh"), Some("feedctx.refresh"));
    menu.append(Some("Mark all as read"), Some("feedctx.markread"));
    if is_real_feed {
        menu.append(Some("Rename"), Some("feedctx.rename"));
        menu.append(Some("Delete"), Some("feedctx.remove"));
        menu.append(Some("Copy feed URL"), Some("feedctx.copyurl"));
    }
    menu
}

fn build_article_menu(is_read: bool) -> gio::Menu {
    let menu = gio::Menu::new();
    let toggle_label = if is_read {
        "Mark as unread"
    } else {
        "Mark as read"
    };
    menu.append(Some(toggle_label), Some("artctx.toggleread"));
    menu.append(Some("Open in browser"), Some("artctx.openbrowser"));
    menu.append(Some("Copy link"), Some("artctx.copylink"));
    menu
}

/// The header-bar gear menu: Preferences (the settings dialog) and About Fodder.
fn build_app_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    menu.append(Some("Preferences"), Some("appmenu.preferences"));
    menu.append(Some("About Fodder"), Some("appmenu.about"));
    menu
}

/// Install the gear menu's action group on the window so the `MenuButton`'s
/// menu model can resolve `appmenu.preferences` / `appmenu.about`.
fn setup_app_menu(this: &Rc<App>) {
    let group = gio::SimpleActionGroup::new();
    add_action(&group, "preferences", this, |app| app.open_settings());
    add_action(&group, "about", this, |app| app.open_about());
    this.window.insert_action_group("appmenu", Some(&group));
}

/// Open a URI in the external browser.
fn open_uri(window: &adw::ApplicationWindow, uri: &str) {
    let launcher = gtk::UriLauncher::new(uri);
    launcher.launch(Some(window), gio::Cancellable::NONE, |res| {
        if let Err(e) = res {
            tracing::warn!("open in browser failed: {e}");
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn wire_signals(
    this: &Rc<App>,
    add_btn: &gtk::Button,
    remove_btn: &gtk::Button,
    refresh_btn: &gtk::Button,
    mark_all_btn: &gtk::Button,
    open_btn: &gtk::Button,
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
    webkit_toggle.connect_toggled(move |btn| {
        if btn.is_active() {
            a.enable_webkit();
        } else {
            a.disable_webkit();
        }
        a.report_reading_state();
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
        let mut urls: Vec<Option<String>> = Vec::with_capacity(feeds.len() + 1);

        // "All Articles" aggregate row.
        self.feeds_list
            .append(&feed_row("All Articles", total, false));
        ids.push(None);
        urls.push(None);

        for feed in &feeds {
            let count = unread.get(&feed.id).copied().unwrap_or(0);
            let has_error = feed.last_error.is_some();
            self.feeds_list
                .append(&feed_row(&feed.title, count, has_error));
            ids.push(Some(feed.id));
            urls.push(Some(feed.url.clone()));
        }
        *self.feed_ids.borrow_mut() = ids;
        *self.feed_urls.borrow_mut() = urls;
        self.suppress.set(false);

        // Decide which feed to show.
        let want_feed = target
            .and_then(|t| t.feed)
            .or_else(|| self.selected_feed.get());
        let idx = self
            .feed_ids
            .borrow()
            .iter()
            .position(|f| *f == want_feed)
            .unwrap_or(0);
        let feed_id = self.feed_ids.borrow().get(idx).copied().flatten();
        // Select the row without re-triggering the row-selected handler (which
        // would navigate with no article and clear the restore state).
        self.suppress.set(true);
        self.select_feed_row(idx);
        self.suppress.set(false);
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
        let mut urls = Vec::with_capacity(arts.len());
        let mut reads = Vec::with_capacity(arts.len());
        for art in &arts {
            let (row, title_label) = article_row(art);
            self.articles_list.append(&row);
            ids.push(art.id);
            labels.push(title_label);
            urls.push(art.url.clone());
            reads.push(art.is_read);
        }
        *self.article_ids.borrow_mut() = ids;
        *self.article_titles.borrow_mut() = labels;
        *self.article_urls.borrow_mut() = urls;
        *self.article_read.borrow_mut() = reads;
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
                // Suppress so the programmatic selection doesn't re-open it.
                self.suppress.set(true);
                self.select_article_row(pos);
                self.suppress.set(false);
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

        // Restoring a session that was in web mode: flip into it now that the
        // article's content is loaded (this fires the toggle handler).
        if self.pending_webkit.replace(false) {
            self.webkit_toggle.set_active(true);
        }

        if !article.is_read {
            self.mark_read(article.id);
        }
        self.report_reading_state();
    }

    /// Tell the daemon what we're showing, so it can restore it on the next open.
    fn report_reading_state(&self) {
        let _ = self.cmd_tx.send(IpcMessage::ReadingState {
            feed_id: self.selected_feed.get(),
            article_id: self.current_article.get(),
            webkit: self.webkit_toggle.is_active(),
        });
    }

    /// Switch the reader to the full WebKit view: load the live article page
    /// (JavaScript off, images on, ephemeral session with tracking prevention).
    /// A fresh WebView is created each time, dropping any previous one so its
    /// subprocesses are released.
    fn enable_webkit(self: &Rc<Self>) {
        let url = self.current_url.borrow().clone();
        let content = self.current_content.borrow().clone();
        if url.is_none() && content.is_none() {
            self.webkit_toggle.set_active(false);
            return;
        }

        if let Some(old) = self.webview.borrow_mut().take() {
            old.terminate_web_process();
            self.webkit_holder.remove(&old);
        }

        let webview = configure_webview(url.as_deref(), content.as_deref().unwrap_or(""));

        // Reveal the page once it commits/finishes and keep nav buttons in sync.
        let app = self.clone();
        webview.connect_load_changed(move |wv, event| match event {
            webkit6::LoadEvent::Committed | webkit6::LoadEvent::Finished => {
                app.reader_stack.set_visible_child_name("webkit");
                app.web_back.set_sensitive(wv.can_go_back());
                app.web_forward.set_sensitive(wv.can_go_forward());
            }
            _ => {}
        });
        // On failure, reveal WebKit's own error page instead of hanging the spinner.
        let app = self.clone();
        webview.connect_load_failed(move |_wv, _event, _uri, _err| {
            app.reader_stack.set_visible_child_name("webkit");
            false // let WebKit render its default error page
        });

        self.webkit_holder.append(&webview);
        *self.webview.borrow_mut() = Some(webview);

        self.web_back.set_visible(true);
        self.web_forward.set_visible(true);
        self.web_back.set_sensitive(false);
        self.web_forward.set_sensitive(false);
        self.reader_stack.set_visible_child_name("webloading");
    }

    /// Return to the light renderer and fully destroy the WebView.
    fn disable_webkit(self: &Rc<Self>) {
        self.destroy_webview();
        if self.current_article.get().is_some() {
            self.reader_stack.set_visible_child_name("content");
        } else {
            self.reader_stack.set_visible_child_name("empty");
        }
    }

    /// Drop the WebView and reclaim its subprocesses, then hide the nav buttons.
    fn destroy_webview(&self) {
        if let Some(webview) = self.webview.borrow_mut().take() {
            // Kill the renderer promptly rather than letting WebKit pool it for
            // reuse. Dropping the view then releases its dedicated context and
            // ephemeral session, so the network process exits too.
            webview.terminate_web_process();
            self.webkit_holder.remove(&webview);
        }
        self.web_back.set_visible(false);
        self.web_forward.set_visible(false);
    }

    /// Mark one article read: update the DB, un-bold its row, refresh badges.
    fn mark_read(self: &Rc<Self>, id: i64) {
        self.update_article_row_read(id, true);
        let this = self.clone();
        runtime::run_db(
            self.rt.handle(),
            self.db.clone(),
            move |c| articles::mark_read(c, id),
            move |res| {
                if let Err(e) = res {
                    tracing::warn!("mark_read failed: {e}");
                }
                this.refresh_badges();
            },
        );
    }

    /// Toggle the right-clicked article's read state.
    fn toggle_ctx_article_read(self: &Rc<Self>) {
        let Some(id) = self.ctx_article.get() else {
            return;
        };
        let was_read = self.ctx_article_read.get();
        let this = self.clone();
        runtime::run_db(
            self.rt.handle(),
            self.db.clone(),
            move |c| {
                if was_read {
                    articles::mark_unread(c, id)
                } else {
                    articles::mark_read(c, id)
                }
            },
            move |res| match res {
                Ok(()) => {
                    this.update_article_row_read(id, !was_read);
                    this.refresh_badges();
                }
                Err(e) => tracing::warn!("toggle read failed: {e}"),
            },
        );
    }

    /// Restyle an article row (bold = unread) and keep `article_read` in sync.
    fn update_article_row_read(&self, id: i64, is_read: bool) {
        let pos = self.article_ids.borrow().iter().position(|a| *a == id);
        if let Some(pos) = pos {
            if let Some(label) = self.article_titles.borrow().get(pos) {
                if is_read {
                    label.set_text(&label.text());
                } else {
                    label.set_markup(&format!(
                        "<b>{}</b>",
                        glib::markup_escape_text(&label.text())
                    ));
                }
            }
            if let Some(read) = self.article_read.borrow_mut().get_mut(pos) {
                *read = is_read;
            }
        }
    }

    fn mark_all_read(self: &Rc<Self>) {
        self.do_mark_all_read(self.selected_feed.get());
    }

    /// Mark all read for a feed (`None` = every feed) and refresh the view.
    fn do_mark_all_read(self: &Rc<Self>, feed_id: Option<i64>) {
        let this = self.clone();
        runtime::run_db(
            self.rt.handle(),
            self.db.clone(),
            move |c| articles::mark_all_read(c, feed_id),
            move |res| match res {
                Ok(()) => this.load_feeds(Some(Target {
                    feed: this.selected_feed.get(),
                    article: this.current_article.get(),
                    webkit: false,
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
        let mut urls: Vec<Option<String>> = Vec::with_capacity(feeds.len() + 1);
        self.feeds_list
            .append(&feed_row("All Articles", total, false));
        ids.push(None);
        urls.push(None);
        for feed in &feeds {
            let count = unread.get(&feed.id).copied().unwrap_or(0);
            self.feeds_list
                .append(&feed_row(&feed.title, count, feed.last_error.is_some()));
            ids.push(Some(feed.id));
            urls.push(Some(feed.url.clone()));
        }
        *self.feed_ids.borrow_mut() = ids;
        *self.feed_urls.borrow_mut() = urls;
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
            launcher.launch(Some(&self.window), gio::Cancellable::NONE, |res| {
                if let Err(e) = res {
                    tracing::warn!("open in browser failed: {e}");
                }
            });
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
                Ok(DiscoveryResult::DirectFeed { url, title }) => {
                    this.confirm_subscribe(url, title)
                }
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

    /// Load the config, apply `mutate`, save it, and tell the daemon to reload.
    fn save_config_and_reload(&self, mutate: impl FnOnce(&mut Config)) {
        if let Ok(path) = paths::config_path() {
            let mut cfg = Config::load(&path).unwrap_or_default();
            mutate(&mut cfg);
            if cfg.poll_interval_minutes < 5 {
                cfg.poll_interval_minutes = 5;
            }
            if let Err(e) = cfg.save(&path) {
                tracing::warn!("saving config failed: {e}");
                return;
            }
        }
        let _ = self.cmd_tx.send(IpcMessage::ReloadConfig);
    }

    /// The preferences dialog: general + notification settings.
    fn open_settings(self: &Rc<Self>) {
        let cfg = paths::config_path()
            .ok()
            .map(|p| Config::load(&p).unwrap_or_default())
            .unwrap_or_default();

        // --- General ---
        let general = adw::PreferencesGroup::new();
        general.set_title("General");

        let autostart = adw::SwitchRow::new();
        autostart.set_title("Launch at startup");
        autostart.set_subtitle("Starts the feed poller when you log in");
        autostart.set_active(fodder_core::autostart::is_enabled());
        let app = self.clone();
        autostart.connect_active_notify(move |row| {
            // Inside a Flatpak sandbox the autostart entry must go through the
            // Background portal, which the daemon owns; route the toggle to it.
            // Natively we can flip the `~/.config/autostart` file in-process
            // (also works when the viewer runs standalone with no daemon).
            if fodder_core::autostart::is_flatpak() {
                let _ = app.cmd_tx.send(IpcMessage::SetAutostart {
                    enabled: row.is_active(),
                });
            } else if let Err(e) = fodder_core::autostart::set_enabled(row.is_active()) {
                tracing::warn!("autostart toggle failed: {e}");
            }
        });
        general.add(&autostart);

        let interval = adw::SpinRow::with_range(5.0, 1440.0, 5.0);
        interval.set_title("Poll interval (minutes)");
        interval.set_subtitle("Minimum 5");
        interval.set_value(f64::from(cfg.poll_interval_minutes));
        let app = self.clone();
        interval.connect_value_notify(move |row| {
            let minutes = row.value() as u32;
            app.save_config_and_reload(move |c| c.poll_interval_minutes = minutes);
        });
        general.add(&interval);

        let low_memory = adw::SwitchRow::new();
        low_memory.set_title("Low memory mode");
        low_memory.set_subtitle("Free the window when closed; reopening is a little slower");
        low_memory.set_active(cfg.low_memory_mode);
        let app = self.clone();
        low_memory.connect_active_notify(move |row| {
            let on = row.is_active();
            // Update the live flag so the very next window close honors the new
            // choice without waiting for a restart.
            app.low_memory.set(on);
            app.save_config_and_reload(move |c| c.low_memory_mode = on);
        });
        general.add(&low_memory);

        // --- Notifications ---
        let notif = adw::PreferencesGroup::new();
        notif.set_title("Notifications");

        let master = adw::SwitchRow::new();
        master.set_title("Enable notifications");
        master.set_active(cfg.notifications_enabled);
        notif.add(&master);

        let new_articles = adw::SwitchRow::new();
        new_articles.set_title("New articles");
        new_articles.set_subtitle("Notify when polling finds new articles");
        new_articles.set_active(cfg.notify_new_articles);
        notif.add(&new_articles);

        let reminder = adw::SwitchRow::new();
        reminder.set_title("Daily reading reminder");
        reminder.set_subtitle("Once a day, if you have unread articles");
        reminder.set_active(cfg.daily_reminder_enabled);
        notif.add(&reminder);

        let (h0, m0) = cfg.reminder_hm().unwrap_or((10, 0));
        let hour = time_spin(0.0, 23.0, h0);
        let minute = time_spin(0.0, 59.0, m0);
        let time_box = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        time_box.set_valign(gtk::Align::Center);
        time_box.append(&hour);
        time_box.append(&gtk::Label::new(Some(":")));
        time_box.append(&minute);
        let time_row = adw::ActionRow::new();
        time_row.set_title("Reminder time");
        time_row.add_suffix(&time_box);
        notif.add(&time_row);

        // Conditional visibility: master gates the sub-options; the reminder
        // switch gates the time row.
        let update_vis: Rc<dyn Fn()> = {
            let master = master.clone();
            let reminder = reminder.clone();
            let new_articles = new_articles.clone();
            let time_row = time_row.clone();
            Rc::new(move || {
                let on = master.is_active();
                new_articles.set_visible(on);
                reminder.set_visible(on);
                time_row.set_visible(on && reminder.is_active());
            })
        };
        update_vis();

        let app = self.clone();
        let uv = update_vis.clone();
        master.connect_active_notify(move |row| {
            uv();
            let on = row.is_active();
            app.save_config_and_reload(move |c| c.notifications_enabled = on);
        });

        let app = self.clone();
        new_articles.connect_active_notify(move |row| {
            let on = row.is_active();
            app.save_config_and_reload(move |c| c.notify_new_articles = on);
        });

        let app = self.clone();
        let uv = update_vis.clone();
        reminder.connect_active_notify(move |row| {
            uv();
            let on = row.is_active();
            app.save_config_and_reload(move |c| c.daily_reminder_enabled = on);
        });

        let save_time = {
            let app = self.clone();
            let hour = hour.clone();
            let minute = minute.clone();
            Rc::new(move || {
                let t = format!("{:02}:{:02}", hour.value() as u32, minute.value() as u32);
                app.save_config_and_reload(move |c| c.daily_reminder_time = t);
            })
        };
        let st = save_time.clone();
        hour.connect_value_changed(move |_| st());
        let st = save_time.clone();
        minute.connect_value_changed(move |_| st());

        let page = adw::PreferencesPage::new();
        page.add(&general);
        page.add(&notif);
        let dialog = adw::PreferencesDialog::new();
        dialog.add(&page);
        dialog.present(Some(&self.window));
    }

    /// The About dialog: app identity, version, and project links.
    fn open_about(self: &Rc<Self>) {
        let about = adw::AboutDialog::builder()
            .application_name(fodder_core::APP_NAME)
            .application_icon(fodder_core::APP_ID)
            .version(fodder_core::VERSION)
            .comments(fodder_core::APP_DESCRIPTION)
            .website(fodder_core::REPOSITORY)
            .issue_url(format!("{}/issues", fodder_core::REPOSITORY))
            .developer_name("dipakmdhrm")
            .developers(["dipakmdhrm <dipakmdhrm@gmail.com>"])
            .license_type(gtk::License::MitX11)
            .copyright("© 2025 dipakmdhrm")
            .build();
        about.present(Some(&self.window));
    }

    fn remove_selected_feed(self: &Rc<Self>) {
        if let Some(feed_id) = self.selected_feed.get() {
            self.remove_feed(feed_id);
        }
    }

    fn remove_feed(self: &Rc<Self>, feed_id: i64) {
        let dialog = adw::AlertDialog::new(
            Some("Delete feed?"),
            Some("This unsubscribes the feed and deletes its stored articles."),
        );
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("remove", "Delete");
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

    /// Load the feed's current title, then show the rename dialog pre-filled.
    fn rename_feed_dialog(self: &Rc<Self>, feed_id: i64) {
        let this = self.clone();
        runtime::run_db(
            self.rt.handle(),
            self.db.clone(),
            move |c| feeds::get_feed(c, feed_id),
            move |res| match res {
                Ok(Some(feed)) => this.show_rename_dialog(feed_id, feed.title),
                Ok(None) => tracing::warn!("rename: feed {feed_id} not found"),
                Err(e) => tracing::warn!("rename: failed to load feed {feed_id}: {e}"),
            },
        );
    }

    fn show_rename_dialog(self: &Rc<Self>, feed_id: i64, current: String) {
        let entry = gtk::Entry::builder()
            .text(&current)
            .activates_default(true)
            .build();
        let dialog = adw::AlertDialog::new(
            Some("Rename feed"),
            Some("Enter a new title for this feed."),
        );
        dialog.set_extra_child(Some(&entry));
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("rename", "Rename");
        dialog.set_response_appearance("rename", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("rename"));
        dialog.set_close_response("cancel");

        let this = self.clone();
        dialog.connect_response(None, move |_, response| {
            if response == "rename" {
                let new_title = entry.text().trim().to_string();
                if !new_title.is_empty() && new_title != current {
                    let _ = this
                        .cmd_tx
                        .send(IpcMessage::RenameFeed { feed_id, new_title });
                }
            }
        });
        dialog.present(Some(&self.window));
    }

    fn handle_daemon(self: &Rc<Self>, event: FromDaemon) {
        match event {
            FromDaemon::Open => self.window.present(),
            FromDaemon::OpenAt {
                feed_id,
                article_id,
            } => {
                self.window.present();
                self.load_feeds(Some(Target {
                    feed: Some(feed_id),
                    article: article_id,
                    webkit: false,
                }));
            }
            FromDaemon::FeedsChanged => self.load_feeds(Some(Target {
                feed: self.selected_feed.get(),
                article: self.current_article.get(),
                webkit: false,
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
        self.pending_webkit.set(false); // a restore target that no longer exists
        self.destroy_webview();
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

/// Build the WebView for the full reader: load the live article page so it
/// renders as the site serves it (HTML, CSS, images). JavaScript stays
/// disabled, and the session is ephemeral with tracking prevention, so no
/// cookies/cache persist and cross-site trackers are curbed. Falls back to the
/// stored content fragment for articles without a URL.
fn configure_webview(url: Option<&str>, content: &str) -> webkit6::WebView {
    let settings = webkit6::Settings::new();
    settings.set_enable_javascript(false);
    settings.set_enable_javascript_markup(false);
    settings.set_enable_webgl(false);
    settings.set_enable_html5_local_storage(false);

    let session = webkit6::NetworkSession::new_ephemeral();
    session.set_itp_enabled(true); // intelligent tracking prevention

    // A dedicated context (not WebKit's shared, app-lifetime default) owns this
    // view's process cache, so dropping the view tears its processes down
    // instead of leaving them pooled for reuse. Only the WebView refs the
    // context/session, so they finalize when it's dropped.
    let context = webkit6::WebContext::new();
    let webview = webkit6::WebView::builder()
        .web_context(&context)
        .network_session(&session)
        .settings(&settings)
        .build();
    webview.set_vexpand(true);
    webview.set_hexpand(true);

    match url {
        Some(u) if !u.trim().is_empty() => webview.load_uri(u),
        _ => webview.load_html(content, None),
    }
    webview
}

/// A wrapping spin button (for hour/minute) that displays values zero-padded.
fn time_spin(min: f64, max: f64, value: u32) -> gtk::SpinButton {
    let spin = gtk::SpinButton::with_range(min, max, 1.0);
    spin.set_wrap(true);
    spin.set_numeric(true);
    spin.set_value(f64::from(value));
    spin.connect_output(|spin| {
        spin.set_text(&format!("{:02}", spin.value() as u32));
        glib::Propagation::Stop
    });
    spin
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
        title.set_markup(&format!(
            "<b>{}</b>",
            glib::markup_escape_text(&article.title)
        ));
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
    // Remove only rows. The list also parents a context-menu popover, which is
    // not a row — `ListBox::remove` can't remove it, so a naive
    // first_child()/remove() loop would spin forever on it.
    let mut child = list.first_child();
    while let Some(widget) = child {
        let next = widget.next_sibling();
        if let Ok(row) = widget.downcast::<gtk::ListBoxRow>() {
            list.remove(&row);
        }
        child = next;
    }
}
