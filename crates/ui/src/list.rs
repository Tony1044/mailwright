//! The message list: Focused/Other, date groups, three-line rows, multi-select, drag to a folder.

use chrono::{Datelike, Local, TimeZone};
use egui::{Align2, Rect, Sense, Ui, pos2, vec2};
use mailwright_store::{ListFilter, MessageRow, SortBy};

use crate::app::MailApp;
use crate::folders::DragMessages;
use crate::icons;
use crate::theme::{Tokens, medium, regular, semibold};
use crate::widgets;

const HEADER_H: f32 = 28.0;
const GROUP_H: f32 = 26.0;

enum Item {
    Group(String),
    Msg(usize),
}

/// The list's rows and date groups, worked out once per reload rather than every frame: a
/// big folder has thousands of messages, and the window redraws many times a second.
#[derive(Default)]
pub struct ListLayout {
    key: Option<(u64, SortBy, bool, chrono::NaiveDate, usize)>,
    items: Vec<Item>,
    heights: Vec<f32>,
    total: f32,
}

fn layout(app: &mut MailApp, row_h: f32, now: chrono::DateTime<Local>) {
    let key = (app.mail.list_gen, app.mail.sort, app.settings.message_preview, now.date_naive(), app.mail.list.len());
    if app.mail.layout.key == Some(key) {
        return;
    }
    let mut items: Vec<Item> = Vec::with_capacity(app.mail.list.len() + 8);
    let mut last_group = String::new();
    let outgoing: std::collections::HashSet<&str> =
        app.mail.folders.iter().filter(|f| matches!(f.well_known.as_deref(), Some("sentitems" | "drafts" | "outbox"))).map(|f| f.id.as_str()).collect();
    for (i, m) in app.mail.list.iter().enumerate() {
        let g = match app.mail.sort {
            SortBy::Date => date_group(m.received, now),
            SortBy::From => {
                let f = if outgoing.contains(m.folder_id.as_str()) { m.to.first().map(|a| a.label().to_string()).unwrap_or_default() } else { m.from.label().to_string() };
                if f.is_empty() { "(none)".into() } else { f }
            }
            SortBy::Importance => match m.importance.as_str() {
                "high" => "Importance: High".into(),
                "low" => "Importance: Low".into(),
                _ => "Importance: Normal".into(),
            },
            SortBy::Flag => match m.flag.as_str() {
                "flagged" => "Flagged".into(),
                "complete" => "Completed".into(),
                _ => "No flag".into(),
            },
            SortBy::Subject => String::new(),
        };
        if !g.is_empty() && g != last_group {
            items.push(Item::Group(g.clone()));
            last_group = g;
        }
        items.push(Item::Msg(i));
    }
    let heights: Vec<f32> = items.iter().map(|it| if matches!(it, Item::Group(_)) { GROUP_H } else { row_h }).collect();
    let total = heights.iter().sum();
    app.mail.layout = ListLayout { key: Some(key), items, heights, total };
}

pub fn local(ts: i64) -> chrono::DateTime<Local> {
    Local.timestamp_opt(ts, 0).single().unwrap_or_else(Local::now)
}

/// The date group a message falls in, as Outlook names them.
pub fn date_group(ts: i64, now: chrono::DateTime<Local>) -> String {
    let d = local(ts).date_naive();
    let today = now.date_naive();
    let days = (today - d).num_days();
    let week_start = today - chrono::Duration::days(today.weekday().num_days_from_monday() as i64);
    if d > today {
        "Future".into()
    } else if days == 0 {
        "Today".into()
    } else if days == 1 {
        "Yesterday".into()
    } else if d >= week_start {
        d.format("%A").to_string()
    } else if d >= week_start - chrono::Duration::days(7) {
        "Last Week".into()
    } else if d >= week_start - chrono::Duration::days(14) {
        "Two Weeks Ago".into()
    } else if d >= week_start - chrono::Duration::days(21) {
        "Three Weeks Ago".into()
    } else if d.year() == today.year() && d.month() + 1 == today.month() || (today.month() == 1 && d.month() == 12 && d.year() + 1 == today.year()) {
        "Last Month".into()
    } else {
        "Older".into()
    }
}

