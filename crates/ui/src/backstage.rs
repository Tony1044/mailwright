//! File: account information, Options and About.

use egui::{Rect, Sense, Ui, pos2, vec2};

use crate::app::{Backstage, Confirm, MailApp};
use crate::settings::{ReadingPane, Settings};
use crate::theme::{Appearance, Tokens, medium, regular, semibold};
use crate::{icons, widgets};

pub fn show(app: &mut MailApp, ui: &mut Ui, page: Backstage) {
    let t = Tokens::get(ui.ctx());
    egui::Panel::left("backstage_nav").exact_size(200.0).frame(egui::Frame::new().fill(t.accent)).show(ui, |ui| {
        ui.add_space(12.0);
        let (r, resp) = ui.allocate_exact_size(vec2(200.0, 40.0), Sense::click());
        icons::paint(ui.painter(), Rect::from_center_size(pos2(r.min.x + 24.0, r.center().y), vec2(20.0, 20.0)), "chevron_left", t.on_accent, t.on_accent);
        if resp.hovered() {
            ui.painter().rect_filled(r, 0.0, egui::Color32::from_white_alpha(30));
        }
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Back"));
        if resp.clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            app.backstage = None;
            app.options_draft = None;
        }
        ui.add_space(12.0);
        for (p, label) in [(Backstage::Info, "Info"), (Backstage::Options, "Options"), (Backstage::About, "About")] {
            let (r, resp) = ui.allocate_exact_size(vec2(200.0, 38.0), Sense::click());
            if p == page {
                ui.painter().rect_filled(r, 0.0, egui::Color32::from_black_alpha(40));
            } else if resp.hovered() {
                ui.painter().rect_filled(r, 0.0, egui::Color32::from_white_alpha(30));
            }
            ui.painter().text(pos2(r.min.x + 24.0, r.center().y), egui::Align2::LEFT_CENTER, label, medium(14.0), t.on_accent);
            resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, p == page, label));
            if resp.clicked() {
                if p == Backstage::Options && app.options_draft.is_none() {
                    app.options_draft = Some(app.settings.clone());
                }
                app.backstage = Some(p);
            }
        }
        ui.add_space(12.0);
        let (r, resp) = ui.allocate_exact_size(vec2(200.0, 38.0), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(r, 0.0, egui::Color32::from_white_alpha(30));
        }
        ui.painter().text(pos2(r.min.x + 24.0, r.center().y), egui::Align2::LEFT_CENTER, "Exit", medium(14.0), t.on_accent);
        if resp.clicked() {
            app.quit = true;
        }
    });
    egui::CentralPanel::default().frame(egui::Frame::new().fill(t.reading).inner_margin(egui::Margin::symmetric(36, 28))).show(ui, |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match page {
            Backstage::Info => info(app, ui, &t),
            Backstage::Options => options(app, ui, &t),
            Backstage::About => about(ui, &t),
        });
    });
}

