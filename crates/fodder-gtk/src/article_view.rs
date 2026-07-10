//! Article pane. Native sanitized-HTML rendering by default, with a live
//! per-view WebKit toggle.
//!
//! The WebKit resource contract: the WebView and its EXCLUSIVELY-OWNED
//! ephemeral NetworkSession live only in `web_state`. Toggle-on creates
//! them lazily; toggle-off calls `terminate_web_process()` (killing the
//! WebKitWebProcess immediately), unparents the view, and drops both refs so
//! the NetworkProcess can exit too. Never store the WebView anywhere else.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4 as gtk;
use libadwaita as adw;

use adw::prelude::*;
use gtk::glib;
use webkit6::prelude::*;

use fodder_core::models::Item;

use crate::render::{ir, textview};

struct WebState {
    view: webkit6::WebView,
    // Held so the session outlives exactly as long as the view.
    _session: webkit6::NetworkSession,
}

pub struct ArticleView {
    pub root: gtk::Widget,
    /// Globe button for the pane header; window.rs packs it.
    pub web_toggle: gtk::ToggleButton,
    title: gtk::Label,
    meta: gtk::Label,
    body: gtk::TextView,
    stack: gtk::Stack,
    web_bin: adw::Bin,
    rendered: Rc<RefCell<textview::Rendered>>,
    current_item: RefCell<Option<Item>>,
    web_state: RefCell<Option<WebState>>,
}

impl ArticleView {
    pub fn new() -> Rc<Self> {
        let title = gtk::Label::builder()
            .css_classes(["title-2"])
            .halign(gtk::Align::Start)
            .wrap(true)
            .selectable(true)
            .build();
        let meta = gtk::Label::builder()
            .css_classes(["dim-label", "caption"])
            .halign(gtk::Align::Start)
            .wrap(true)
            .build();
        let body = gtk::TextView::builder()
            .editable(false)
            .cursor_visible(false)
            .wrap_mode(gtk::WrapMode::WordChar)
            .pixels_above_lines(2)
            .pixels_below_lines(2)
            .top_margin(12)
            .build();

        let rendered = Rc::new(RefCell::new(textview::Rendered::empty()));
        attach_link_handlers(&body, &rendered);

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .margin_top(18)
            .margin_bottom(18)
            .margin_start(18)
            .margin_end(18)
            .build();
        content.append(&title);
        content.append(&meta);
        content.append(&body);
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&content)
            .vexpand(true)
            .build();

        let placeholder = gtk::Label::builder()
            .label("Select an article to read")
            .css_classes(["dim-label", "title-3"])
            .vexpand(true)
            .build();

        let web_bin = adw::Bin::new();
        let web_toggle = gtk::ToggleButton::builder()
            .icon_name("web-browser-symbolic")
            .tooltip_text("Render with WebKit")
            .sensitive(false)
            .build();

        let stack = gtk::Stack::new();
        stack.add_named(&placeholder, Some("empty"));
        stack.add_named(&scroller, Some("article"));
        stack.add_named(&web_bin, Some("web"));

        let this = Rc::new(ArticleView {
            root: stack.clone().upcast(),
            web_toggle: web_toggle.clone(),
            title,
            meta,
            body,
            stack,
            web_bin,
            rendered,
            current_item: RefCell::new(None),
            web_state: RefCell::new(None),
        });