/// The time shown on a row.
pub fn short_time(ts: i64, now: chrono::DateTime<Local>) -> String {
    let t = local(ts);
    let days = (now.date_naive() - t.date_naive()).num_days();
    if days == 0 {
        t.format("%H:%M").to_string()
    } else if (0..7).contains(&days) {
        t.format("%a %H:%M").to_string()
    } else if t.year() == now.year() {
        t.format("%a %d/%m").to_string()
    } else {
        t.format("%d/%m/%Y").to_string()
    }
}

fn is_outgoing(app: &MailApp, m: &MessageRow) -> bool {
    app.mail.folders.iter().find(|f| f.id == m.folder_id).is_some_and(|f| matches!(f.well_known.as_deref(), Some("sentitems" | "drafts" | "outbox")))
}

pub fn list(app: &mut MailApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    header(app, ui);
    let row_h = if app.settings.message_preview { 64.0 } else { 46.0 };
    let now = Local::now();
    layout(app, row_h, now);
    let total = app.mail.layout.total;
    if app.mail.list.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            let msg = if app.mail.searching.is_some() {
                "We didn't find anything in what's saved on this computer."
            } else if app.current_folder().is_some_and(|f| !f.synced) {
                "Getting your mail\u{2026}"
            } else if app.mail.filter != ListFilter::All {
                "Nothing matches this filter."
            } else {
                "We didn't find anything to show here."
            };
            ui.label(egui::RichText::new(msg).color(t.text_dim));
            if app.mail.searching.is_some() && app.session().is_some_and(|s| s.connected) && app.mail.server_results.is_none() && ui.link("Search the server").clicked() {
                app.search_server();
            }
        });
        return;
    }
    let scroll_to = app.mail.scroll_to_selected.then(|| app.mail.selected.last().cloned()).flatten();
    app.mail.scroll_to_selected = false;
    let mut clicked: Option<(String, egui::Modifiers, bool)> = None;
    let mut action: Option<RowAction> = None;
    egui::ScrollArea::vertical().id_salt(("list", app.mail.folder.clone(), app.mail.searching.is_some())).auto_shrink([false, false]).show_viewport(ui, |ui, viewport| {
        ui.set_height(total);
        let origin = ui.min_rect().min;
        let width = ui.available_width();
        let mut y = 0.0;
        let layout = std::mem::take(&mut app.mail.layout);
        for (k, it) in layout.items.iter().enumerate() {
            let h = layout.heights[k];
            let rect = Rect::from_min_size(pos2(origin.x, origin.y + y), vec2(width, h));
            let visible = y + h >= viewport.min.y && y <= viewport.max.y;
            if let (Item::Msg(i), Some(target)) = (it, &scroll_to) {
                if app.mail.list.get(*i).is_some_and(|m| &m.id == target) {
                    ui.scroll_to_rect(rect, None);
                }
            }
            if !visible && y > viewport.max.y && scroll_to.is_none() {
                break;
            }
            if visible {
                match it {
                    Item::Group(g) => {
                        ui.painter().text(pos2(rect.min.x + 12.0, rect.center().y + 2.0), Align2::LEFT_CENTER, g, semibold(12.0), t.text_dim);
                        ui.painter().line_segment([pos2(rect.min.x + 8.0, rect.max.y - 1.0), pos2(rect.max.x - 8.0, rect.max.y - 1.0)], egui::Stroke::new(1.0, t.border));
                    }
                    Item::Msg(i) => {
                        if let Some(m) = app.mail.list.get(*i).cloned() {
                            let (r, a) = row(app, ui, rect, &m, now);
                            if let Some(r) = r {
                                clicked = Some(r);
                            }
                            if a.is_some() {
                                action = a;
                            }
                        }
                    }
                }
            }
            y += h;
        }
        if app.mail.layout.key.is_none() {
            app.mail.layout = layout;
        }
    });
    if let Some((id, mods, double)) = clicked {
        app.mail.list_has_focus = true;
        ui.memory_mut(|m| m.stop_text_input());
        if double {
            app.open_message_window(&id);
        } else {
            app.select_message(&id, mods.command, mods.shift);
        }
    }
    if let Some(a) = action {
        match a {
            RowAction::Delete(id) => {
                app.mail.selected = vec![id];
                app.delete_selected();
            }
            RowAction::Flag(id, status) => app.act(mailwright_store::Action::SetFlag { id, status }),
            RowAction::Read(id, read) => app.act(mailwright_store::Action::SetRead { id, read }),
        }
    }
}

