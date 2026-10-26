//! Drawing a parsed HTML body ([`mailwright_html::Document`]) with egui widgets.

use std::collections::HashMap;

use egui::{Color32, FontFamily, FontId, RichText, Sense, Stroke, TextureHandle, Ui, vec2};
use mailwright_html::{Align, Block, Cell, Document, Image, ImageSource, Inline, Style};

use crate::theme::Tokens;

/// Default body text size (11 pt).
pub const BODY_PX: f32 = 14.5;

pub struct RenderCtx<'a> {
    pub message_id: &'a str,
    pub textures: &'a HashMap<String, TextureHandle>,
    pub remote: &'a HashMap<String, Option<TextureHandle>>,
    pub allow_remote: bool,
    pub tokens: Tokens,
    /// Draw on a dark background (adapt dark text and light fills).
    pub dark: bool,
}

/// What the user did in the body.
#[derive(Debug, Clone, PartialEq)]
pub enum BodyAction {
    Link(String),
}

pub fn decode_texture(ctx: &egui::Context, key: &str, bytes: &[u8]) -> Option<TextureHandle> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = if img.width() > 4096 || img.height() > 4096 { img.thumbnail(4096, 4096) } else { img };
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let ci = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    Some(ctx.load_texture(key, ci, egui::TextureOptions::LINEAR))
}

fn lum(c: [u8; 3]) -> f32 {
    (0.2126 * c[0] as f32 + 0.7152 * c[1] as f32 + 0.0722 * c[2] as f32) / 255.0
}

/// A mail colour, adjusted for a dark background: dark text gets light, light fills get dark.
fn text_color(c: Option<[u8; 3]>, rc: &RenderCtx<'_>) -> Color32 {
    match c {
        None => rc.tokens.text,
        Some(c) if rc.dark && lum(c) < 0.45 => {
            let k = 1.0 - lum(c);
            let v = (160.0 + 90.0 * k) as u8;
            if c[0].abs_diff(c[1]) < 24 && c[1].abs_diff(c[2]) < 24 {
                Color32::from_rgb(v, v, v)
            } else {
                Color32::from_rgb(c[0].saturating_add(110), c[1].saturating_add(110), c[2].saturating_add(110))
            }
        }
        Some(c) => Color32::from_rgb(c[0], c[1], c[2]),
    }
}

fn fill_color(c: [u8; 3], rc: &RenderCtx<'_>) -> Option<Color32> {
    if rc.dark {
        if lum(c) > 0.85 {
            return None;
        }
        if lum(c) > 0.55 {
            return Some(Color32::from_rgb(c[0] / 4, c[1] / 4, c[2] / 4));
        }
    } else if c == [255, 255, 255] {
        return None;
    }
    Some(Color32::from_rgb(c[0], c[1], c[2]))
}

fn rich(text: &str, s: &Style, link: bool, rc: &RenderCtx<'_>, base: f32) -> RichText {
    let size = s.size.map(|pt| pt * 4.0 / 3.0).unwrap_or(base) * if s.small { 0.82 } else { 1.0 };
    let family = if s.mono {
        FontFamily::Monospace
    } else if s.bold {
        FontFamily::Name("semibold".into())
    } else {
        FontFamily::Proportional
    };
    let mut rt = RichText::new(text).font(FontId::new(size, family));
    rt = rt.color(if link && s.color == Some([5, 99, 193]) { rc.tokens.accent_text } else { text_color(s.color, rc) });
    if s.italic {
        rt = rt.italics();
    }
    if s.underline || link {
        rt = rt.underline();
    }
    if s.strike {
        rt = rt.strikethrough();
    }
    if let Some(bg) = s.background.and_then(|b| fill_color(b, rc)) {
        rt = rt.background_color(bg);
    }
    rt
}

pub fn document(ui: &mut Ui, doc: &Document, rc: &RenderCtx<'_>) -> Option<BodyAction> {
    let mut out = None;
    ui.spacing_mut().item_spacing.y = 0.0;
    blocks(ui, &doc.blocks, rc, &mut out, 0);
    out
}

