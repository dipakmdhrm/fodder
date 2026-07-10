//! HTML → render IR. Pure (no GTK), so it is fully unit-testable.
//!
//! Pipeline: ammonia sanitizes (strips scripts/handlers, resolves relative
//! URLs against the article link), html5ever parses, and a walker flattens
//! the DOM into a list of styled blocks the TextView layer can consume.

use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData, RcDom};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SpanStyle {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strikethrough: bool,
    pub link: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub style: SpanStyle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Paragraph(Vec<Span>),
    Heading(u8, Vec<Span>),
    Quote(Vec<Span>),
    ListItem {
        depth: u8,
        marker: String,
        spans: Vec<Span>,
    },
    Code(String),
    Image {
        url: String,
        alt: String,
    },
    Rule,
}

pub fn html_to_blocks(html: &str, base_url: Option<&str>) -> Vec<Block> {
    let clean = sanitize(html, base_url);
    let dom = html5ever::parse_document(RcDom::default(), Default::default()).one(clean);
    let mut walker = Walker::default();
    walk(&dom.document, &mut walker);
    walker.flush();
    walker.blocks
}

fn sanitize(html: &str, base_url: Option<&str>) -> String {
    let mut builder = ammonia::Builder::default();
    if let Some(base) = base_url.and_then(|b| ammonia::Url::parse(b).ok()) {
        builder.url_relative(ammonia::UrlRelative::RewriteWithBase(base));
    }
    builder.clean(html).to_string()
}

#[derive(Default)]
struct Walker {
    blocks: Vec<Block>,
    spans: Vec<Span>,
    style: SpanStyle,
    quote_depth: u8,
    li_depth: u8,
    /// One entry per open list; `Some(n)` is an ordered list's counter.
    list_stack: Vec<Option<u32>>,
    /// Marker for the current list item, consumed by its first flush.
    li_marker: Option<String>,
    in_pre: bool,
    pre_text: String,
}

impl Walker {
    fn push_text(&mut self, text: &str) {
        if self.in_pre {
            self.pre_text.push_str(text);
            return;
        }
        let collapsed = collapse_whitespace(text);
        if collapsed.is_empty() {
            return;
        }
        // Avoid duplicated separators across text-node boundaries.
        if collapsed == " "
            && self
                .spans
                .last()
                .is_none_or(|s| s.text.ends_with([' ', '\n']))
        {
            return;
        }
        self.append_span(collapsed);
    }

    fn push_newline(&mut self) {
        if !self.spans.is_empty() {
            self.append_span("\n".to_string());
        }
    }

    fn append_span(&mut self, text: String) {
        match self.spans.last_mut() {
            Some(last) if last.style == self.style => last.text.push_str(&text),
            _ => self.spans.push(Span {
                text,
                style: self.style.clone(),
            }),
        }
    }

    /// Emit accumulated inline spans as a block chosen from the current context.
    fn flush(&mut self) {
        trim_edges(&mut self.spans);
        if self.spans.is_empty() {
            self.li_marker.take();
            return;
        }
        let spans = std::mem::take(&mut self.spans);
        let block = if self.li_depth > 0 {
            Block::ListItem {
                depth: self.li_depth - 1,
                marker: self.li_marker.take().unwrap_or_default(),
                spans,
            }
        } else if self.quote_depth > 0 {
            Block::Quote(spans)
        } else {
            Block::Paragraph(spans)
        };
        self.blocks.push(block);
    }

    fn flush_heading(&mut self, level: u8) {
        trim_edges(&mut self.spans);
        if !self.spans.is_empty() {
            let spans = std::mem::take(&mut self.spans);
            self.blocks.push(Block::Heading(level, spans));
        }
    }

    fn with_style(&mut self, node: &Handle, change: impl FnOnce(&mut SpanStyle)) {
        let saved = self.style.clone();
        change(&mut self.style);
        walk_children(node, self);
        self.style = saved;
    }
}

