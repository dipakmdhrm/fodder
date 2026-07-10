//! Article pane. Phase 3 placeholder: plain-text rendering of the HTML body.
//! Phase 4 replaces this with the sanitized native renderer; Phase 5 adds the
//! WebKit toggle.

use gtk4 as gtk;

use gtk::prelude::*;

use fodder_core::models::Item;

pub struct ArticleView {
    pub root: gtk::Widget,
    title: gtk::Label,
    meta: gtk::Label,
    body: gtk::TextView,
    placeholder: gtk::Label,
    stack: gtk::Stack,
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
            .left_margin(2)
            .right_margin(2)
            .top_margin(12)
            .build();

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
            placeholder,
            stack,
        }
    }

    pub fn clear(&self) {
        self.stack.set_visible_child_name("empty");
        self.placeholder.set_visible(true);
    }

    pub fn show(&self, item: &Item, feed_title: &str) {
        self.title.set_text(&item.title);
        self.meta.set_text(&format_meta(item, feed_title));
        let text = item
            .content_html
            .as_deref()
            .map(strip_tags)
            .unwrap_or_default();
        self.body.buffer().set_text(text.trim());
        self.stack.set_visible_child_name("article");
    }
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

/// Crude placeholder until the Phase 4 renderer lands.
fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}