fn blocks(ui: &mut Ui, list: &[Block], rc: &RenderCtx<'_>, out: &mut Option<BodyAction>, depth: usize) {
    if depth > 24 {
        return;
    }
    for (i, b) in list.iter().enumerate() {
        match b {
            Block::Para { inlines, align, background } => {
                let fill = background.and_then(|c| fill_color(c, rc));
                para(ui, inlines, *align, fill, rc, out, BODY_PX, false);
                if i + 1 < list.len() {
                    ui.add_space(8.0);
                }
            }
            Block::Heading { level, inlines, align } => {
                let size = match level {
                    1 => 26.0,
                    2 => 21.0,
                    3 => 18.0,
                    4 => 16.0,
                    _ => 14.5,
                };
                ui.add_space(4.0);
                para(ui, inlines, *align, None, rc, out, size, true);
                ui.add_space(8.0);
            }
            Block::ListItem { marker, depth: d, inlines } => {
                ui.horizontal_top(|ui| {
                    ui.add_space(8.0 + *d as f32 * 22.0);
                    let (r, _) = ui.allocate_exact_size(vec2(18.0, BODY_PX * 1.3), Sense::hover());
                    ui.painter().text(r.right_center() - vec2(4.0, 0.0), egui::Align2::RIGHT_CENTER, marker, FontId::proportional(BODY_PX), rc.tokens.text);
                    ui.vertical(|ui| para(ui, inlines, Align::Left, None, rc, out, BODY_PX, false));
                });
                ui.add_space(3.0);
            }
            Block::Quote(inner) => {
                let resp = egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 0, top: 2, bottom: 2 }).show(ui, |ui| blocks(ui, inner, rc, out, depth + 1));
                let r = resp.response.rect;
                ui.painter().line_segment([r.left_top() + vec2(2.0, 0.0), r.left_bottom() + vec2(2.0, 0.0)], Stroke::new(2.0, rc.tokens.border_strong));
                ui.add_space(8.0);
            }
            Block::Pre(text) => {
                egui::Frame::new().fill(rc.tokens.header_bg).inner_margin(egui::Margin::same(8)).corner_radius(4.0).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(text).monospace().size(13.0).color(rc.tokens.text)).wrap());
                });
                ui.add_space(8.0);
            }
            Block::Rule => {
                ui.add_space(4.0);
                let w = ui.available_width();
                let (r, _) = ui.allocate_exact_size(vec2(w, 1.0), Sense::hover());
                ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(1.0, rc.tokens.border_strong));
                ui.add_space(8.0);
            }
            Block::Table { rows, border, width } => {
                table(ui, rows, *border, *width, rc, out, depth);
                ui.add_space(8.0);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn para(ui: &mut Ui, inlines: &[Inline], align: Align, fill: Option<Color32>, rc: &RenderCtx<'_>, out: &mut Option<BodyAction>, base: f32, bold: bool) {
    let main = match align {
        Align::Left => egui::Align::Min,
        Align::Center => egui::Align::Center,
        Align::Right => egui::Align::Max,
    };
    let layout = egui::Layout::left_to_right(egui::Align::Max).with_main_wrap(true).with_main_align(main);
    let frame = egui::Frame::new().fill(fill.unwrap_or(Color32::TRANSPARENT)).inner_margin(if fill.is_some() { egui::Margin::same(6) } else { egui::Margin::ZERO });
    frame.show(ui, |ui| {
        let w = ui.available_width();
        ui.allocate_ui_with_layout(vec2(w, 0.0), layout, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 2.0);
            for inl in inlines {
                match inl {
                    Inline::Text { text, style, link } => {
                        let mut s = *style;
                        if bold {
                            s.bold = true;
                        }
                        let rt = rich(text, &s, link.is_some(), rc, base);
                        match link {
                            Some(url) => {
                                let resp = ui.add(egui::Label::new(rt).sense(Sense::click()).wrap());
                                let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(url.as_str());
                                if resp.clicked() {
                                    *out = Some(BodyAction::Link(url.clone()));
                                }
                            }
                            None => {
                                ui.add(egui::Label::new(rt).wrap());
                            }
                        }
                    }
                    Inline::Image(img) => {
                        if let Some(a) = image(ui, img, rc) {
                            *out = Some(a);
                        }
                    }
                    Inline::Break => ui.end_row(),
                }
            }
        });
    });
}

fn image(ui: &mut Ui, img: &Image, rc: &RenderCtx<'_>) -> Option<BodyAction> {
    let tex = match &img.source {
        ImageSource::Cid(cid) => rc.textures.get(&format!("{}/cid:{cid}", rc.message_id)).cloned(),
        ImageSource::Data { bytes, .. } => {
            let key = format!("{}/data:{}", rc.message_id, bytes.len() ^ bytes.iter().take(64).fold(0usize, |h, b| h.wrapping_mul(31) ^ *b as usize));
            match rc.textures.get(&key) {
                Some(t) => Some(t.clone()),
                None => decode_texture(ui.ctx(), &key, bytes),
            }
        }
        ImageSource::Remote(url) => {
            if rc.allow_remote {
                rc.remote.get(url).cloned().flatten()
            } else {
                None
            }
        }
    };
    let avail = ui.available_width().max(40.0);
    let (w, h) = match (&tex, img.width, img.height) {
        (_, Some(w), Some(h)) => (w, h),
        (Some(t), Some(w), None) => (w, w * t.size_vec2().y / t.size_vec2().x.max(1.0)),
        (Some(t), None, Some(h)) => (h * t.size_vec2().x / t.size_vec2().y.max(1.0), h),
        (Some(t), None, None) => (t.size_vec2().x, t.size_vec2().y),
        (None, w, h) => (w.unwrap_or(120.0), h.unwrap_or(40.0)),
    };
    let scale = if w > avail { avail / w } else { 1.0 };
    let size = vec2(w * scale, h * scale).max(vec2(4.0, 4.0));
    let resp = match tex {
        Some(t) => ui.add(egui::Image::from_texture((t.id(), size)).sense(Sense::click())),
        None => {
            let (r, resp) = ui.allocate_exact_size(size.min(vec2(avail, 400.0)), Sense::click());
            ui.painter().rect(r, 2.0, rc.tokens.header_bg, Stroke::new(1.0, rc.tokens.border), egui::StrokeKind::Inside);
            if r.width() > 40.0 && r.height() > 18.0 {
                let label = if img.alt.is_empty() { "Picture".to_string() } else { img.alt.clone() };
                let clipped = ui.painter().with_clip_rect(r.shrink(2.0));
                clipped.text(r.left_top() + vec2(6.0, 4.0), egui::Align2::LEFT_TOP, label, FontId::proportional(11.5), rc.tokens.text_dim);
            }
            resp
        }
    };
    if let Some(link) = &img.link {
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(link.as_str());
        if resp.clicked() {
            return Some(BodyAction::Link(link.clone()));
        }
    }
    None
}

