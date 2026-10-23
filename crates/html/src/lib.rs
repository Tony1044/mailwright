//! HTML mail, made safe and simple to draw.
//!
//! [`parse`] turns a message body into [`Block`]s of styled [`Inline`] runs: paragraphs,
//! headings, list items, quotes, rules, preformatted text, tables and images. Scripts, styles,
//! forms and hidden elements are dropped; links are kept as text with a target (MailCraft shows
//! where a link goes before opening it); images are `cid:` (attached), `data:` or remote, and
//! remote ones are only fetched when the user asks, as in Outlook.
//!
//! [`to_text`] gives the plain text (for quoting and previews) and [`text_to_html`] goes the
//! other way for what the user types.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use base64::Engine as _;
use scraper::{Html, Node};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub mono: bool,
    pub small: bool,
    /// Text colour.
    pub color: Option<[u8; 3]>,
    pub background: Option<[u8; 3]>,
    /// Font size in points, when the mail sets one.
    pub size: Option<f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ImageSource {
    /// An attachment, by content id.
    Cid(String),
    /// Embedded in the HTML.
    Data { mime: String, bytes: Vec<u8> },
    /// On the web: shown only when the user allows it.
    Remote(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub source: ImageSource,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub alt: String,
    pub link: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Inline {
    Text { text: String, style: Style, link: Option<String> },
    Image(Image),
    Break,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub blocks: Vec<Block>,
    pub header: bool,
    pub background: Option<[u8; 3]>,
    pub colspan: usize,
    /// Width hint: points, or a fraction of the table when negative (−0.5 = 50%).
    pub width: Option<f32>,
    pub align: Align,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Para { inlines: Vec<Inline>, align: Align, background: Option<[u8; 3]> },
    Heading { level: u8, inlines: Vec<Inline>, align: Align },
    /// `marker` is "•", "1." and so on; `depth` starts at 0.
    ListItem { marker: String, depth: u8, inlines: Vec<Inline> },
    Quote(Vec<Block>),
    Pre(String),
    Rule,
    Table { rows: Vec<Vec<Cell>>, border: bool, width: Option<f32> },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Document {
    pub blocks: Vec<Block>,
    /// Remote images in the message (not fetched unless allowed).
    pub remote_images: usize,
}

impl Document {
    /// Every image, in order.
    pub fn images(&self) -> Vec<&Image> {
        fn walk<'a>(blocks: &'a [Block], out: &mut Vec<&'a Image>) {
            for b in blocks {
                match b {
                    Block::Para { inlines, .. } | Block::Heading { inlines, .. } | Block::ListItem { inlines, .. } => {
                        for i in inlines {
                            if let Inline::Image(img) = i {
                                out.push(img);
                            }
                        }
                    }
                    Block::Quote(inner) => walk(inner, out),
                    Block::Table { rows, .. } => {
                        for r in rows {
                            for c in r {
                                walk(&c.blocks, out);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.blocks, &mut out);
        out
    }
}

/// Parse a colour: `#rgb`, `#rrggbb`, `rgb(r,g,b)` or a common name.
pub fn parse_color(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().trim_end_matches("!important").trim().to_ascii_lowercase();
    if let Some(hex) = s.strip_prefix('#') {
        let h = |i: usize, n: usize| u8::from_str_radix(hex.get(i..i + n)?, 16).ok();
        return match hex.len() {
            3 => Some([h(0, 1)? * 17, h(1, 1)? * 17, h(2, 1)? * 17]),
            6 | 8 => Some([h(0, 2)?, h(2, 2)?, h(4, 2)?]),
            _ => None,
        };
    }
    if let Some(inner) = s.strip_prefix("rgb(").or_else(|| s.strip_prefix("rgba(")) {
        let parts: Vec<u8> = inner.trim_end_matches(')').split(',').take(3).filter_map(|p| p.trim().parse::<f32>().ok()).map(|v| v.clamp(0.0, 255.0) as u8).collect();
        return (parts.len() == 3).then(|| [parts[0], parts[1], parts[2]]);
    }
    Some(match s.as_str() {
        "black" => [0, 0, 0],
        "white" => [255, 255, 255],
        "red" => [255, 0, 0],
        "green" => [0, 128, 0],
        "blue" => [0, 0, 255],
        "navy" => [0, 0, 128],
        "gray" | "grey" => [128, 128, 128],
        "silver" => [192, 192, 192],
        "maroon" => [128, 0, 0],
        "purple" => [128, 0, 128],
        "teal" => [0, 128, 128],
        "orange" => [255, 165, 0],
        "darkblue" => [0, 0, 139],
        "darkred" => [139, 0, 0],
        "darkgreen" => [0, 100, 0],
        "darkgray" | "darkgrey" => [169, 169, 169],
        "lightgray" | "lightgrey" => [211, 211, 211],
        "whitesmoke" => [245, 245, 245],
        _ => return None,
    })
}

fn parse_length(s: &str) -> Option<f32> {
    let s = s.trim().to_ascii_lowercase();
    if let Some(v) = s.strip_suffix("pt") {
        return v.trim().parse().ok();
    }
    if let Some(v) = s.strip_suffix("px") {
        return v.trim().parse::<f32>().ok().map(|px| px * 0.75);
    }
    if let Some(v) = s.strip_suffix("em").or_else(|| s.strip_suffix("rem")) {
        return v.trim().parse::<f32>().ok().map(|em| em * 11.0);
    }
    if let Some(v) = s.strip_suffix('%') {
        return v.trim().parse::<f32>().ok().map(|p| p / 100.0 * 11.0);
    }
    match s.as_str() {
        "xx-small" => Some(7.0),
        "x-small" => Some(7.5),
        "small" => Some(10.0),
        "medium" => Some(12.0),
        "large" => Some(13.5),
        "x-large" => Some(18.0),
        "xx-large" => Some(24.0),
        _ => s.parse::<f32>().ok().map(|px| px * 0.75),
    }
}

/// A width attribute or CSS width: points, or a negative fraction for percentages.
fn parse_width(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(p) = s.strip_suffix('%') {
        return p.trim().parse::<f32>().ok().map(|p| -(p / 100.0).clamp(0.0, 1.0));
    }
    let v = s.trim_end_matches("px").trim().parse::<f32>().ok()?;
    Some(v * 0.75)
}

#[derive(Default)]
struct Css {
    hidden: bool,
    color: Option<[u8; 3]>,
    background: Option<[u8; 3]>,
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    size: Option<f32>,
    mono: bool,
    align: Option<Align>,
    width: Option<f32>,
}

fn parse_style(s: &str) -> Css {
    let mut c = Css::default();
    for decl in s.split(';') {
        let Some((k, v)) = decl.split_once(':') else { continue };
        let k = k.trim().to_ascii_lowercase();
        let v = v.trim();
        let lv = v.to_ascii_lowercase();
        match k.as_str() {
            "display" if lv.starts_with("none") => c.hidden = true,
            "visibility" if lv.starts_with("hidden") => c.hidden = true,
            "max-height" | "height" if lv.trim_end_matches("!important").trim() == "0" || lv.starts_with("0px") => c.hidden = true,
            "mso-hide" if lv.starts_with("all") => c.hidden = true,
            "color" => c.color = parse_color(v),
            "background-color" | "background" => c.background = v.split_whitespace().find_map(parse_color),
            "font-weight" => c.bold = Some(lv.starts_with("bold") || lv.parse::<u32>().is_ok_and(|w| w >= 600)),
            "font-style" => c.italic = Some(lv.starts_with("italic") || lv.starts_with("oblique")),
            "text-decoration" | "text-decoration-line" => {
                c.underline = Some(lv.contains("underline"));
                c.strike = Some(lv.contains("line-through"));
            }
            "font-size" => c.size = parse_length(v),
            "font-family" => c.mono = lv.contains("mono") || lv.contains("courier") || lv.contains("consolas"),
            "text-align" => {
                c.align = Some(if lv.starts_with("center") {
                    Align::Center
                } else if lv.starts_with("right") {
                    Align::Right
                } else {
                    Align::Left
                })
            }
            "width" => c.width = parse_width(&lv),
            _ => {}
        }
    }
    c
}

struct Ctx {
    style: Style,
    link: Option<String>,
    align: Align,
    pre: bool,
}

struct Builder {
    blocks: Vec<Block>,
    inlines: Vec<Inline>,
    /// The current paragraph's alignment.
    align: Align,
    /// Pending list item marker for the paragraph being built.
    list_item: Option<(String, u8)>,
    heading: Option<u8>,
    space_pending: bool,
    remote: usize,
    list_depth: u8,
}

impl Builder {
    fn new() -> Builder {
        Builder { blocks: Vec::new(), inlines: Vec::new(), align: Align::Left, list_item: None, heading: None, space_pending: false, remote: 0, list_depth: 0 }
    }

    fn has_content(&self) -> bool {
        self.inlines.iter().any(|i| match i {
            Inline::Text { text, .. } => !text.trim().is_empty(),
            Inline::Image(_) => true,
            Inline::Break => false,
        })
    }

    /// End the current paragraph.
    fn flush(&mut self) {
        // Drop trailing breaks and spaces.
        while matches!(self.inlines.last(), Some(Inline::Break)) {
            self.inlines.pop();
        }
        if let Some(Inline::Text { text, .. }) = self.inlines.last_mut() {
            let t = text.trim_end().to_string();
            *text = t;
        }
        let inlines = std::mem::take(&mut self.inlines);
        let had = inlines.iter().any(|i| !matches!(i, Inline::Text { text, .. } if text.trim().is_empty()) && !matches!(i, Inline::Break));
        if had {
            if let Some((marker, depth)) = self.list_item.take() {
                self.blocks.push(Block::ListItem { marker, depth, inlines });
            } else if let Some(level) = self.heading {
                self.blocks.push(Block::Heading { level, inlines, align: self.align });
            } else {
                self.blocks.push(Block::Para { inlines, align: self.align, background: None });
            }
        }
        self.space_pending = false;
    }

    fn text(&mut self, raw: &str, ctx: &Ctx) {
        let text = if ctx.pre {
            raw.to_string()
        } else {
            // Collapse whitespace, carrying one space across element boundaries.
            let mut out = String::with_capacity(raw.len());
            let at_start = !self.has_content() && !matches!(self.inlines.last(), Some(Inline::Image(_)));
            for ch in raw.chars() {
                if ch.is_whitespace() && ch != '\u{a0}' {
                    self.space_pending = true;
                } else {
                    if self.space_pending && !(at_start && out.is_empty()) && !matches!(self.inlines.last(), Some(Inline::Break) if out.is_empty()) {
                        out.push(' ');
                    }
                    self.space_pending = false;
                    out.push(if ch == '\u{a0}' { ' ' } else { ch });
                }
            }
            out
        };
        if text.is_empty() {
            return;
        }
        let mut text = text;
        // A space starting a link or styled run belongs to the text before it.
        if !ctx.pre && text.starts_with(' ')
            && let Some(Inline::Text { text: t, style, link }) = self.inlines.last_mut()
                && (*style != ctx.style || *link != ctx.link) {
                    t.push(' ');
                    text.remove(0);
                    if text.is_empty() {
                        return;
                    }
                }
        if let Some(Inline::Text { text: t, style, link }) = self.inlines.last_mut()
            && *style == ctx.style && *link == ctx.link {
                t.push_str(&text);
                return;
            }
        self.inlines.push(Inline::Text { text, style: ctx.style, link: ctx.link.clone() });
    }
}

fn image_from(el: &scraper::node::Element, link: Option<String>, remote: &mut usize) -> Option<Image> {
    let src = el.attr("src")?.trim();
    let lower = src.to_ascii_lowercase();
    let source = if let Some(cid) = lower.strip_prefix("cid:").map(|_| &src[4..]) {
        ImageSource::Cid(cid.trim_matches(['<', '>']).to_string())
    } else if lower.starts_with("data:") {
        let (head, data) = src[5..].split_once(',')?;
        let mime = head.split(';').next().unwrap_or("image/png").to_string();
        if !head.contains("base64") {
            return None;
        }
        let bytes = base64::engine::general_purpose::STANDARD.decode(data.trim().as_bytes()).ok()?;
        ImageSource::Data { mime, bytes }
    } else if lower.starts_with("http://") || lower.starts_with("https://") {
        ImageSource::Remote(src.to_string())
    } else {
        return None;
    };
    let css = el.attr("style").map(parse_style).unwrap_or_default();
    let dim = |a: &str| el.attr(a).and_then(|v| v.trim().trim_end_matches("px").parse::<f32>().ok());
    let width = dim("width").or(css.width.filter(|w| *w > 0.0).map(|w| w / 0.75));
    let height = dim("height");
    // Tracking pixels.
    if matches!((width, height), (Some(w), Some(h)) if w <= 2.0 && h <= 2.0) {
        return None;
    }
    if css.hidden {
        return None;
    }
    if matches!(source, ImageSource::Remote(_)) {
        *remote += 1;
    }
    Some(Image { source, width, height, alt: el.attr("alt").unwrap_or("").to_string(), link })
}

fn safe_link(href: &str) -> Option<String> {
    let h = href.trim();
    let lower = h.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:") || lower.starts_with("tel:") {
        Some(h.to_string())
    } else {
        None
    }
}

const BLOCK_TAGS: &[&str] = &[
    "p", "div", "section", "article", "header", "footer", "main", "nav", "aside", "address", "center", "form", "fieldset", "figure", "figcaption", "dl", "dt", "dd", "body", "html",
];

fn walk(node: ego_tree_node::NodeRef<'_>, ctx: &Ctx, b: &mut Builder) {
    for child in node.children() {
        match child.value() {
            Node::Text(t) => b.text(t, ctx),
            Node::Element(el) => element(child, el, ctx, b),
            _ => {}
        }
    }
}

/// `ego_tree` comes with scraper; name its node type once.
mod ego_tree_node {
    pub type NodeRef<'a> = ego_tree::NodeRef<'a, scraper::Node>;
}

fn element(node: ego_tree_node::NodeRef<'_>, el: &scraper::node::Element, ctx: &Ctx, b: &mut Builder) {
    let tag = el.name().to_ascii_lowercase();
    if matches!(tag.as_str(), "script" | "style" | "head" | "title" | "meta" | "link" | "noscript" | "template" | "iframe" | "object" | "embed" | "input" | "select" | "textarea" | "button" | "svg" | "map") {
        return;
    }
    if el.attr("hidden").is_some() {
        return;
    }
    let css = el.attr("style").map(parse_style).unwrap_or_default();
    if css.hidden {
        return;
    }
    let mut style = ctx.style;
    if let Some(c) = css.color.or_else(|| if tag == "font" { el.attr("color").and_then(parse_color) } else { None }) {
        style.color = Some(c);
    }
    if let Some(bg) = css.background {
        style.background = Some(bg);
    }
    if let Some(v) = css.bold {
        style.bold = v;
    }
    if let Some(v) = css.italic {
        style.italic = v;
    }
    if let Some(v) = css.underline {
        style.underline = v;
    }
    if let Some(v) = css.strike {
        style.strike = v;
    }
    if let Some(s) = css.size {
        style.size = Some(s.clamp(6.0, 36.0));
    }
    if css.mono {
        style.mono = true;
    }
    match tag.as_str() {
        "b" | "strong" => style.bold = true,
        "i" | "em" | "cite" | "var" | "dfn" => style.italic = true,
        "u" | "ins" => style.underline = true,
        "s" | "strike" | "del" => style.strike = true,
        "code" | "tt" | "kbd" | "samp" => style.mono = true,
        "small" | "sup" | "sub" => style.small = true,
        "th" => style.bold = true,
        "font" => {
            if let Some(sz) = el.attr("size").and_then(|s| s.trim().parse::<i32>().ok()) {
                style.size = Some(match sz {
                    i32::MIN..=1 => 7.5,
                    2 => 10.0,
                    3 => 12.0,
                    4 => 13.5,
                    5 => 18.0,
                    6 => 24.0,
                    _ => 36.0,
                });
            }
            if el.attr("face").is_some_and(|f| f.to_ascii_lowercase().contains("courier")) {
                style.mono = true;
            }
        }
        "a" => {}
        _ => {}
    }
    let link = if tag == "a" { el.attr("href").and_then(safe_link).or(ctx.link.clone()) } else { ctx.link.clone() };
    if tag == "a" && link.is_some() && ctx.link.is_none() && style.color.is_none() {
        style.underline = true;
        style.color = Some([5, 99, 193]);
    }
    let align = css.align.or_else(|| match el.attr("align").map(|a| a.to_ascii_lowercase()) {
        Some(a) if a == "center" => Some(Align::Center),
        Some(a) if a == "right" => Some(Align::Right),
        _ => None,
    });
    let align = if tag == "center" { Align::Center } else { align.unwrap_or(ctx.align) };
    let inner = Ctx { style, link, align, pre: ctx.pre || tag == "pre" };

    match tag.as_str() {
        "br" => {
            b.inlines.push(Inline::Break);
            b.space_pending = false;
        }
        "img" => {
            if let Some(img) = image_from(el, ctx.link.clone(), &mut b.remote) {
                if b.space_pending && b.has_content() {
                    b.text(" ", ctx);
                }
                b.inlines.push(Inline::Image(img));
            }
        }
        "hr" => {
            b.flush();
            b.blocks.push(Block::Rule);
        }
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            b.flush();
            let level = tag[1..].parse().unwrap_or(3);
            b.heading = Some(level);
            b.align = align;
            walk(node, &inner, b);
            b.flush();
            b.heading = None;
            b.align = ctx.align;
        }
        "pre" => {
            b.flush();
            let mut text = String::new();
            collect_text(node, &mut text);
            let text = text.trim_matches('\n').to_string();
            if !text.is_empty() {
                b.blocks.push(Block::Pre(text));
            }
        }
        "blockquote" => {
            b.flush();
            let mut sub = Builder::new();
            sub.list_depth = b.list_depth;
            walk(node, &inner, &mut sub);
            sub.flush();
            b.remote += sub.remote;
            if !sub.blocks.is_empty() {
                b.blocks.push(Block::Quote(sub.blocks));
            }
        }
        "ul" | "ol" => {
            b.flush();
            let ordered = tag == "ol";
            let mut n: i64 = el.attr("start").and_then(|s| s.parse().ok()).unwrap_or(1);
            let kind = el.attr("type").unwrap_or("1").to_string();
            let depth = b.list_depth;
            b.list_depth += 1;
            for li in node.children() {
                match li.value() {
                    Node::Element(e) if e.name().eq_ignore_ascii_case("li") => {
                        let marker = if ordered {
                            let m = match kind.as_str() {
                                "a" => ((b'a' + ((n - 1).rem_euclid(26)) as u8) as char).to_string(),
                                "A" => ((b'A' + ((n - 1).rem_euclid(26)) as u8) as char).to_string(),
                                "i" => roman(n).to_lowercase(),
                                "I" => roman(n),
                                _ => n.to_string(),
                            };
                            n += 1;
                            format!("{m}.")
                        } else {
                            ["\u{2022}", "\u{25e6}", "\u{25aa}"][(depth as usize) % 3].to_string()
                        };
                        b.flush();
                        b.list_item = Some((marker, depth));
                        let li_css = e.attr("style").map(parse_style).unwrap_or_default();
                        if !li_css.hidden {
                            walk(li, &inner, b);
                        }
                        b.flush();
                        b.list_item = None;
                    }
                    Node::Element(_) => element(li, match li.value() {
                        Node::Element(e) => e,
                        _ => continue,
                    }, &inner, b),
                    Node::Text(t) if !t.trim().is_empty() => b.text(t, &inner),
                    _ => {}
                }
            }
            b.list_depth -= 1;
        }
        "table" => {
            b.flush();
            let table = table(node, el, &inner, b);
            match table {
                Some(Block::Table { rows, .. }) if rows.iter().all(|r| r.len() <= 1) => {
                    // A layout table with one column: just its contents.
                    for r in rows {
                        for c in r {
                            if let Some(bg) = c.background {
                                for blk in c.blocks {
                                    b.blocks.push(match blk {
                                        Block::Para { inlines, align, background: None } => Block::Para { inlines, align, background: Some(bg) },
                                        other => other,
                                    });
                                }
                            } else {
                                b.blocks.extend(c.blocks);
                            }
                        }
                    }
                }
                Some(t) => b.blocks.push(t),
                None => {}
            }
        }
        "li" => {
            // A stray list item.
            b.flush();
            b.list_item = Some(("\u{2022}".into(), b.list_depth));
            walk(node, &inner, b);
            b.flush();
            b.list_item = None;
        }
        t if BLOCK_TAGS.contains(&t) || t == "tr" || t == "td" || t == "th" || t == "tbody" => {
            b.flush();
            let saved = b.align;
            b.align = align;
            let start = b.blocks.len();
            walk(node, &inner, b);
            b.flush();
            b.align = saved;
            if let (Some(bg), "div" | "p" | "center") = (css.background, t) {
                for blk in &mut b.blocks[start..] {
                    if let Block::Para { background, .. } = blk {
                        background.get_or_insert(bg);
                    }
                }
            }
        }
        _ => walk(node, &inner, b),
    }
}

fn roman(mut n: i64) -> String {
    let mut s = String::new();
    for (v, r) in [(1000, "M"), (900, "CM"), (500, "D"), (400, "CD"), (100, "C"), (90, "XC"), (50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I")] {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

fn collect_text(node: ego_tree_node::NodeRef<'_>, out: &mut String) {
    for c in node.children() {
        match c.value() {
            Node::Text(t) => out.push_str(t),
            Node::Element(e) if e.name().eq_ignore_ascii_case("br") => out.push('\n'),
            Node::Element(_) => collect_text(c, out),
            _ => {}
        }
    }
}

fn table(node: ego_tree_node::NodeRef<'_>, el: &scraper::node::Element, ctx: &Ctx, b: &mut Builder) -> Option<Block> {
    let border = el.attr("border").and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(0) > 0
        || el.attr("style").is_some_and(|s| {
            let s = s.to_ascii_lowercase();
            s.contains("border:") && !s.contains("border:0") && !s.contains("border: 0") && !s.contains("border:none") && !s.contains("border: none")
        });
    let table_bg = el.attr("bgcolor").and_then(parse_color).or_else(|| el.attr("style").map(parse_style).and_then(|c| c.background));
    let width = el.attr("width").and_then(parse_width).or_else(|| el.attr("style").map(parse_style).and_then(|c| c.width));
    let mut rows: Vec<Vec<Cell>> = Vec::new();
    collect_rows(node, ctx, b, table_bg, &mut rows);
    rows.retain(|r| !r.is_empty());
    if rows.is_empty() {
        return None;
    }
    Some(Block::Table { rows, border, width })
}

fn collect_rows(node: ego_tree_node::NodeRef<'_>, ctx: &Ctx, b: &mut Builder, table_bg: Option<[u8; 3]>, rows: &mut Vec<Vec<Cell>>) {
    for child in node.children() {
        let Node::Element(e) = child.value() else { continue };
        let name = e.name().to_ascii_lowercase();
        match name.as_str() {
            "thead" | "tbody" | "tfoot" => collect_rows(child, ctx, b, table_bg, rows),
            "tr" => {
                if e.attr("style").map(parse_style).is_some_and(|c| c.hidden) {
                    continue;
                }
                let row_bg = e.attr("bgcolor").and_then(parse_color).or_else(|| e.attr("style").map(parse_style).and_then(|c| c.background));
                let mut cells = Vec::new();
                for c in child.children() {
                    let Node::Element(ce) = c.value() else { continue };
                    let cname = ce.name().to_ascii_lowercase();
                    if cname != "td" && cname != "th" {
                        continue;
                    }
                    let css = ce.attr("style").map(parse_style).unwrap_or_default();
                    if css.hidden {
                        continue;
                    }
                    let mut sub = Builder::new();
                    let mut style = ctx.style;
                    if cname == "th" {
                        style.bold = true;
                    }
                    if let Some(col) = css.color {
                        style.color = Some(col);
                    }
                    if let Some(v) = css.bold {
                        style.bold = v;
                    }
                    if let Some(s) = css.size {
                        style.size = Some(s.clamp(6.0, 36.0));
                    }
                    let align = css.align.unwrap_or(match ce.attr("align").map(|a| a.to_ascii_lowercase()).as_deref() {
                        Some("center") => Align::Center,
                        Some("right") => Align::Right,
                        _ => if cname == "th" { Align::Center } else { Align::Left },
                    });
                    sub.align = align;
                    let cctx = Ctx { style, link: ctx.link.clone(), align, pre: ctx.pre };
                    walk(c, &cctx, &mut sub);
                    sub.flush();
                    b.remote += sub.remote;
                    cells.push(Cell {
                        blocks: sub.blocks,
                        header: cname == "th",
                        background: ce.attr("bgcolor").and_then(parse_color).or(css.background).or(row_bg).or(table_bg),
                        colspan: ce.attr("colspan").and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(1).clamp(1, 50),
                        width: ce.attr("width").and_then(parse_width).or(css.width),
                        align,
                    });
                }
                // Rows of nothing but empty spacer cells add nothing.
                if cells.iter().any(|c| !c.blocks.is_empty()) {
                    rows.push(cells);
                }
            }
            _ => {}
        }
    }
}

/// Parse a message body. Plain text bodies are fine too (they come wrapped by the cache).
pub fn parse(html: &str) -> Document {
    let doc = Html::parse_document(html);
    let mut b = Builder::new();
    let ctx = Ctx { style: Style::default(), link: None, align: Align::Left, pre: false };
    walk(doc.tree.root(), &ctx, &mut b);
    b.flush();
    Document { blocks: b.blocks, remote_images: b.remote }
}

fn inline_text(inlines: &[Inline], out: &mut String) {
    for i in inlines {
        match i {
            Inline::Text { text, link, .. } => {
                out.push_str(text);
                if let Some(l) = link
                    && !l.starts_with("mailto:") && !text.contains(l.as_str()) {
                        out.push_str(&format!(" <{l}>"));
                    }
            }
            Inline::Image(img) if !img.alt.is_empty() => out.push_str(&format!("[{}]", img.alt)),
            Inline::Image(_) => {}
            Inline::Break => out.push('\n'),
        }
    }
}

fn blocks_text(blocks: &[Block], out: &mut String) {
    for blk in blocks {
        match blk {
            Block::Para { inlines, .. } | Block::Heading { inlines, .. } => {
                inline_text(inlines, out);
                out.push_str("\n\n");
            }
            Block::ListItem { marker, depth, inlines } => {
                out.push_str(&"  ".repeat(*depth as usize));
                out.push_str(marker);
                out.push(' ');
                inline_text(inlines, out);
                out.push('\n');
            }
            Block::Quote(inner) => {
                let mut s = String::new();
                blocks_text(inner, &mut s);
                for line in s.trim_end().lines() {
                    out.push_str("> ");
                    out.push_str(line);
                    out.push('\n');
                }
                out.push('\n');
            }
            Block::Pre(t) => {
                out.push_str(t);
                out.push_str("\n\n");
            }
            Block::Rule => out.push_str("________________________________\n\n"),
            Block::Table { rows, .. } => {
                for r in rows {
                    let cells: Vec<String> = r
                        .iter()
                        .map(|c| {
                            let mut s = String::new();
                            blocks_text(&c.blocks, &mut s);
                            s.split_whitespace().collect::<Vec<_>>().join(" ")
                        })
                        .collect();
                    out.push_str(&cells.join("\t"));
                    out.push('\n');
                }
                out.push('\n');
            }
        }
    }
}

/// The plain text of an HTML body.
pub fn to_text(html: &str) -> String {
    let doc = parse(html);
    let mut out = String::new();
    blocks_text(&doc.blocks, &mut out);
    let mut s = out.trim().to_string();
    while s.contains("\n\n\n") {
        s = s.replace("\n\n\n", "\n\n");
    }
    s
}

pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// What the user typed, as HTML: escaped, paragraphs and line breaks kept, links made clickable.
pub fn text_to_html(text: &str, font: &str, size_pt: f32) -> String {
    let mut out = format!("<div style=\"font-family:{};font-size:{size_pt}pt\">", escape(font));
    let text = text.replace("\r\n", "\n");
    for line in text.split('\n') {
        if line.trim().is_empty() {
            out.push_str("<div><br></div>");
            continue;
        }
        out.push_str("<div>");
        let mut first = true;
        for word in line.split(' ') {
            if !first {
                out.push(' ');
            }
            first = false;
            let trimmed = word.trim_end_matches(['.', ',', ')', ';', ':', '!', '?']);
            let tail = &word[trimmed.len()..];
            let lower = trimmed.to_ascii_lowercase();
            if (lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("www.")) && trimmed.len() > 8 {
                let href = if lower.starts_with("www.") { format!("https://{trimmed}") } else { trimmed.to_string() };
                out.push_str(&format!("<a href=\"{}\">{}</a>{}", escape(&href), escape(trimmed), escape(tail)));
            } else if trimmed.contains('@') && trimmed.contains('.') && !trimmed.starts_with('@') && trimmed.chars().all(|c| c.is_alphanumeric() || "@._-+".contains(c)) {
                out.push_str(&format!("<a href=\"mailto:{0}\">{0}</a>{1}", escape(trimmed), escape(tail)));
            } else {
                // Keep runs of spaces.
                out.push_str(&escape(word).replace("  ", " &nbsp;"));
            }
        }
        out.push_str("</div>");
    }
    out.push_str("</div>");
    out
}

/// The `<body>` contents of an HTML document (for quoting it inside another).
pub fn body_inner(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<body").and_then(|i| lower[i..].find('>').map(|j| i + j + 1));
    let end = lower.rfind("</body>");
    match (start, end) {
        (Some(s), Some(e)) if e > s => html[s..e].to_string(),
        (Some(s), None) => html[s..].to_string(),
        _ => {
            // Drop a head if there's no body tag.
            match lower.find("</head>") {
                Some(h) => html[h + 7..].to_string(),
                None => html.to_string(),
            }
        }
    }
}

/// `<style>` blocks from the head of an HTML document (kept when quoting it).
pub fn head_styles(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let mut out = String::new();
    let mut from = 0;
    while let Some(s) = lower[from..].find("<style").map(|i| i + from) {
        let Some(e) = lower[s..].find("</style>").map(|i| i + s + 8) else { break };
        out.push_str(&html[s..e]);
        from = e;
    }
    out
}

#[cfg(test)]
mod tests;