fn walk_children(node: &Handle, w: &mut Walker) {
    for child in node.children.borrow().iter() {
        walk(child, w);
    }
}

fn attr(node: &Handle, name: &str) -> Option<String> {
    if let NodeData::Element { attrs, .. } = &node.data {
        attrs
            .borrow()
            .iter()
            .find(|a| a.name.local.as_ref() == name)
            .map(|a| a.value.to_string())
    } else {
        None
    }
}

fn walk(node: &Handle, w: &mut Walker) {
    match &node.data {
        NodeData::Text { contents } => w.push_text(&contents.borrow()),
        NodeData::Element { name, .. } => {
            let tag = name.local.as_ref();
            match tag {
                "p" | "div" | "section" | "article" | "figure" | "figcaption" | "summary"
                | "details" | "tr" | "dt" | "dd" => {
                    w.flush();
                    walk_children(node, w);
                    w.flush();
                }
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    w.flush();
                    walk_children(node, w);
                    let level = tag.as_bytes()[1] - b'0';
                    w.flush_heading(level);
                }
                "strong" | "b" => w.with_style(node, |s| s.bold = true),
                "em" | "i" | "cite" | "var" => w.with_style(node, |s| s.italic = true),
                "code" | "kbd" | "samp" | "tt" if !w.in_pre => {
                    w.with_style(node, |s| s.code = true)
                }
                "s" | "del" | "strike" => w.with_style(node, |s| s.strikethrough = true),
                "a" => {
                    let href = attr(node, "href");
                    w.with_style(node, |s| s.link = href);
                }
                "br" => w.push_newline(),
                "ul" | "ol" => {
                    w.flush();
                    w.list_stack.push((tag == "ol").then_some(attr_start(node)));
                    walk_children(node, w);
                    w.list_stack.pop();
                    w.flush();
                }
                "li" => {
                    w.flush();
                    let marker = match w.list_stack.last_mut() {
                        Some(Some(counter)) => {
                            let m = format!("{counter}. ");
                            *counter += 1;
                            m
                        }
                        _ => "•  ".to_string(),
                    };
                    w.li_marker = Some(marker);
                    w.li_depth += 1;
                    walk_children(node, w);
                    w.flush();
                    w.li_depth -= 1;
                }
                "blockquote" => {
                    w.flush();
                    w.quote_depth += 1;
                    walk_children(node, w);
                    w.flush();
                    w.quote_depth -= 1;
                }
                "pre" => {
                    w.flush();
                    w.in_pre = true;
                    w.pre_text.clear();
                    walk_children(node, w);
                    w.in_pre = false;
                    let text = std::mem::take(&mut w.pre_text);
                    let text = text.trim_matches('\n');
                    if !text.is_empty() {
                        w.blocks.push(Block::Code(text.to_string()));
                    }
                }
                "img" => {
                    if let Some(url) = attr(node, "src") {
                        w.flush();
                        w.blocks.push(Block::Image {
                            url,
                            alt: attr(node, "alt").unwrap_or_default(),
                        });
                    }
                }
                "hr" => {
                    w.flush();
                    w.blocks.push(Block::Rule);
                }
                "td" | "th" => {
                    walk_children(node, w);
                    w.push_text(" ");
                }
                _ => walk_children(node, w),
            }
        }
        _ => walk_children(node, w),
    }
}

fn attr_start(node: &Handle) -> u32 {
    attr(node, "start")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1)
}

fn collapse_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last_ws = false;
    for c in text.chars() {
        if c.is_whitespace() {
            if !last_ws {
                out.push(' ');
            }
            last_ws = true;
        } else {
            out.push(c);
            last_ws = false;
        }
    }
    out
}

