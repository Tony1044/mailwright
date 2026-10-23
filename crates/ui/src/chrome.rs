//! The title row (quick access, search, account), the status bar and the info bars under the ribbon.

use egui::{Align2, Rect, Sense, Ui, pos2, vec2};

use crate::app::{MailApp, View};
use crate::icons;
use crate::theme::{Tokens, regular};
use crate::widgets;

pub fn title_bar(app: &mut MailApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let full = ui.max_rect();
    ui.horizontal_centered(|ui| {
        ui.add_space(8.0);
        let (r, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
        crate::app_icon::paint(ui.painter(), r);
        ui.add_space(4.0);
        if widgets::icon_button(ui, "send_receive", "Send/Receive All Folders (F9)", app.session.is_some()).clicked() {
            app.sync_all();
        }
        let offline = app.settings.work_offline;
        let undo_tip = if offline { "Work Offline is on" } else { "Work Offline" };
        if widgets::icon_button(ui, if offline { "offline" } else { "online" }, undo_tip, true).clicked() {
            app.set_work_offline(!offline);
        }
    });
    // The search box, centred in the window.
    let w = (full.width() * 0.4).clamp(260.0, 620.0);
    let sr = Rect::from_center_size(pos2(full.center().x, full.center().y), vec2(w, 26.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(sr).layout(egui::Layout::left_to_right(egui::Align::Center)));
    search_box(app, &mut child, w);
    // The account, on the right.
    let name = app.session.as_ref().map(|s| if s.name.is_empty() { s.address.clone() } else { s.name.clone() }).unwrap_or_default();
    let key = app.session.as_ref().map(|s| s.address.clone()).unwrap_or_default();
    let ar = Rect::from_center_size(pos2(full.max.x - 22.0, full.center().y), vec2(26.0, 26.0));
    let resp = ui.interact(ar, ui.id().with("account"), Sense::click());
    widgets::avatar(ui, ar.center(), 13.0, &name, &key);
    let resp = resp.on_hover_text(format!("{name}\n{key}"));
    if resp.clicked() {
        app.backstage = Some(crate::app::Backstage::Info);
    }
    let galley = ui.painter().layout_no_wrap(name, regular(12.0), t.text_dim);
    let x = ar.min.x - 8.0 - galley.size().x;
    if x > sr.max.x + 12.0 {
        ui.painter().galley(pos2(x, full.center().y - galley.size().y / 2.0), galley, t.text_dim);
    }
}

fn search_box(app: &mut MailApp, ui: &mut Ui, w: f32) {
    let t = Tokens::get(ui.ctx());
    let id = egui::Id::new("search_box");
    let r = ui.max_rect();
    let focused = ui.memory(|m| m.has_focus(id));
    ui.painter().rect(r, 5.0, t.input, egui::Stroke::new(1.0, if focused { t.accent } else { t.border_strong }), egui::StrokeKind::Inside);
    icons::paint(ui.painter(), Rect::from_center_size(pos2(r.min.x + 16.0, r.center().y), vec2(15.0, 15.0)), "search", t.text_dim, t.accent);
    ui.add_space(30.0);
    let hint = if app.view == View::Calendar { "Search mail" } else { "Search" };
    let resp = ui.add(egui::TextEdit::singleline(&mut app.mail.search).id(id).frame(egui::Frame::NONE).hint_text(hint).desired_width(w - 64.0).margin(vec2(0.0, 3.0)));
    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        app.start_search();
    }
    if focused && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.clear_search();
        ui.memory_mut(|m| m.surrender_focus(id));
    }
    if !app.mail.search.is_empty() || app.mail.searching.is_some() {
        let cr = Rect::from_center_size(pos2(r.max.x - 15.0, r.center().y), vec2(20.0, 20.0));
        let c = ui.interact(cr, id.with("clear"), Sense::click());
        if c.hovered() {
            ui.painter().rect_filled(cr, 4.0, t.hover);
        }
        icons::paint(ui.painter(), cr.shrink(4.0), "close", t.text_dim, t.accent);
        if c.on_hover_text("Close search").clicked() {
            app.clear_search();
        }
    }
    if ui.input_mut(|i| i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::E))) {
        ui.memory_mut(|m| m.request_focus(id));
    }
}

pub fn status_bar(app: &mut MailApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let r = ui.max_rect();
    ui.painter().line_segment([r.left_top(), r.right_top()], egui::Stroke::new(1.0, t.border));
    let left = match app.view {
        View::Mail => {
            let items = app.mail.list.len();
            let unread = app.mail.list.iter().filter(|m| !m.is_read).count();
            if app.mail.searching.is_some() {
                format!("Results: {items}")
            } else if unread > 0 {
                format!("Items: {items}    Unread: {unread}")
            } else {
                format!("Items: {items}")
            }
        }
        View::Calendar => app.cal.range_title(&app.settings),
    };
    ui.painter().text(pos2(r.min.x + 10.0, r.center().y), Align2::LEFT_CENTER, left, regular(11.5), t.text_dim);
    let (connected, queued, demo) = app.session.as_ref().map(|s| (s.connected, s.queued, s.demo)).unwrap_or_default();
    let right = if app.settings.work_offline {
        "Working Offline".to_string()
    } else if demo && connected {
        "Connected to the sample mailbox".into()
    } else if connected {
        "Connected to: Microsoft Exchange".into()
    } else {
        "Disconnected".into()
    };
    let galley = ui.painter().layout_no_wrap(right, regular(11.5), t.text_dim);
    let rx = r.max.x - 10.0 - galley.size().x;
    ui.painter().galley(pos2(rx, r.center().y - galley.size().y / 2.0), galley, t.text_dim);
    let dot = pos2(rx - 10.0, r.center().y);
    ui.painter().circle_filled(dot, 4.0, if connected && !app.settings.work_offline { t.green } else { t.text_disabled });
    let mut middle = app.status.clone();
    if queued > 0 {
        middle = format!("{middle}    {queued} change{} waiting to send", if queued == 1 { "" } else { "s" });
    }
    let mw = ui.painter().layout_no_wrap(middle.clone(), regular(11.5), t.text_dim).size().x;
    let mx = (r.center().x - mw / 2.0).max(r.min.x + 200.0);
    if mx + mw < rx - 24.0 {
        ui.painter().text(pos2(mx, r.center().y), Align2::LEFT_CENTER, middle, regular(11.5), t.text_dim);
    }
}

/// Sign-in prompts and errors, under the ribbon.
pub fn banners(app: &mut MailApp, ui: &mut Ui) {
    let needs = app.session.as_ref().and_then(|s| s.needs_sign_in.clone());
    if needs.is_none() && app.errors.is_empty() {
        return;
    }
    let t = Tokens::get(ui.ctx());
    egui::Panel::top("banners").frame(egui::Frame::new().fill(t.ribbon).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| {
        if let Some(m) = needs {
            if let Some(resp) = widgets::info_bar(ui, "lock", &format!("{m} You're seeing what's saved on this computer."), false, Some("Sign in again")) {
                if resp.clicked() {
                    if let Some(s) = &app.session {
                        app.signin.client_id = s.account.client_id.clone();
                        app.signin.tenant = s.account.tenant.clone();
                    }
                    app.begin_sign_in(false);
                }
            }
        }
        let mut dismiss = None;
        for (i, e) in app.errors.iter().enumerate().take(3) {
            if let Some(resp) = widgets::info_bar(ui, "info", e, true, Some("Dismiss")) {
                if resp.clicked() {
                    dismiss = Some(i);
                }
            }
        }
        if let Some(i) = dismiss {
            app.errors.remove(i);
        }
    });
}
