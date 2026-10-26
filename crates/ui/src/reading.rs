//! The reading pane (and message windows): header, meeting box, attachments and the body.

use egui::{Align2, Rect, Sense, Ui, pos2, vec2};
use mailwright_store::{Action, AttachmentRow};

use crate::app::{MailApp, Reading};
use crate::icons;
use crate::render::{self, BodyAction, RenderCtx};
use crate::theme::{Tokens, medium, regular, semibold};
use crate::widgets;

enum Act {
    Reply(&'static str),
    Delete,
    Respond(&'static str, bool),
    Attachment(AttachmentRow, bool),
    SaveAll,
    Link(String),
    AllowRemote(bool),
    Flag(&'static str),
    EditDraft,
    OpenEvent(String),
    JoinMeeting(String),
}

pub fn pane(app: &mut MailApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(reading) = app.mail.reading.take() else {
        empty(app, ui, &t);
        return;
    };
    let acts = message_view(app, ui, &reading, true);
    app.mail.reading = Some(reading);
    for a in acts {
        perform(app, a);
    }
}

fn empty(app: &mut MailApp, ui: &mut Ui, t: &Tokens) {
    ui.centered_and_justified(|ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.35);
            let (r, _) = ui.allocate_exact_size(vec2(64.0, 64.0), Sense::hover());
            icons::paint(ui.painter(), r, "mail", t.text_disabled, t.text_disabled);
            ui.add_space(8.0);
            let n = app.mail.selected.len();
            if n > 1 {
                ui.label(egui::RichText::new(format!("{n} items selected")).font(semibold(15.0)));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add_space((ui.available_width() - 260.0).max(0.0) / 2.0);
                    if widgets::icon_text(ui, "delete", "Delete", true).clicked() {
                        app.delete_selected();
                    }
                    if widgets::icon_text(ui, "read", "Mark as read", true).clicked() {
                        app.set_read_selected(true);
                    }
                    if widgets::icon_text(ui, "flag", "Flag", true).clicked() {
                        app.flag_selected("flagged");
                    }
                });
            } else {
                ui.label(egui::RichText::new("Select an item to read").font(semibold(15.0)).color(t.text_dim));
                ui.label(egui::RichText::new("Nothing is selected").color(t.text_dim));
            }
        });
    });
}

fn perform(app: &mut MailApp, a: Act) {
    match a {
        Act::Reply(kind) => app.reply(kind),
        Act::Delete => app.delete_selected(),
        Act::Respond(r, send) => {
            if let Some(ev) = app.mail.reading.as_ref().and_then(|r| r.event.clone()) {
                app.act(Action::RespondEvent { id: ev.id, response: r.to_string(), comment: String::new(), send });
                app.status = match r {
                    "accept" => "Accepted".into(),
                    "tentative" => "Tentatively accepted".into(),
                    _ => "Declined".into(),
                };
            }
        }
        Act::Attachment(att, open) => app.save_attachment(&att, open),
        Act::SaveAll => {
            let atts: Vec<AttachmentRow> = app.mail.reading.as_ref().map(|r| r.attachments.iter().filter(|a| !a.inline).cloned().collect()).unwrap_or_default();
            if let Some(dir) = rfd::FileDialog::new().set_title("Save All Attachments").pick_folder() {
                for a in atts {
                    let path = dir.join(crate::compose::safe_file_name(&a.name));
                    app.send(mailwright_sync::Command::SaveAttachment { message_id: a.message_id.clone(), attachment_id: a.id.clone(), path, open: false });
                }
            }
        }
        Act::Link(url) => {
            if let Some(addr) = url.strip_prefix("mailto:") {
                let addr = addr.split('?').next().unwrap_or(addr).to_string();
                app.new_mail_to(&addr);
            } else if let Err(e) = open::that_detached(&url) {
                app.errors.push(format!("Couldn't open the link: {e}"));
            }
        }
        Act::AllowRemote(always) => {
            if let Some(r) = &mut app.mail.reading {
                r.allow_remote = true;
                if always {
                    let s = r.row.from.address.to_ascii_lowercase();
                    if !s.is_empty() && !app.settings.safe_senders.contains(&s) {
                        app.settings.safe_senders.push(s);
                    }
                }
            }
            if always {
                app.save_settings();
            }
            app.fetch_remote_images();
        }
        Act::Flag(s) => app.flag_selected(s),
        Act::EditDraft => {
            if let Some(r) = app.mail.reading.as_ref().map(|r| r.row.clone()) {
                app.edit_draft(&r);
            }
        }
        Act::OpenEvent(id) => app.open_event(&id),
        Act::JoinMeeting(url) => {
            let _ = open::that_detached(url);
        }
    }
}

