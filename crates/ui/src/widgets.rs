//! Ribbon buttons and groups, the tab strip, and small shared widgets.

use egui::{Align2, Color32, CornerRadius, Rect, Response, Sense, Stroke, Ui, pos2, vec2};

use crate::icons;
use crate::theme::{Tokens, medium, regular, semibold};

/// Height of a ribbon group's content, its title row, and the big-button layout.
pub const CONTENT_H: f32 = 66.0;
pub const LABEL_H: f32 = 18.0;
const BIG_ICON_Y: f32 = 18.0;
const BIG_LABEL_Y: f32 = 36.0;
const LABEL_PITCH: f32 = 13.0;
const DROPDOWN: f32 = 9.0;

fn bg(ui: &Ui, r: Rect, resp: &Response, checked: bool, enabled: bool, t: &Tokens) {
    let fill = if enabled && resp.is_pointer_button_down_on() {
        t.pressed
    } else if checked {
        t.checked
    } else if enabled && resp.hovered() {
        t.hover
    } else {
        return;
    };
    ui.painter().rect_filled(r, CornerRadius::same(4), fill);
    if checked {
        ui.painter().rect_stroke(r, CornerRadius::same(4), Stroke::new(1.0, t.accent.linear_multiply(0.5)), egui::StrokeKind::Inside);
    }
}

fn label_line(ui: &Ui, cx: f32, y: f32, text: &str, color: Color32, marker: bool) {
    let galley = ui.painter().layout_no_wrap(text.to_string(), regular(11.5), color);
    let extra = if marker { DROPDOWN + 3.0 } else { 0.0 };
    let size = galley.size();
    let x = cx - (size.x + extra) / 2.0;
    ui.painter().galley(pos2(x, y), galley, color);
    if marker {
        let c = pos2(x + size.x + 3.0 + DROPDOWN / 2.0, y + size.y / 2.0);
        icons::paint(ui.painter(), Rect::from_center_size(c, vec2(DROPDOWN, DROPDOWN)), "dropdown", color, color);
    }
}

/// A large ribbon button: a 32 px icon over a (possibly two-line) label. `menu` adds the
/// drop-down marker. Clicks only count when `enabled`.
pub fn big(ui: &mut Ui, icon: &str, label: &str, enabled: bool, checked: bool, menu: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let text_w = label.split('\n').map(|l| ui.painter().layout_no_wrap(l.to_string(), regular(11.5), t.text).size().x).fold(0.0, f32::max);
    let w = (text_w + if menu { DROPDOWN + 3.0 } else { 0.0 } + 12.0).max(46.0);
    let (r, resp) = ui.allocate_exact_size(vec2(w, CONTENT_H), Sense::click());
    bg(ui, r, &resp, checked, enabled, &t);
    let ic = Rect::from_center_size(pos2(r.center().x, r.min.y + BIG_ICON_Y), vec2(32.0, 32.0));
    let (c, a) = if enabled { (t.icon, t.accent) } else { (t.text_disabled, t.text_disabled) };
    icons::paint(ui.painter(), ic, icon, c, a);
    let lines: Vec<&str> = label.split('\n').collect();
    let mut y = r.min.y + BIG_LABEL_Y;
    for (i, l) in lines.iter().enumerate() {
        label_line(ui, r.center().x, y, l, if enabled { t.text } else { t.text_disabled }, menu && i + 1 == lines.len());
        y += LABEL_PITCH;
    }
    let plain = label.replace('\n', " ");
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &plain));
    if !enabled {
        return resp.on_hover_text(plain);
    }
    resp
}