enum RowAction {
    Delete(String),
    Flag(String, String),
    Read(String, bool),
}

fn header(app: &mut MailApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEADER_H + 6.0), Sense::hover());
    ui.painter().line_segment([pos2(r.min.x, r.max.y - 0.5), pos2(r.max.x, r.max.y - 0.5)], egui::Stroke::new(1.0, t.border));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r.shrink2(vec2(8.0, 3.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let ui = &mut child;
    if let Some(q) = app.mail.searching.clone() {
        let scope = if app.mail.search_all { "All Mailboxes" } else { "Current Folder" };
        ui.label(egui::RichText::new(format!("Results for \u{201c}{q}\u{201d}")).font(semibold(12.5)));
        let resp = ui.add(egui::Button::new(egui::RichText::new(format!("{scope} \u{25be}")).font(regular(11.5))).frame(false));
        egui::Popup::menu(&resp).show(|ui| {
            if ui.selectable_label(!app.mail.search_all, "Current Folder").clicked() {
                app.mail.search_all = false;
                app.mail.list_dirty = true;
                ui.close();
            }
            if ui.selectable_label(app.mail.search_all, "All Mailboxes").clicked() {
                app.mail.search_all = true;
                app.mail.list_dirty = true;
                ui.close();
            }
        });
        let online = app.session().is_some_and(|s| s.connected);
        if online && app.mail.server_results.is_none() && ui.link("Include results from the server").clicked() {
            app.search_server();
        }
        return;
    }
    if app.focused_applies() && app.mail.filter == ListFilter::All {
        for (other, label) in [(false, "Focused"), (true, "Other")] {
            let on = app.mail.other_tab == other;
            let font = if on { semibold(13.0) } else { medium(13.0) };
            // Unread mail on the other tab shows as a count, so nothing new goes unnoticed there.
            let label = if !on && app.mail.other_half_unread > 0 { format!("{label} ({})", app.mail.other_half_unread) } else { label.to_string() };
            let label = label.as_str();
            let w = ui.painter().layout_no_wrap(label.into(), font.clone(), t.text).size().x + 16.0;
            let (rr, resp) = ui.allocate_exact_size(vec2(w, HEADER_H), Sense::click());
            let colour = if on || app.mail.other_half_unread > 0 { t.accent_text } else { t.text_dim };
            ui.painter().text(rr.center(), Align2::CENTER_CENTER, label, font, colour);
            if on {
                ui.painter().line_segment([pos2(rr.min.x + 6.0, rr.max.y - 1.0), pos2(rr.max.x - 6.0, rr.max.y - 1.0)], egui::Stroke::new(2.5, t.accent));
            }
            resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, label));
            if resp.clicked() && !on {
                app.mail.other_tab = other;
                app.mail.list_dirty = true;
                app.mail.selected.clear();
                app.mail.reading = None;
            }
        }
    } else {
        let name = app.current_folder().map(|f| f.name.clone()).unwrap_or_default();
        ui.label(egui::RichText::new(name).font(semibold(13.0)));
        if app.mail.filter != ListFilter::All {
            let label = match app.mail.filter {
                ListFilter::Unread => "Unread",
                ListFilter::Flagged => "Flagged",
                ListFilter::Attachments => "Has Attachments",
                ListFilter::ToMe => "To Me",
                _ => "",
            };
            if ui.add(egui::Button::new(egui::RichText::new(format!("{label} \u{2715}")).font(regular(11.5))).small()).on_hover_text("Clear the filter").clicked() {
                app.mail.filter = ListFilter::All;
                app.mail.list_dirty = true;
            }
        }
    }
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let sort = match app.mail.sort {
            SortBy::Date => "By Date",
            SortBy::From => "By From",
            SortBy::Subject => "By Subject",
            SortBy::Importance => "By Importance",
            SortBy::Flag => "By Flag",
        };
        let arrow = if app.mail.ascending { "\u{2191}" } else { "\u{2193}" };
        if ui.add(egui::Button::new(egui::RichText::new(arrow).font(regular(12.0))).frame(false)).on_hover_text("Reverse sort order").clicked() {
            app.mail.ascending = !app.mail.ascending;
            app.mail.list_dirty = true;
        }
        let resp = ui.add(egui::Button::new(egui::RichText::new(format!("{sort} \u{25be}")).font(regular(11.5)).color(t.text_dim)).frame(false));
        egui::Popup::menu(&resp).show(|ui| {
            for (s, label) in [(SortBy::Date, "Date"), (SortBy::From, "From"), (SortBy::Subject, "Subject"), (SortBy::Importance, "Importance"), (SortBy::Flag, "Flag Status")] {
                if ui.selectable_label(app.mail.sort == s, label).clicked() {
                    app.mail.sort = s;
                    app.mail.ascending = matches!(s, SortBy::From | SortBy::Subject);
                    app.mail.list_dirty = true;
                    ui.close();
                }
            }
        });
    });
}