fn message_view(app: &mut MailApp, ui: &mut Ui, r: &Reading, in_pane: bool) -> Vec<Act> {
    let t = Tokens::get(ui.ctx());
    let mut acts = Vec::new();
    let m = &r.row;
    egui::Frame::new().inner_margin(egui::Margin { left: 18, right: 18, top: 14, bottom: 8 }).show(ui, |ui| {
        // Subject.
        let subject = if m.subject.trim().is_empty() { "(no subject)".to_string() } else { m.subject.clone() };
        ui.add(egui::Label::new(egui::RichText::new(subject).font(semibold(18.0)).color(t.text)).wrap());
        ui.add_space(10.0);
        // Sender, recipients, actions.
        ui.horizontal_top(|ui| {
            let (ar, _) = ui.allocate_exact_size(vec2(44.0, 44.0), Sense::hover());
            widgets::avatar(ui, ar.center(), 20.0, m.from.label(), &m.from.address);
            ui.add_space(6.0);
            let actions_w = if m.is_draft { 120.0 } else { 300.0 };
            ui.vertical(|ui| {
                ui.set_width((ui.available_width() - actions_w).max(120.0));
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(if m.from.name.is_empty() { m.from.address.clone() } else { m.from.name.clone() }).font(semibold(14.0)));
                    if !m.from.name.is_empty() && !m.from.address.is_empty() {
                        ui.label(egui::RichText::new(format!("<{}>", m.from.address)).color(t.text_dim));
                    }
                });
                let line = |label: &str, list: &[mailwright_store::Addr]| -> String { format!("{label}: {}", list.iter().map(|a| a.label().to_string()).collect::<Vec<_>>().join("; ")) };
                if !m.to.is_empty() {
                    ui.add(egui::Label::new(egui::RichText::new(line("To", &m.to)).color(t.text_dim).font(regular(12.0))).wrap()).on_hover_text(m.to.iter().map(|a| a.full()).collect::<Vec<_>>().join("\n"));
                }
                if !m.cc.is_empty() {
                    ui.add(egui::Label::new(egui::RichText::new(line("Cc", &m.cc)).color(t.text_dim).font(regular(12.0))).wrap());
                }
                if !m.bcc.is_empty() {
                    ui.add(egui::Label::new(egui::RichText::new(line("Bcc", &m.bcc)).color(t.text_dim).font(regular(12.0))).wrap());
                }
            });
            ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if m.is_draft {
                        if widgets::icon_text(ui, "rename", "Continue editing", true).clicked() {
                            acts.push(Act::EditDraft);
                        }
                    } else {
                        let local = m.id.starts_with("local:");
                        if in_pane && widgets::icon_button(ui, "delete", "Delete", true).clicked() {
                            acts.push(Act::Delete);
                        }
                        if widgets::icon_text(ui, "forward", "Forward", !local).clicked() && !local {
                            acts.push(Act::Reply("forward"));
                        }
                        if widgets::icon_text(ui, "reply_all", "Reply All", !local).clicked() && !local {
                            acts.push(Act::Reply("replyAll"));
                        }
                        if widgets::icon_text(ui, "reply", "Reply", !local).clicked() && !local {
                            acts.push(Act::Reply("reply"));
                        }
                    }
                });
                let when = crate::list::local(if m.received > 0 { m.received } else { m.sent });
                ui.label(egui::RichText::new(when.format("%a %d/%m/%Y %H:%M").to_string()).color(t.text_dim).font(regular(11.5)));
            });
        });
        ui.add_space(6.0);
        // Info bars.
        if m.importance == "high" {
            widgets::info_bar(ui, "important", "This message was sent with High importance.", false, None);
        }
        if m.flagged() {
            if let Some(resp) = widgets::info_bar(ui, "flag", "Follow up.", false, Some("Mark complete")) {
                if resp.clicked() {
                    acts.push(Act::Flag("complete"));
                }
            }
        } else if m.completed() {
            widgets::info_bar(ui, "flag_complete", "Completed.", false, None);
        }
        if m.pending {
            widgets::info_bar(ui, "sync", "Your changes to this item haven't reached the server yet.", false, None);
        }
        if let Some(doc) = &r.doc {
            if doc.remote_images > 0 && !r.allow_remote {
                ui.horizontal(|ui| {
                    if let Some(resp) = widgets::info_bar(ui, "image", "To help protect your privacy, pictures from the web in this message weren't downloaded.", false, Some("Download pictures")) {
                        if resp.clicked() {
                            acts.push(Act::AllowRemote(false));
                        }
                    }
                });
                if !m.from.address.is_empty() && ui.small_button(format!("Always download pictures from {}", m.from.address)).clicked() {
                    acts.push(Act::AllowRemote(true));
                }
            }
        }
        // Meeting.
        if m.is_meeting {
            meeting_box(ui, r, &t, &mut acts);
        }
        // Attachments.
        let files: Vec<&AttachmentRow> = r.attachments.iter().filter(|a| !a.inline).collect();
        if !files.is_empty() {
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                for a in &files {
                    attachment_card(ui, a, &t, &mut acts);
                }
                if files.len() > 1 && ui.link(format!("Save all {} attachments", files.len())).clicked() {
                    acts.push(Act::SaveAll);
                }
            });
        } else if m.has_attachments && r.attachments.is_empty() {
            ui.label(egui::RichText::new("Attachments will show when the message has downloaded.").color(t.text_dim).font(regular(11.5)));
        }
    });
    let sep = ui.available_rect_before_wrap();
    ui.painter().line_segment([pos2(sep.min.x + 18.0, sep.min.y), pos2(sep.max.x - 18.0, sep.min.y)], egui::Stroke::new(1.0, t.border));
    // Body.
    egui::ScrollArea::vertical().id_salt(("body", &m.id, in_pane)).auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::new().inner_margin(egui::Margin { left: 18, right: 22, top: 12, bottom: 24 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            match &r.doc {
                Some(doc) if !doc.blocks.is_empty() => {
                    let rc = RenderCtx { message_id: &m.id, textures: &app.textures, remote: &app.remote, allow_remote: r.allow_remote, tokens: t, dark: t.dark };
                    if let Some(BodyAction::Link(u)) = render::document(ui, doc, &rc) {
                        acts.push(Act::Link(u));
                    }
                }
                Some(_) => {
                    ui.label(egui::RichText::new("This message has no content.").color(t.text_dim));
                }
                None => {
                    if !m.preview.is_empty() {
                        ui.add(egui::Label::new(egui::RichText::new(&m.preview).size(render::BODY_PX)).wrap());
                        ui.add_space(12.0);
                    }
                    let online = app.session().is_some_and(|s| s.connected);
                    ui.horizontal(|ui| {
                        if online {
                            ui.spinner();
                            ui.label(egui::RichText::new("Downloading the message\u{2026}").color(t.text_dim));
                        } else {
                            ui.label(egui::RichText::new("This message hasn't been downloaded yet. It will be when you're connected.").color(t.text_dim));
                        }
                    });
                }
            }
        });
    });
    // Right-click in the pane: the same menu as the list.
    acts
}

