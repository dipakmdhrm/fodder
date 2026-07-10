//! IR → GtkTextView. Thin: all structure decisions were made in `ir`.

use gtk4 as gtk;

use gtk::glib;
use gtk::pango;
use gtk::prelude::*;

use super::ir::{Block, Span, SpanStyle};

const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_IMAGE_WIDTH: i32 = 560;

/// Anonymous tags created for one render, removed before the next.
pub struct Rendered {
    tags: Vec<gtk::TextTag>,
    /// (tag, target url) pairs for click hit-testing.
    pub links: Vec<(gtk::TextTag, String)>,
}

impl Rendered {
    pub fn empty() -> Self {
        Rendered {
            tags: Vec::new(),
            links: Vec::new(),
        }
    }

    pub fn clear(&mut self, buffer: &gtk::TextBuffer) {
        buffer.set_text("");
        let table = buffer.tag_table();
        for tag in self.tags.drain(..) {
            table.remove(&tag);
        }
        self.links.clear();
    }
}

struct TagSet {
    heading: [gtk::TextTag; 3],
    bold: gtk::TextTag,
    italic: gtk::TextTag,
    code: gtk::TextTag,
    code_block: gtk::TextTag,
    quote: gtk::TextTag,
    strike: gtk::TextTag,
    list_indent: Vec<gtk::TextTag>,
}

pub fn render(view: &gtk::TextView, blocks: &[Block], rendered: &mut Rendered) {
    let buffer = view.buffer();
    rendered.clear(&buffer);

    let tags = make_tags(&buffer, rendered, blocks);
    let mut first = true;
    for block in blocks {
        if !first {
            buffer.insert(&mut buffer.end_iter(), "\n");
        }
        first = false;
        match block {
            Block::Paragraph(spans) => insert_spans(&buffer, rendered, &tags, spans, &[]),
            Block::Heading(level, spans) => {
                let tag = &tags.heading[(level.clamp(&1, &3) - 1) as usize];
                insert_spans(&buffer, rendered, &tags, spans, std::slice::from_ref(tag));
            }
            Block::Quote(spans) => insert_spans(
                &buffer,
                rendered,
                &tags,
                spans,
                std::slice::from_ref(&tags.quote),
            ),
            Block::ListItem {
                depth,
                marker,
                spans,
            } => {
                let indent =
                    tags.list_indent[(*depth as usize).min(tags.list_indent.len() - 1)].clone();
                buffer.insert_with_tags(&mut buffer.end_iter(), marker, &[&indent]);
                insert_spans(&buffer, rendered, &tags, spans, &[indent]);
            }
            Block::Code(code) => {
                buffer.insert_with_tags(&mut buffer.end_iter(), code, &[&tags.code_block]);
            }
            Block::Image { url, alt } => insert_image(view, &buffer, url, alt),
            Block::Rule => {
                buffer.insert_with_tags(&mut buffer.end_iter(), "⸻", &[&tags.quote]);
            }
        }
    }
}