fn info(app: &mut MailApp, ui: &mut Ui, t: &Tokens) {
    ui.label(egui::RichText::new("Account Information").font(semibold(26.0)));
    ui.add_space(18.0);
    let Some(s) = &app.session else { return };
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(64.0, 64.0), Sense::hover());
        widgets::avatar(ui, r.center(), 30.0, &s.name, &s.address);
        ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(if s.name.is_empty() { s.address.clone() } else { s.name.clone() }).font(semibold(17.0)));
            ui.label(egui::RichText::new(&s.address).color(t.text_dim));
            let conn = if app.settings.work_offline {
                "Working offline"
            } else if s.demo {
                "Sample mailbox (on this computer)"
            } else if s.connected {
                "Microsoft Exchange Online, through Microsoft Graph"
            } else {
                "Disconnected"
            };
            ui.label(egui::RichText::new(conn).color(t.text_dim));
        });
    });
    ui.add_space(20.0);
    section(ui, "Account Settings", t);
    egui::Grid::new("acct").num_columns(2).spacing(vec2(16.0, 6.0)).show(ui, |ui| {
        ui.label(egui::RichText::new("Application (client) ID").color(t.text_dim));
        ui.label(&s.account.client_id);
        ui.end_row();
        ui.label(egui::RichText::new("Tenant").color(t.text_dim));
        ui.label(s.account.tenant_or_default());
        ui.end_row();
        let store = mailcraft_graph::tokens::SystemTokenStore::new(app.data_dir.clone());
        ui.label(egui::RichText::new("Sign-in kept in").color(t.text_dim));
        ui.label(if s.demo { "memory (sample mailbox)".to_string() } else { mailcraft_graph::tokens::TokenStore::location(&store) });
        ui.end_row();
        ui.label(egui::RichText::new("Offline cache").color(t.text_dim));
        let size: u64 = ["", "-wal"].iter().filter_map(|x| std::fs::metadata(format!("{}{x}", s.db_path.display())).ok()).map(|m| m.len()).sum();
        ui.label(format!("{} ({}), mail from the last {} days", s.db_path.display(), widgets::size_text(size as i64), app.settings.offline_days));
        ui.end_row();
        ui.label(egui::RichText::new("Waiting to send").color(t.text_dim));
        ui.label(format!("{} change{}", s.queued, if s.queued == 1 { "" } else { "s" }));
        ui.end_row();
    });
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        if widgets::icon_text(ui, "send_receive", "Send/Receive now", true).clicked() {
            app.sync_all();
            app.backstage = None;
        }
        if widgets::icon_text(ui, "sign_out", "Sign out", true).clicked() {
            app.mail.confirm = Some(Confirm::SignOut);
            app.backstage = None;
        }
    });
    ui.add_space(8.0);
    ui.label(egui::RichText::new("Signing out removes this computer's copy of the mailbox and the saved sign-in. Nothing changes on the server.").color(t.text_dim).font(regular(12.0)));
}

fn section(ui: &mut Ui, title: &str, t: &Tokens) {
    ui.label(egui::RichText::new(title).font(semibold(15.0)));
    let r = ui.available_rect_before_wrap();
    ui.painter().line_segment([pos2(r.min.x, r.min.y + 2.0), pos2(r.min.x + 560.0, r.min.y + 2.0)], egui::Stroke::new(1.0, t.border));
    ui.add_space(10.0);
}