        web_toggle.connect_toggled(glib::clone!(
            #[weak]
            this,
            move |button| this.set_web_mode(button.is_active())
        ));
        this
    }

    pub fn clear(&self) {
        self.current_item.borrow_mut().take();
        self.web_toggle.set_active(false); // tears down WebKit if it was live
        self.web_toggle.set_sensitive(false);
        self.stack.set_visible_child_name("empty");
        self.rendered.borrow_mut().clear(&self.body.buffer());
    }

    pub fn show(&self, item: &Item, feed_title: &str) {
        self.title.set_text(&item.title);
        self.meta.set_text(&format_meta(item, feed_title));
        *self.current_item.borrow_mut() = Some(item.clone());
        self.web_toggle.set_sensitive(true);

        if let Some(web) = self.web_state.borrow().as_ref() {
            // Web mode stays live across article switches; just reload.
            load_article(&web.view, item);
            return;
        }
        let blocks = ir::html_to_blocks(
            item.content_html.as_deref().unwrap_or(""),
            item.link.as_deref(),
        );
        textview::render(&self.body, &blocks, &mut self.rendered.borrow_mut());
        self.stack.set_visible_child_name("article");
    }

    fn set_web_mode(&self, enabled: bool) {
        if enabled {
            if self.web_state.borrow().is_some() {
                return;
            }
            let Some(item) = self.current_item.borrow().clone() else {
                return;
            };
            // Ephemeral session: no disk state, and ours alone — dropping it
            // is what lets the WebKitNetworkProcess exit on toggle-off.
            let session = webkit6::NetworkSession::new_ephemeral();
            let view = webkit6::WebView::builder()
                .network_session(&session)
                .vexpand(true)
                .build();
            // Keep it a renderer, not a browser: user-initiated navigation
            // goes to the external browser.
            view.connect_decide_policy(|_, decision, decision_type| {
                if decision_type != webkit6::PolicyDecisionType::NavigationAction {
                    return false;
                }
                let Some(nav) = decision
                    .downcast_ref::<webkit6::NavigationPolicyDecision>()
                    .and_then(|d| d.navigation_action())
                else {
                    return false;
                };
                let mut nav = nav;
                if !nav.is_user_gesture() {
                    return false;
                }
                if let Some(uri) = nav.request().and_then(|r| r.uri()) {
                    decision.ignore();
                    gtk::UriLauncher::new(&uri).launch(
                        None::<&gtk::Window>,
                        None::<&gtk::gio::Cancellable>,
                        |result| {
                            if let Err(e) = result {
                                log::warn!("could not open link: {e}");
                            }
                        },
                    );
                    return true;
                }
                false
            });
            load_article(&view, &item);
            self.web_bin.set_child(Some(&view));
            self.stack.set_visible_child_name("web");
            *self.web_state.borrow_mut() = Some(WebState {
                view,
                _session: session,
            });
        } else {
            let Some(web) = self.web_state.borrow_mut().take() else {
                return;
            };
            // Hard-kill the web process rather than waiting on finalization,
            // then drop our refs (WebState) so the session dies with it.
            web.view.terminate_web_process();
            self.web_bin.set_child(gtk::Widget::NONE);
            drop(web);

            let has_item = self.current_item.borrow().is_some();
            if has_item {
                // Re-render natively (buffer was untouched only if the item
                // never changed while in web mode).
                let item = self.current_item.borrow().clone().unwrap();
                let blocks = ir::html_to_blocks(
                    item.content_html.as_deref().unwrap_or(""),
                    item.link.as_deref(),
                );
                textview::render(&self.body, &blocks, &mut self.rendered.borrow_mut());
                self.stack.set_visible_child_name("article");
            } else {
                self.stack.set_visible_child_name("empty");
            }
        }
    }
}

fn load_article(view: &webkit6::WebView, item: &Item) {
    let html = format!(
        "<html><head><meta charset=\"utf-8\">\
         <style>body{{max-width:42em;margin:1em auto;padding:0 1em;\
         font-family:sans-serif;line-height:1.5}}img{{max-width:100%;height:auto}}</style>\
         </head><body><h2>{}</h2>{}</body></html>",
        glib::markup_escape_text(&item.title),
        item.content_html.as_deref().unwrap_or("")
    );
    view.load_html(&html, item.link.as_deref());
}

fn attach_link_handlers(view: &gtk::TextView, rendered: &Rc<RefCell<textview::Rendered>>) {
    let link_at =
        |view: &gtk::TextView, rendered: &Rc<RefCell<textview::Rendered>>, x: f64, y: f64| {
            let (bx, by) =
                view.window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
            let iter = view.iter_at_location(bx, by)?;
            let rendered = rendered.borrow();
            iter.tags().iter().find_map(|tag| {
                rendered
                    .links
                    .iter()
                    .find(|(t, _)| t == tag)
                    .map(|(_, url)| url.clone())
            })
        };

    let click = gtk::GestureClick::new();
    click.connect_released(glib::clone!(
        #[weak]
        view,
        #[strong]
        rendered,
        move |_, n, x, y| {
            if n != 1 {
                return;
            }
            if let Some(url) = link_at(&view, &rendered, x, y) {
                gtk::UriLauncher::new(&url).launch(
                    None::<&gtk::Window>,
                    None::<&gtk::gio::Cancellable>,
                    |result| {
                        if let Err(e) = result {
                            log::warn!("could not open link: {e}");
                        }
                    },
                );
            }
        }
    ));
    view.add_controller(click);

    let motion = gtk::EventControllerMotion::new();
    motion.connect_motion(glib::clone!(
        #[weak]
        view,
        #[strong]
        rendered,
        move |_, x, y| {
            let over_link = link_at(&view, &rendered, x, y).is_some();
            view.set_cursor_from_name(Some(if over_link { "pointer" } else { "text" }));
        }
    ));
    view.add_controller(motion);
}

fn format_meta(item: &Item, feed_title: &str) -> String {
    let mut parts = vec![feed_title.to_string()];
    if let Some(author) = &item.author {
        parts.push(author.clone());
    }
    if let Some(ts) = item.published_at {
        if let Some(dt) = chrono::DateTime::from_timestamp(ts, 0) {
            parts.push(dt.format("%b %e, %Y %H:%M").to_string());
        }
    }
    parts.join("  ·  ")
}