fn meeting_box(ui: &mut Ui, r: &Reading, t: &Tokens, acts: &mut Vec<Act>) {
    ui.add_space(6.0);
    egui::Frame::new().fill(t.header_bg).corner_radius(6.0).inner_margin(egui::Margin::same(10)).stroke(egui::Stroke::new(1.0, t.border)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let Some(ev) = &r.event else {
            let kind = r.row.meeting_type.as_deref().unwrap_or("meeting");
            ui.horizontal(|ui| {
                let (ir, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
                icons::paint(ui.painter(), ir, "calendar", t.icon, t.accent);
                ui.label(match kind {
                    "meetingCancelled" => "This meeting was cancelled.",
                    "meetingAccepted" => "Accepted.",
                    "meetingTenativelyAccepted" | "meetingTentativelyAccepted" => "Tentatively accepted.",
                    "meetingDeclined" => "Declined.",
                    _ => "Meeting details will show when the message has downloaded.",
                });
            });
            return;
        };
        ui.horizontal_top(|ui| {
            let (ir, _) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::hover());
            icons::paint(ui.painter(), ir, "calendar", t.icon, t.accent);
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(crate::calendar::when_text(ev)).font(semibold(13.0)));
                if !ev.location.is_empty() {
                    ui.label(egui::RichText::new(&ev.location).color(t.text_dim));
                }
                if !ev.organizer.address.is_empty() {
                    ui.label(egui::RichText::new(format!("Organiser: {}", ev.organizer.label())).color(t.text_dim));
                }
                if !ev.join_url.is_empty() && ui.link("Join the online meeting").clicked() {
                    acts.push(Act::JoinMeeting(ev.join_url.clone()));
                }
            });
        });
        ui.add_space(6.0);
        if ev.is_cancelled || r.row.meeting_type.as_deref() == Some("meetingCancelled") {
            ui.label(egui::RichText::new("This meeting was cancelled.").color(t.high));
            return;
        }
        let state = match ev.response.as_str() {
            "accepted" => Some("You accepted this meeting."),
            "tentativelyAccepted" => Some("You tentatively accepted this meeting."),
            "declined" => Some("You declined this meeting."),
            "organizer" => Some("You're the organiser."),
            _ => None,
        };
        if let Some(s) = state {
            ui.label(egui::RichText::new(s).color(t.text_dim));
        }
        if !ev.is_organizer && r.row.meeting_type.as_deref().is_none_or(|k| k == "meetingRequest") {
            ui.horizontal(|ui| {
                for (icon, label, resp) in [("accept", "Accept", "accept"), ("tentative", "Tentative", "tentative"), ("decline", "Decline", "decline")] {
                    let b = widgets::icon_text(ui, icon, label, true);
                    if b.clicked() {
                        acts.push(Act::Respond(resp, true));
                    }
                    b.context_menu(|ui| {
                        if ui.button("Send the response now").clicked() {
                            acts.push(Act::Respond(resp, true));
                            ui.close();
                        }
                        if ui.button("Don't send a response").clicked() {
                            acts.push(Act::Respond(resp, false));
                            ui.close();
                        }
                    });
                }
                if ui.link("Calendar\u{2026}").clicked() {
                    acts.push(Act::OpenEvent(ev.id.clone()));
                }
            });
        }
    });
}

