//! Article pane: native sanitized-HTML rendering (default). The WebKit
//! toggle (Phase 5) mounts alongside this in a stack.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4 as gtk;

use gtk::glib;
use gtk::prelude::*;

use fodder_core::models::Item;

use crate::render::{ir, textview};

pub struct ArticleView {
    pub root: gtk::Widget,
    title: gtk::Label,
    meta: gtk::Label,
    body: gtk::TextView,
    stack: gtk::Stack,
    rendered: Rc<RefCell<textview::Rendered>>,
}

impl ArticleView {
    pub fn new() -> Self {
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

        let stack = gtk::Stack::new();
        stack.add_named(&placeholder, Some("empty"));
        stack.add_named(&scroller, Some("article"));

        ArticleView {
            root: stack.clone().upcast(),
            title,
            meta,
            body,
            stack,
            rendered,
        }
    }

    pub fn clear(&self) {
        self.stack.set_visible_child_name("empty");
        self.rendered.borrow_mut().clear(&self.body.buffer());
    }

    pub fn show(&self, item: &Item, feed_title: &str) {
        self.title.set_text(&item.title);
        self.meta.set_text(&format_meta(item, feed_title));
        let blocks = ir::html_to_blocks(
            item.content_html.as_deref().unwrap_or(""),
            item.link.as_deref(),
        );
        textview::render(&self.body, &blocks, &mut self.rendered.borrow_mut());
        self.stack.set_visible_child_name("article");
    }
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