fn row(app: &mut MailApp, ui: &mut Ui, rect: Rect, m: &MessageRow, now: chrono::DateTime<Local>) -> (Option<(String, egui::Modifiers, bool)>, Option<RowAction>) {
    let t = Tokens::get(ui.ctx());
    let id = egui::Id::new(("msg", &m.id));
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    let selected = app.mail.selected.contains(&m.id);
    let fill = if selected {
        if app.mail.list_has_focus { t.selected } else { t.selected_unfocused }
    } else if resp.hovered() {
        t.hover
    } else {
        t.list
    };
    let p = ui.painter_at(rect);
    p.rect_filled(rect.shrink2(vec2(4.0, 1.0)), 4.0, fill);
    if !m.is_read {
        p.rect_filled(Rect::from_min_size(pos2(rect.min.x + 4.0, rect.min.y + 4.0), vec2(3.0, rect.height() - 8.0)), 1.5, t.accent);
    }
    let x0 = rect.min.x + 16.0;
    let right = rect.max.x - 12.0;
    let outgoing = is_outgoing(app, m);
    let who = if outgoing {
        let names: Vec<String> = m.to.iter().chain(m.cc.iter()).map(|a| a.label().to_string()).collect();
        if names.is_empty() { "(no recipients)".to_string() } else { names.join("; ") }
    } else if m.from.label().is_empty() {
        "(unknown sender)".to_string()
    } else {
        m.from.label().to_string()
    };
    let y1 = rect.min.y + 13.0;
    let y2 = rect.min.y + 31.0;
    let y3 = rect.min.y + 48.0;
    // Line 1: who, and the time (or quick actions while hovered).
    let hovered = resp.hovered() || ui.rect_contains_pointer(rect);
    let time = short_time(m.received, now);
    let tw = p.layout_no_wrap(time.clone(), regular(11.5), t.text_dim).size().x;
    let mut action = None;
    let actions_w = if hovered { 50.0 } else { 0.0 };
    if hovered {
        let dr = Rect::from_center_size(pos2(right - 10.0, y1), vec2(20.0, 20.0));
        let rr = Rect::from_center_size(pos2(right - 34.0, y1), vec2(20.0, 20.0));
        let d = ui.interact(dr, id.with("del"), Sense::click()).on_hover_text("Delete");
        let r = ui.interact(rr, id.with("read"), Sense::click()).on_hover_text(if m.is_read { "Mark as Unread" } else { "Mark as Read" });
        for (rect_, resp_, icon) in [(dr, &d, "delete"), (rr, &r, if m.is_read { "unread" } else { "read" })] {
            if resp_.hovered() {
                p.rect_filled(rect_, 4.0, t.pressed);
            }
            icons::paint(&p, rect_.shrink(2.0), icon, t.icon, t.accent);
        }
        if d.clicked() {
            action = Some(RowAction::Delete(m.id.clone()));
        }
        if r.clicked() {
            action = Some(RowAction::Read(m.id.clone(), !m.is_read));
        }
    } else {
        p.text(pos2(right, y1), Align2::RIGHT_CENTER, &time, regular(11.5), if m.is_read { t.text_dim } else { t.accent_text });
    }
    let who_font = if m.is_read { regular(13.0) } else { semibold(13.0) };
    let who_max = right - x0 - tw.max(actions_w) - 10.0;
    let prefix = if m.is_draft && m.folder_id != app.folder_wk("sentitems").unwrap_or_default() {
        p.text(pos2(x0, y1), Align2::LEFT_CENTER, "[Draft]", regular(13.0), t.high);
        p.layout_no_wrap("[Draft] ".into(), regular(13.0), t.high).size().x
    } else {
        0.0
    };
    p.text(pos2(x0 + prefix, y1), Align2::LEFT_CENTER, elide(&p, &who, who_font.clone(), who_max - prefix), who_font, t.text);
    // Line 2: subject, and the icons.
    let mut icons_x = right;
    let flag_rect = Rect::from_center_size(pos2(icons_x - 8.0, y2), vec2(18.0, 18.0));
    let fr = ui.interact(flag_rect, id.with("flag"), Sense::click()).on_hover_text(if m.flagged() { "Mark Complete" } else { "Flag this message" });
    if m.flagged() || m.completed() || hovered {
        let (icon, col) = if m.flagged() {
            ("flag", t.flag)
        } else if m.completed() {
            ("flag_complete", t.green)
        } else {
            ("flag_outline", t.text_dim)
        };
        icons::paint(&p, flag_rect.shrink(2.0), icon, if m.flagged() { t.flag } else { col }, col);
    }
    if fr.clicked() {
        action = Some(RowAction::Flag(m.id.clone(), if m.flagged() { "complete".into() } else { "flagged".into() }));
    }
    icons_x -= 22.0;
    let mut small_icon = |name: &str, c: egui::Color32| {
        icons::paint(&p, Rect::from_center_size(pos2(icons_x - 7.0, y2), vec2(14.0, 14.0)), name, t.icon, c);
        icons_x -= 18.0;
    };
    if m.has_attachments {
        small_icon("attach", t.icon);
    }
    if m.importance == "high" {
        small_icon("important", t.high);
    } else if m.importance == "low" {
        small_icon("low", t.accent);
    }
    if m.is_meeting {
        small_icon("calendar", t.accent);
    }
    if m.pending {
        small_icon("sync", t.text_dim);
    }
    for c in m.categories.iter().take(3) {
        let cr = Rect::from_center_size(pos2(icons_x - 6.0, y2), vec2(10.0, 10.0));
        p.rect_filled(cr, 2.0, crate::theme::category_color(c));
        icons_x -= 14.0;
    }
    let subject = if m.subject.trim().is_empty() { "(no subject)".to_string() } else { m.subject.clone() };
    let sub_font = if m.is_read { regular(12.5) } else { semibold(12.5) };
    let sub_col = if m.is_read { t.text } else { t.accent_text };
    p.text(pos2(x0, y2), Align2::LEFT_CENTER, elide(&p, &subject, sub_font.clone(), icons_x - x0 - 6.0), sub_font, sub_col);
    // Line 3: preview.
    if app.settings.message_preview && rect.height() > 50.0 {
        let preview = m.preview.split_whitespace().collect::<Vec<_>>().join(" ");
        let preview = if preview.is_empty() { "This message has no content.".to_string() } else { preview };
        p.text(pos2(x0, y3), Align2::LEFT_CENTER, elide(&p, &preview, regular(12.0), right - x0), regular(12.0), t.text_dim);
    }
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, format!("{who} {subject}")));

    // Dragging to a folder.
    if resp.drag_started() {
        let ids = if selected { app.mail.selected.clone() } else { vec![m.id.clone()] };
        egui::DragAndDrop::set_payload(ui.ctx(), DragMessages(ids));
    }
    if resp.dragged() {
        if let (Some(pos), Some(payload)) = (ui.ctx().pointer_interact_pos(), egui::DragAndDrop::payload::<DragMessages>(ui.ctx())) {
            let n = payload.0.len();
            let text = if n == 1 { "Move 1 item".to_string() } else { format!("Move {n} items") };
            egui::Area::new(egui::Id::new("drag_badge")).order(egui::Order::Tooltip).fixed_pos(pos + vec2(12.0, 8.0)).interactable(false).show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(text);
                });
            });
        }
    }
    let mut clicked = None;
    if resp.clicked() {
        let mods = ui.input(|i| i.modifiers);
        clicked = Some((m.id.clone(), mods, false));
    }
    if resp.double_clicked() {
        clicked = Some((m.id.clone(), egui::Modifiers::NONE, true));
    }
    if resp.secondary_clicked() && !selected {
        app.select_message(&m.id, false, false);
    }
    resp.context_menu(|ui| context_menu(app, ui));
    (clicked, action)
}