fn attachment_card(ui: &mut Ui, a: &AttachmentRow, t: &Tokens, acts: &mut Vec<Act>) {
    let name_w = ui.painter().layout_no_wrap(a.name.clone(), medium(12.0), t.text).size().x.min(200.0);
    let (r, resp) = ui.allocate_exact_size(vec2(name_w + 56.0, 40.0), Sense::click());
    let fill = if resp.hovered() { t.hover } else { t.input };
    ui.painter().rect(r, 4.0, fill, egui::Stroke::new(1.0, t.border_strong), egui::StrokeKind::Inside);
    let (icon, col) = icons::file_icon(&a.name);
    icons::paint(ui.painter(), Rect::from_center_size(pos2(r.min.x + 18.0, r.center().y), vec2(22.0, 22.0)), icon, col, col);
    let p = ui.painter_at(r.shrink(2.0));
    p.text(pos2(r.min.x + 34.0, r.min.y + 13.0), Align2::LEFT_CENTER, crate::list::elide(&p, &a.name, medium(12.0), name_w), medium(12.0), t.text);
    p.text(pos2(r.min.x + 34.0, r.min.y + 28.0), Align2::LEFT_CENTER, widgets::size_text(a.size), regular(11.0), t.text_dim);
    let resp = resp.on_hover_text(format!("{}\nClick to open; right-click to save", a.name));
    if resp.clicked() {
        acts.push(Act::Attachment(a.clone(), true));
    }
    resp.context_menu(|ui| {
        if widgets::menu_item(ui, "open", "Open", "").clicked() {
            acts.push(Act::Attachment(a.clone(), true));
            ui.close();
        }
        if widgets::menu_item(ui, "download", "Save As\u{2026}", "").clicked() {
            acts.push(Act::Attachment(a.clone(), false));
            ui.close();
        }
    });
}