/// A small ribbon button: a 16 px icon and a label, 22 px tall.
pub fn small(ui: &mut Ui, icon: &str, label: &str, enabled: bool, checked: bool, menu: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let text_w = if label.is_empty() { 0.0 } else { ui.painter().layout_no_wrap(label.to_string(), regular(11.5), t.text).size().x + 6.0 };
    let w = 24.0 + text_w + if menu { DROPDOWN + 4.0 } else { 0.0 };
    let (r, resp) = ui.allocate_exact_size(vec2(w, 22.0), Sense::click());
    bg(ui, r, &resp, checked, enabled, &t);
    let (c, a) = if enabled { (t.icon, t.accent) } else { (t.text_disabled, t.text_disabled) };
    icons::paint(ui.painter(), Rect::from_center_size(pos2(r.min.x + 12.0, r.center().y), vec2(17.0, 17.0)), icon, c, a);
    let col = if enabled { t.text } else { t.text_disabled };
    if !label.is_empty() {
        ui.painter().text(pos2(r.min.x + 24.0, r.center().y), Align2::LEFT_CENTER, label, regular(11.5), col);
    }
    if menu {
        icons::paint(ui.painter(), Rect::from_center_size(pos2(r.max.x - DROPDOWN / 2.0 - 3.0, r.center().y), vec2(DROPDOWN, DROPDOWN)), "dropdown", col, col);
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    resp
}

/// A ribbon group: contents, then the title centred underneath, then a separator.
pub fn group(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    let t = Tokens::get(ui.ctx());
    let start = ui.cursor().min;
    let title_w = ui.painter().layout_no_wrap(title.to_string(), regular(11.0), t.group_label).size().x;
    let inner = ui.vertical(|ui| {
        ui.set_min_width(title_w + 10.0);
        ui.allocate_ui_with_layout(vec2(ui.available_width(), CONTENT_H), egui::Layout::left_to_right(egui::Align::Min), |ui| {
            ui.spacing_mut().item_spacing = vec2(1.0, 1.0);
            add(ui);
        });
        ui.add_space(LABEL_H);
    });
    let r = inner.response.rect;
    ui.painter().text(pos2((start.x + r.max.x) / 2.0, start.y + CONTENT_H + LABEL_H / 2.0), Align2::CENTER_CENTER, title, regular(11.0), t.group_label);
    ui.add_space(5.0);
    let x = ui.cursor().min.x;
    ui.painter().line_segment([pos2(x, start.y + 4.0), pos2(x, start.y + CONTENT_H + LABEL_H - 4.0)], Stroke::new(1.0, t.border));
    ui.add_space(6.0);
}

/// A column of up to three small buttons inside a group.
pub fn stack(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(vec2(0.0, CONTENT_H), egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.spacing_mut().item_spacing = vec2(1.0, 0.0);
        add(ui);
    });
}

/// The ribbon's tab strip. Returns the clicked tab, if any. Tab 0 is File, drawn in the app colour.
pub fn tab_strip(ui: &mut Ui, tabs: &[&str], current: usize) -> Option<usize> {
    let t = Tokens::get(ui.ctx());
    let mut clicked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.add_space(4.0);
        for (i, name) in tabs.iter().enumerate() {
            let font = if i == current { semibold(12.5) } else { medium(12.5) };
            let w = ui.painter().layout_no_wrap(name.to_string(), font.clone(), t.text).size().x + 22.0;
            let (r, resp) = ui.allocate_exact_size(vec2(w, 28.0), Sense::click());
            if i == 0 {
                let fill = if resp.hovered() { t.accent_text } else { t.accent };
                ui.painter().rect_filled(r.shrink2(vec2(1.0, 3.0)), CornerRadius::same(4), fill);
                ui.painter().text(r.center(), Align2::CENTER_CENTER, *name, medium(12.5), t.on_accent);
            } else {
                if resp.hovered() && i != current {
                    ui.painter().rect_filled(r.shrink2(vec2(1.0, 3.0)), CornerRadius::same(4), t.hover);
                }
                let col = if i == current { t.accent_text } else { t.text };
                ui.painter().text(r.center(), Align2::CENTER_CENTER, *name, font, col);
                if i == current {
                    let y = r.max.y - 2.0;
                    ui.painter().line_segment([pos2(r.center().x - w / 2.0 + 10.0, y), pos2(r.center().x + w / 2.0 - 10.0, y)], Stroke::new(2.5, t.accent));
                }
            }
            resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, i == current, *name));
            if resp.clicked() {
                clicked = Some(i);
            }
        }
    });
    clicked
}

/// A square icon button with a tooltip.
pub fn icon_button(ui: &mut Ui, icon: &str, tip: &str, enabled: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(26.0, 24.0), Sense::click());
    bg(ui, r, &resp, false, enabled, &t);
    let (c, a) = if enabled { (t.icon, t.accent) } else { (t.text_disabled, t.text_disabled) };
    icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(16.0, 16.0)), icon, c, a);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, tip));
    resp.on_hover_text(tip)
}

