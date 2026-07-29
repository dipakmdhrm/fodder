//! The sanitized light reader: article HTML → safe Pango markup in a GtkLabel.
//!
//! Content is first sanitized with `ammonia` (defence in depth — scripts and
//! dangerous attributes are stripped), then a small whitelist walker converts
//! the safe DOM into the limited set of tags Pango understands. There is no
//! script execution and no network fetch: this is a text renderer, not a
//! browser. The full WebKit renderer (M5) is a separate mode.

use ego_tree::NodeRef;
use gtk4 as gtk;
use gtk4::glib;
use gtk4::prelude::*;
use scraper::node::Node;
use scraper::Html;

/// Convert article HTML into Pango markup safe to hand to `GtkLabel`.
pub fn html_to_pango(html: &str) -> String {
    let clean = ammonia::clean(html);
    let doc = Html::parse_fragment(&clean);

    let mut out = String::new();
    for child in doc.tree.root().children() {
        render(child, &mut out);
    }

    collapse_blank_lines(out.trim())
}

/// Recursively render a DOM node into Pango markup.
fn render(node: NodeRef<Node>, out: &mut String) {
    match node.value() {
        Node::Text(text) => out.push_str(&escape(text)),
        Node::Element(el) => {
            let name = el.name();
            match name {
                "br" => out.push('\n'),
                "p" | "div" | "section" | "article" => {
                    render_children(node, out);
                    out.push_str("\n\n");
                }
                "b" | "strong" => wrap(node, out, "<b>", "</b>"),
                "i" | "em" => wrap(node, out, "<i>", "</i>"),
                "u" => wrap(node, out, "<u>", "</u>"),
                "s" | "del" | "strike" => wrap(node, out, "<s>", "</s>"),
                "code" | "tt" | "kbd" | "samp" => wrap(node, out, "<tt>", "</tt>"),
                "pre" => {
                    out.push_str("<tt>");
                    render_children(node, out);
                    out.push_str("</tt>\n\n");
                }
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    out.push_str("<big><b>");
                    render_children(node, out);
                    out.push_str("</b></big>\n\n");
                }
                "li" => {
                    out.push_str("• ");
                    render_children(node, out);
                    out.push('\n');
                }
                "ul" | "ol" => {
                    render_children(node, out);
                    out.push('\n');
                }
                "blockquote" => {
                    render_children(node, out);
                    out.push_str("\n\n");
                }
                "a" => {
                    if let Some(href) = el.attr("href") {
                        out.push_str(&format!("<a href=\"{}\">", escape(href)));
                        render_children(node, out);
                        out.push_str("</a>");
                    } else {
                        render_children(node, out);
                    }
                }
                "img" => {
                    if let Some(alt) = el.attr("alt") {
                        if !alt.is_empty() {
                            out.push_str(&format!("[{}]", escape(alt)));
                        }
                    }
                }
                // Unknown/unsupported elements: keep their text content.
                _ => render_children(node, out),
            }
        }
        _ => {}
    }
}

fn render_children(node: NodeRef<Node>, out: &mut String) {
    for child in node.children() {
        render(child, out);
    }
}

fn wrap(node: NodeRef<Node>, out: &mut String, open: &str, close: &str) {
    out.push_str(open);
    render_children(node, out);
    out.push_str(close);
}

/// Escape text for Pango markup (handles `& < > ' "`).
fn escape(text: &str) -> String {
    glib::markup_escape_text(text).to_string()
}

/// Collapse runs of 3+ newlines down to a paragraph break.
fn collapse_blank_lines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut newlines = 0;
    for ch in s.chars() {
        if ch == '\n' {
            newlines += 1;
            if newlines <= 2 {
                out.push(ch);
            }
        } else {
            newlines = 0;
            out.push(ch);
        }
    }
    out
}

/// A GtkLabel configured to render article markup, wrapped in a scroller.
pub fn body_label() -> gtk::Label {
    let label = gtk::Label::new(None);
    label.set_use_markup(true);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_xalign(0.0);
    label.set_yalign(0.0);
    label.set_selectable(true);
    label.set_vexpand(true);
    label
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_scripts_and_keeps_text() {
        let out = html_to_pango("<p>Hello <script>alert(1)</script>world</p>");
        assert!(out.contains("Hello"));
        assert!(out.contains("world"));
        assert!(!out.contains("alert"));
    }

    #[test]
    fn converts_basic_formatting() {
        let out = html_to_pango("<p><b>bold</b> and <i>italic</i></p>");
        assert!(out.contains("<b>bold</b>"));
        assert!(out.contains("<i>italic</i>"));
    }

    #[test]
    fn escapes_text_entities() {
        let out = html_to_pango("<p>a &lt; b &amp; c</p>");
        assert!(out.contains("a &lt; b &amp; c"));
    }

    #[test]
    fn keeps_safe_links() {
        let out = html_to_pango(r#"<a href="https://example.com">link</a>"#);
        assert!(out.contains(r#"<a href="https://example.com">link</a>"#));
    }
}