fn make_tags(buffer: &gtk::TextBuffer, rendered: &mut Rendered, blocks: &[Block]) -> TagSet {
    let table = buffer.tag_table();
    let mut new_tag = |setup: &dyn Fn(&gtk::TextTag)| {
        let tag = gtk::TextTag::new(None);
        setup(&tag);
        table.add(&tag);
        rendered.tags.push(tag.clone());
        tag
    };

    let heading_scale = [1.6, 1.35, 1.15];
    let heading = std::array::from_fn(|i| {
        new_tag(&|t: &gtk::TextTag| {
            t.set_weight(700);
            t.set_scale(heading_scale[i]);
            t.set_pixels_above_lines(10);
        })
    });
    let max_depth = blocks
        .iter()
        .filter_map(|b| match b {
            Block::ListItem { depth, .. } => Some(*depth as usize),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    let list_indent = (0..=max_depth)
        .map(|d| {
            new_tag(&move |t: &gtk::TextTag| {
                t.set_left_margin(16 + 20 * d as i32);
            })
        })
        .collect();

    TagSet {
        heading,
        bold: new_tag(&|t| t.set_weight(700)),
        italic: new_tag(&|t| t.set_style(pango::Style::Italic)),
        code: new_tag(&|t| {
            t.set_family(Some("monospace"));
            t.set_scale(0.9);
        }),
        code_block: new_tag(&|t| {
            t.set_family(Some("monospace"));
            t.set_scale(0.9);
            t.set_left_margin(16);
            t.set_pixels_above_lines(6);
            t.set_pixels_below_lines(6);
        }),
        quote: new_tag(&|t| {
            t.set_left_margin(20);
            t.set_style(pango::Style::Italic);
        }),
        strike: new_tag(&|t| t.set_strikethrough(true)),
        list_indent,
    }
}

fn insert_spans(
    buffer: &gtk::TextBuffer,
    rendered: &mut Rendered,
    tags: &TagSet,
    spans: &[Span],
    extra: &[gtk::TextTag],
) {
    for span in spans {
        let mut applied: Vec<gtk::TextTag> = extra.to_vec();
        let SpanStyle {
            bold,
            italic,
            code,
            strikethrough,
            link,
        } = &span.style;
        if *bold {
            applied.push(tags.bold.clone());
        }
        if *italic {
            applied.push(tags.italic.clone());
        }
        if *code {
            applied.push(tags.code.clone());
        }
        if *strikethrough {
            applied.push(tags.strike.clone());
        }
        if let Some(url) = link {
            let tag = gtk::TextTag::new(None);
            tag.set_underline(pango::Underline::Single);
            tag.set_foreground(Some("#1c71d8"));
            buffer.tag_table().add(&tag);
            rendered.tags.push(tag.clone());
            rendered.links.push((tag.clone(), url.clone()));
            applied.push(tag);
        }
        let refs: Vec<&gtk::TextTag> = applied.iter().collect();
        buffer.insert_with_tags(&mut buffer.end_iter(), &span.text, &refs);
    }
}

fn insert_image(view: &gtk::TextView, buffer: &gtk::TextBuffer, url: &str, alt: &str) {
    let anchor = buffer.create_child_anchor(&mut buffer.end_iter());
    let picture = gtk::Picture::builder()
        .halign(gtk::Align::Start)
        .can_shrink(true)
        .build();
    if !alt.is_empty() {
        picture.set_tooltip_text(Some(alt));
    }
    view.add_child_at_anchor(&picture, &anchor);
    load_image_async(url.to_string(), picture);
}

/// Cache-then-network image loading; decode and sizing on the main loop.
fn load_image_async(url: String, picture: gtk::Picture) {
    let (tx, rx) = async_channel::bounded::<Vec<u8>>(1);
    std::thread::spawn(move || {
        let cache_path = fodder_core::paths::image_cache_dir().join(cache_key(&url));
        let bytes = match std::fs::read(&cache_path) {
            Ok(bytes) => Some(bytes),
            Err(_) => match fodder_core::fetch::fetch_bytes(&url, MAX_IMAGE_BYTES) {
                Ok(bytes) => {
                    if let Some(dir) = cache_path.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    let _ = std::fs::write(&cache_path, &bytes);
                    Some(bytes)
                }
                Err(e) => {
                    log::debug!("image fetch failed for {url}: {e}");
                    None
                }
            },
        };
        if let Some(bytes) = bytes {
            let _ = tx.send_blocking(bytes);
        }
    });
    glib::spawn_future_local(async move {
        let Ok(bytes) = rx.recv().await else { return };
        match gtk::gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes)) {
            Ok(texture) => {
                let (w, h) = (texture.width(), texture.height());
                if w > MAX_IMAGE_WIDTH {
                    picture.set_size_request(MAX_IMAGE_WIDTH, h * MAX_IMAGE_WIDTH / w.max(1));
                } else {
                    picture.set_size_request(w, h);
                }
                picture.set_paintable(Some(&texture));
            }
            Err(e) => log::debug!("undecodable image: {e}"),
        }
    });
}

fn cache_key(url: &str) -> String {
    // FNV-1a; just a cache filename, not security-relevant.
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in url.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}