fn trim_edges(spans: &mut Vec<Span>) {
    if let Some(first) = spans.first_mut() {
        let trimmed = first.text.trim_start().to_string();
        first.text = trimmed;
    }
    if let Some(last) = spans.last_mut() {
        let trimmed = last.text.trim_end().to_string();
        last.text = trimmed;
    }
    spans.retain(|s| !s.text.is_empty());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(spans: &[Span]) -> String {
        spans.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn paragraphs_with_inline_styles() {
        let blocks = html_to_blocks("<p>Hello <b>bold</b> and <em>italic</em>.</p>", None);
        assert_eq!(blocks.len(), 1);
        let Block::Paragraph(spans) = &blocks[0] else {
            panic!("expected paragraph, got {:?}", blocks[0]);
        };
        assert_eq!(text_of(spans), "Hello bold and italic.");
        assert!(spans.iter().any(|s| s.style.bold && s.text == "bold"));
        assert!(spans.iter().any(|s| s.style.italic && s.text == "italic"));
    }

    #[test]
    fn headings_carry_level() {
        let blocks = html_to_blocks("<h2>Title</h2><p>body</p>", None);
        assert!(matches!(&blocks[0], Block::Heading(2, spans) if text_of(spans) == "Title"));
    }

    #[test]
    fn script_and_event_handlers_are_stripped() {
        let blocks = html_to_blocks(
            r#"<p onclick="evil()">ok</p><script>alert(1)</script><style>p{}</style>"#,
            None,
        );
        assert_eq!(blocks.len(), 1);
        let Block::Paragraph(spans) = &blocks[0] else {
            panic!("expected paragraph");
        };
        assert_eq!(text_of(spans), "ok");
    }

    #[test]
    fn relative_urls_resolved_against_base() {
        let blocks = html_to_blocks(
            r#"<p><a href="/page">go</a></p><img src="pic.png" alt="a pic">"#,
            Some("https://example.org/posts/1"),
        );
        let Block::Paragraph(spans) = &blocks[0] else {
            panic!("expected paragraph");
        };
        assert_eq!(
            spans[0].style.link.as_deref(),
            Some("https://example.org/page")
        );
        assert!(matches!(
            &blocks[1],
            Block::Image { url, alt } if url == "https://example.org/posts/pic.png" && alt == "a pic"
        ));
    }

    #[test]
    fn nested_lists_track_depth_and_markers() {
        let html =
            "<ul><li>one<ul><li>inner</li></ul></li></ul><ol><li>first</li><li>second</li></ol>";
        let blocks = html_to_blocks(html, None);
        let items: Vec<_> = blocks
            .iter()
            .filter_map(|b| match b {
                Block::ListItem {
                    depth,
                    marker,
                    spans,
                } => Some((*depth, marker.as_str(), text_of(spans))),
                _ => None,
            })
            .collect();
        assert_eq!(
            items,
            vec![
                (0, "•  ", "one".to_string()),
                (1, "•  ", "inner".to_string()),
                (0, "1. ", "first".to_string()),
                (0, "2. ", "second".to_string()),
            ]
        );
    }

    #[test]
    fn pre_keeps_verbatim_text() {
        let blocks = html_to_blocks("<pre>let x = 1;\n  let y = 2;</pre>", None);
        assert!(matches!(
            &blocks[0],
            Block::Code(code) if code == "let x = 1;\n  let y = 2;"
        ));
    }

    #[test]
    fn blockquote_becomes_quote() {
        let blocks = html_to_blocks("<blockquote><p>wise words</p></blockquote>", None);
        assert!(matches!(&blocks[0], Block::Quote(spans) if text_of(spans) == "wise words"));
    }

    #[test]
    fn entities_and_whitespace_collapse() {
        let blocks = html_to_blocks("<p>a &amp;\n\n   b</p>", None);
        let Block::Paragraph(spans) = &blocks[0] else {
            panic!("expected paragraph");
        };
        assert_eq!(text_of(spans), "a & b");
    }

    #[test]
    fn br_becomes_newline() {
        let blocks = html_to_blocks("<p>line one<br>line two</p>", None);
        let Block::Paragraph(spans) = &blocks[0] else {
            panic!("expected paragraph");
        };
        assert_eq!(text_of(spans), "line one\nline two");
    }
}