fn options(app: &mut MailApp, ui: &mut Ui, t: &Tokens) {
    if app.options_draft.is_none() {
        app.options_draft = Some(app.settings.clone());
    }
    let Some(mut d) = app.options_draft.take() else { return };
    ui.label(egui::RichText::new("MailCraft Options").font(semibold(26.0)));
    ui.add_space(16.0);
    section(ui, "General", t);
    ui.horizontal(|ui| {
        ui.label("Theme");
        egui::ComboBox::from_id_salt("opt_theme").selected_text(d.appearance.label()).show_ui(ui, |ui| {
            for a in Appearance::ALL {
                ui.selectable_value(&mut d.appearance, a, a.label());
            }
        });
    });
    ui.add_space(12.0);
    section(ui, "Mail", t);
    let mut mark = d.mark_read_after.is_some();
    ui.horizontal(|ui| {
        ui.checkbox(&mut mark, "Mark items as read when viewed in the Reading Pane, after");
        let mut secs = d.mark_read_after.unwrap_or(1);
        ui.add_enabled(mark, egui::DragValue::new(&mut secs).range(0..=60).suffix(" s"));
        d.mark_read_after = mark.then_some(secs);
    });
    ui.checkbox(&mut d.remote_images, "Download pictures in HTML email automatically");
    if !d.safe_senders.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("Pictures always download from:").color(t.text_dim));
            let mut remove = None;
            for (i, s) in d.safe_senders.iter().enumerate() {
                if ui.small_button(format!("{s} \u{2715}")).clicked() {
                    remove = Some(i);
                }
            }
            if let Some(i) = remove {
                d.safe_senders.remove(i);
            }
        });
    }
    ui.checkbox(&mut d.focused_inbox, "Show Focused Inbox");
    ui.checkbox(&mut d.message_preview, "Show a preview line under each message");
    ui.horizontal(|ui| {
        ui.label("Reading Pane");
        for (p, l) in [(ReadingPane::Right, "Right"), (ReadingPane::Bottom, "Bottom"), (ReadingPane::Off, "Off")] {
            ui.radio_value(&mut d.reading_pane, p, l);
        }
    });
    ui.horizontal(|ui| {
        ui.label("Keep mail on this computer for");
        egui::ComboBox::from_id_salt("opt_days").selected_text(format!("{} days", d.offline_days)).show_ui(ui, |ui| {
            for (v, l) in [(30, "1 month"), (90, "3 months"), (180, "6 months"), (365, "1 year"), (730, "2 years")] {
                ui.selectable_value(&mut d.offline_days, v, l);
            }
        });
    });
    ui.add_space(12.0);
    section(ui, "Compose", t);
    ui.horizontal(|ui| {
        ui.label("Font");
        ui.add(egui::TextEdit::singleline(&mut d.compose_font).desired_width(160.0));
        ui.add(egui::DragValue::new(&mut d.compose_size).range(8.0..=24.0).suffix(" pt"));
    });
    ui.label("Signature");
    ui.add(egui::TextEdit::multiline(&mut d.signature).desired_rows(4).desired_width(520.0).hint_text("Your name\nJob title | Organisation"));
    ui.checkbox(&mut d.signature_on_replies, "Add the signature to replies and forwards");
    ui.add_space(12.0);
    section(ui, "Calendar", t);
    ui.horizontal(|ui| {
        ui.label("Work hours");
        ui.add(egui::DragValue::new(&mut d.work_start).range(0..=12).suffix(":00"));
        ui.label("to");
        ui.add(egui::DragValue::new(&mut d.work_end).range(12..=23).suffix(":00"));
    });
    ui.horizontal(|ui| {
        ui.label("First day of the week");
        let days = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
        egui::ComboBox::from_id_salt("opt_week").selected_text(days[d.week_start.min(6) as usize]).show_ui(ui, |ui| {
            for (i, n) in days.iter().enumerate() {
                ui.selectable_value(&mut d.week_start, i as u32, *n);
            }
        });
    });
    ui.horizontal(|ui| {
        ui.label("Default reminder");
        ui.add(egui::DragValue::new(&mut d.default_reminder).range(0..=10080).suffix(" min"));
    });
    ui.add_space(18.0);
    let mut done = false;
    ui.horizontal(|ui| {
        if widgets::primary(ui, "OK").clicked() {
            apply_options(app, ui.ctx(), &d);
            done = true;
        }
        if widgets::secondary(ui, "Cancel").clicked() {
            done = true;
        }
    });
    if done {
        app.backstage = None;
    } else {
        app.options_draft = Some(d);
    }
}

fn apply_options(app: &mut MailApp, ctx: &egui::Context, d: &Settings) {
    let days_changed = d.offline_days != app.settings.offline_days;
    app.settings = Settings { account: app.settings.account.clone(), ..d.clone() };
    crate::theme::set_appearance(ctx, d.appearance);
    if days_changed {
        app.send(mailcraft_sync::Command::SetWindow(d.offline_days));
    }
    app.mail.list_dirty = true;
    app.save_settings();
}

fn about(ui: &mut Ui, t: &Tokens) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(56.0, 56.0), Sense::hover());
        crate::app_icon::paint(ui.painter(), r);
        ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.label(egui::RichText::new("MailCraft").font(semibold(26.0)));
            ui.label(egui::RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).color(t.text_dim));
        });
    });
    ui.add_space(16.0);
    for line in [
        "A desktop mail and calendar client for Microsoft 365, written in Rust with no unsafe code.",
        "It works with Exchange Online through Microsoft Graph: mail folders, messages and attachments, sending, flags and categories, Focused Inbox, the calendar and meeting responses. Mail from the last few months is kept on this computer (SQLite) and kept in step with Graph delta queries, so it can be read, searched and organised offline; changes made offline are sent when you're back online.",
        "MailCraft is an independent program. It is not made or endorsed by Microsoft. Microsoft 365, Exchange and Outlook are trademarks of the Microsoft group of companies.",
        "Fonts: Inter, Source Sans 3, Source Serif 4 and JetBrains Mono, under the SIL Open Font License 1.1.",
        "Licensed under the MIT or Apache 2.0 licence.",
    ] {
        ui.add(egui::Label::new(egui::RichText::new(line).font(regular(13.0))).wrap());
        ui.add_space(8.0);
    }
}