pub fn elide(p: &egui::Painter, s: &str, font: egui::FontId, max_w: f32) -> String {
    if max_w <= 10.0 {
        return String::new();
    }
    let s = s.replace(['\n', '\r', '\t'], " ");
    if p.layout_no_wrap(s.clone(), font.clone(), egui::Color32::WHITE).size().x <= max_w {
        return s;
    }
    let chars: Vec<char> = s.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let cand: String = chars[..mid].iter().collect::<String>() + "\u{2026}";
        if p.layout_no_wrap(cand, font.clone(), egui::Color32::WHITE).size().x <= max_w {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    chars[..lo].iter().collect::<String>().trim_end().to_string() + "\u{2026}"
}

pub fn context_menu(app: &mut MailApp, ui: &mut Ui) {
    ui.set_min_width(230.0);
    let one = app.mail.selected.len() == 1;
    if one {
        if widgets::menu_item(ui, "reply", "Reply", "Ctrl+R").clicked() {
            app.reply("reply");
            ui.close();
        }
        if widgets::menu_item(ui, "reply_all", "Reply All", "Ctrl+Shift+R").clicked() {
            app.reply("replyAll");
            ui.close();
        }
        if widgets::menu_item(ui, "forward", "Forward", "Ctrl+F").clicked() {
            app.reply("forward");
            ui.close();
        }
        ui.separator();
    }
    if widgets::menu_item(ui, "read", "Mark as Read", "Ctrl+Q").clicked() {
        app.set_read_selected(true);
        ui.close();
    }
    if widgets::menu_item(ui, "unread", "Mark as Unread", "Ctrl+U").clicked() {
        app.set_read_selected(false);
        ui.close();
    }
    ui.menu_button("Categorize", |ui| crate::ribbon::categorize_menu(app, ui));
    ui.menu_button("Follow Up", |ui| crate::ribbon::follow_up_menu(app, ui));
    ui.separator();
    ui.menu_button("Move", |ui| crate::ribbon::move_menu(app, ui));
    if widgets::menu_item(ui, "archive", "Archive", "Backspace").clicked() {
        app.archive_selected();
        ui.close();
    }
    if widgets::menu_item(ui, "delete", "Delete", "Del").clicked() {
        app.delete_selected();
        ui.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_follow_outlooks_names() {
        let now = Local.with_ymd_and_hms(2026, 10, 23, 15, 0, 0).single().unwrap(); // a Friday
        let ts = |y, m, d| Local.with_ymd_and_hms(y, m, d, 9, 0, 0).single().unwrap().timestamp();
        assert_eq!(date_group(ts(2026, 10, 23), now), "Today");
        assert_eq!(date_group(ts(2026, 10, 22), now), "Yesterday");
        assert_eq!(date_group(ts(2026, 10, 20), now), "Tuesday");
        assert_eq!(date_group(ts(2026, 10, 15), now), "Last Week");
        assert_eq!(date_group(ts(2026, 10, 8), now), "Two Weeks Ago");
        assert_eq!(date_group(ts(2026, 10, 1), now), "Three Weeks Ago");
        assert_eq!(date_group(ts(2026, 9, 20), now), "Last Month");
        assert_eq!(date_group(ts(2026, 3, 1), now), "Older");
        assert_eq!(short_time(ts(2026, 10, 23), now), "09:00");
        assert_eq!(short_time(ts(2026, 10, 20), now), "Tue 09:00");
        assert_eq!(short_time(ts(2025, 12, 1), now), "01/12/2025");
    }
}