/// Message windows (double-click in the list).
pub fn reader_windows(app: &mut MailApp, ctx: &egui::Context) {
    let readers = std::mem::take(&mut app.readers);
    let mut keep = Vec::new();
    for (wid, mid) in readers {
        let Some(row) = app.session().and_then(|s| s.store.message(&mid).ok().flatten()) else { continue };
        let title = if row.subject.is_empty() { "(no subject)".to_string() } else { row.subject.clone() };
        let attachments = app.session().map(|s| s.store.attachments(&mid).unwrap_or_default()).unwrap_or_default();
        let doc = row.body.as_deref().map(mailwright_html::parse);
        let reading = Reading { row, doc, attachments, event: None, allow_remote: app.settings.remote_images, requested: true, opened_at: std::time::Instant::now() };
        let mut open = true;
        let mut acts = Vec::new();
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of(("reader", wid)), egui::ViewportBuilder::default().with_title(format!("{title} \u{2013} Message")).with_inner_size([820.0, 720.0]), |ui, _class| {
            let t = Tokens::get(ui.ctx());
            egui::CentralPanel::default().frame(egui::Frame::new().fill(t.reading)).show(ui, |ui| {
                acts = message_view(app, ui, &reading, false);
            });
            if ui.ctx().input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape)) {
                open = false;
            }
        });
        for a in acts {
            if let Act::Reply(kind) = a {
                app.mail.selected = vec![mid.clone()];
                app.reply(kind);
            } else if let Act::Link(u) = a {
                perform(app, Act::Link(u));
            } else if let Act::Attachment(att, o) = a {
                app.save_attachment(&att, o);
            }
        }
        if open {
            keep.push((wid, mid));
        }
    }
    keep.extend(std::mem::take(&mut app.readers));
    app.readers = keep;
}

impl MailApp {
    pub fn open_message_window(&mut self, id: &str) {
        let row = self.mail.list.iter().find(|m| m.id == id).cloned();
        if let Some(r) = &row {
            if r.is_draft && !self.folder_wk("sentitems").is_some_and(|s| s == r.folder_id) {
                self.select_message(id, false, false);
                self.edit_draft(r);
                return;
            }
        }
        self.select_message(id, false, false);
        if !self.readers.iter().any(|(_, m)| m == id) {
            let wid = self.next_id();
            self.readers.push((wid, id.to_string()));
        }
    }
}