fn table(ui: &mut Ui, rows: &[Vec<Cell>], border: bool, width: Option<f32>, rc: &RenderCtx<'_>, out: &mut Option<BodyAction>, depth: usize) {
    let avail = ui.available_width();
    let cols = rows.iter().map(|r| r.iter().map(|c| c.colspan).sum::<usize>()).max().unwrap_or(1).max(1);
    let table_w = match width {
        Some(w) if w < 0.0 => avail * -w,
        Some(w) => (w / 0.75).min(avail),
        None => avail,
    }
    .max(60.0);
    // Column widths: hints from the first row that gives them, the rest shared out.
    let mut col_w: Vec<Option<f32>> = vec![None; cols];
    for r in rows {
        let mut c = 0;
        for cell in r {
            if cell.colspan == 1 && c < cols && col_w[c].is_none() {
                col_w[c] = cell.width.map(|w| if w < 0.0 { table_w * -w } else { w / 0.75 });
            }
            c += cell.colspan;
        }
    }
    let fixed: f32 = col_w.iter().flatten().sum();
    let free = col_w.iter().filter(|w| w.is_none()).count();
    let share = if free > 0 { ((table_w - fixed) / free as f32).max(30.0) } else { 0.0 };
    let mut widths: Vec<f32> = col_w.iter().map(|w| w.unwrap_or(share)).collect();
    let sum: f32 = widths.iter().sum();
    if sum > table_w {
        let k = table_w / sum;
        for w in &mut widths {
            *w *= k;
        }
    }
    let pad = if border { 5.0 } else { 3.0 };
    for r in rows {
        let bg_idx: Vec<egui::layers::ShapeIdx> = r.iter().map(|_| ui.painter().add(egui::Shape::Noop)).collect();
        let resp = ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            let mut c = 0;
            let mut rects = Vec::new();
            for cell in r {
                let w: f32 = widths.iter().skip(c).take(cell.colspan).sum::<f32>().max(20.0);
                c += cell.colspan;
                let inner = ui.allocate_ui_with_layout(vec2(w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_width(w);
                    egui::Frame::new().inner_margin(egui::Margin::same(pad as i8)).show(ui, |ui| {
                        ui.set_width(w - 2.0 * pad);
                        let mut list = cell.blocks.clone();
                        if cell.align != Align::Left {
                            for b in &mut list {
                                if let Block::Para { align, .. } = b {
                                    *align = cell.align;
                                }
                            }
                        }
                        blocks(ui, &list, rc, out, depth + 1);
                    });
                });
                rects.push(inner.response.rect);
            }
            rects
        });
        let rects = resp.inner;
        let row_h = rects.iter().map(|r| r.height()).fold(0.0, f32::max);
        for (k, (cell, rect)) in r.iter().zip(rects.iter()).enumerate() {
            let full = egui::Rect::from_min_size(rect.min, vec2(rect.width(), row_h));
            let fill = cell.background.and_then(|c| fill_color(c, rc)).or(if cell.header && border { Some(rc.tokens.header_bg) } else { None });
            let shape = match (fill, border) {
                (Some(f), true) => egui::Shape::rect_filled(full, 0.0, f),
                (Some(f), false) => egui::Shape::rect_filled(full, 0.0, f),
                (None, _) => egui::Shape::Noop,
            };
            if let Some(idx) = bg_idx.get(k) {
                ui.painter().set(*idx, shape);
            }
            if border {
                ui.painter().rect_stroke(full, 0.0, Stroke::new(1.0, rc.tokens.border_strong), egui::StrokeKind::Inside);
            }
        }
    }
}