/// An icon and text button (reading pane actions, toolbars).
pub fn icon_text(ui: &mut Ui, icon: &str, text: &str, enabled: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let tw = ui.painter().layout_no_wrap(text.to_string(), regular(12.0), t.text).size().x;
    let (r, resp) = ui.allocate_exact_size(vec2(tw + 34.0, 28.0), Sense::click());
    let fill = if enabled && resp.is_pointer_button_down_on() {
        t.pressed
    } else if enabled && resp.hovered() {
        t.hover
    } else {
        t.input
    };
    ui.painter().rect(r, CornerRadius::same(4), fill, Stroke::new(1.0, t.border_strong), egui::StrokeKind::Inside);
    let (c, a) = if enabled { (t.icon, t.accent) } else { (t.text_disabled, t.text_disabled) };
    icons::paint(ui.painter(), Rect::from_center_size(pos2(r.min.x + 15.0, r.center().y), vec2(16.0, 16.0)), icon, c, a);
    ui.painter().text(pos2(r.min.x + 27.0, r.center().y), Align2::LEFT_CENTER, text, regular(12.0), if enabled { t.text } else { t.text_disabled });
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, text));
    resp
}

/// A menu entry with an icon.
pub fn menu_item(ui: &mut Ui, icon: &str, text: &str, shortcut: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let w = ui.available_width().max(200.0);
    let (r, resp) = ui.allocate_exact_size(vec2(w, 26.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::same(4), t.hover);
    }
    if !icon.is_empty() {
        icons::paint(ui.painter(), Rect::from_center_size(pos2(r.min.x + 14.0, r.center().y), vec2(16.0, 16.0)), icon, t.icon, t.accent);
    }
    ui.painter().text(pos2(r.min.x + 32.0, r.center().y), Align2::LEFT_CENTER, text, regular(12.5), t.text);
    if !shortcut.is_empty() {
        ui.painter().text(pos2(r.max.x - 8.0, r.center().y), Align2::RIGHT_CENTER, shortcut, regular(11.5), t.text_dim);
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, text));
    resp
}

/// A circle with initials (or a picture, one day).
pub fn avatar(ui: &Ui, center: egui::Pos2, radius: f32, name: &str, key: &str) {
    let col = crate::theme::person_color(if key.is_empty() { name } else { key });
    ui.painter().circle_filled(center, radius, col);
    let init = crate::theme::initials(if name.trim().is_empty() { key } else { name });
    ui.painter().text(center, Align2::CENTER_CENTER, init, medium(radius * 0.8), Color32::WHITE);
}

/// A primary (filled, app colour) button.
pub fn primary(ui: &mut Ui, text: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    ui.add(egui::Button::new(egui::RichText::new(text).color(t.on_accent).font(medium(12.5))).fill(t.accent).min_size(vec2(80.0, 28.0)))
}

/// A secondary (outlined) button.
pub fn secondary(ui: &mut Ui, text: &str) -> Response {
    ui.add(egui::Button::new(egui::RichText::new(text).font(regular(12.5))).min_size(vec2(80.0, 28.0)))
}

/// A thin bar with a message (info, error) and optional action.
pub fn info_bar(ui: &mut Ui, icon: &str, text: &str, error: bool, action: Option<&str>) -> Option<Response> {
    let t = Tokens::get(ui.ctx());
    let mut out = None;
    egui::Frame::new()
        .fill(if error { t.error_bar } else { t.info_bar })
        .inner_margin(egui::Margin::symmetric(10, 6))
        .corner_radius(CornerRadius::same(4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                let c = if t.dark { t.info_bar_text } else { t.text };
                icons::paint(ui.painter(), r, icon, c, if error { t.high } else { t.accent });
                ui.add(egui::Label::new(egui::RichText::new(text).color(if error { t.text } else { t.info_bar_text })).wrap());
                if let Some(a) = action {
                    out = Some(ui.link(a));
                }
            });
        });
    out
}

/// "2.4 MB" and so on.
pub fn size_text(bytes: i64) -> String {
    let b = bytes.max(0) as f64;
    if b < 1024.0 {
        format!("{} bytes", bytes.max(0))
    } else if b < 1024.0 * 1024.0 {
        format!("{:.0} KB", (b / 1024.0).ceil())
    } else if b < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} MB", b / 1024.0 / 1024.0)
    } else {
        format!("{:.1} GB", b / 1024.0 / 1024.0 / 1024.0)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn sizes_read_naturally() {
        assert_eq!(super::size_text(512), "512 bytes");
        assert_eq!(super::size_text(1500), "2 KB");
        assert_eq!(super::size_text(4_500_000), "4.3 MB");
    }
}
