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
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        if widgets::icon_text(ui, "plus", "Add Account", true).clicked() {
            app.add_account();
        }
        if widgets::icon_text(ui, "send_receive", "Send/Receive all accounts", true).clicked() {
            app.sync_all();
            app.backstage = None;
        }
    });
    ui.add_space(18.0);
    let keyring = app
        .keyring_location
        .get_or_insert_with(|| {
            let store = mailwright_graph::tokens::SystemTokenStore::new(app.data_dir.clone());
            mailwright_graph::tokens::TokenStore::location(&store)
        })
        .clone();
    let mut remove = None;
    let mut reauth = None;
    for (i, s) in app.sessions.iter().enumerate() {
        egui::Frame::new().stroke(egui::Stroke::new(1.0, t.border)).corner_radius(8.0).inner_margin(egui::Margin::same(16)).show(ui, |ui| {
            ui.set_width(ui.available_width().min(760.0));
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(56.0, 56.0), Sense::hover());
                widgets::avatar(ui, r.center(), 26.0, &s.name, &s.address);
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(if s.name.is_empty() { s.address.clone() } else { s.name.clone() }).font(semibold(16.0)));
                    ui.label(egui::RichText::new(&s.address).color(t.text_dim));
                    let kind = if s.demo {
                        "Sample mailbox (on this computer)".to_string()
                    } else if s.account.tenant == "consumers" {
                        "Outlook.com, through Microsoft Graph".to_string()
                    } else {
                        "Microsoft 365 (Exchange Online), through Microsoft Graph".to_string()
                    };
                    let state = if s.needs_sign_in.is_some() {
                        "needs you to sign in again"
                    } else if app.settings.work_offline && !s.demo {
                        "working offline"
                    } else if s.connected {
                        "connected"
                    } else {
                        "disconnected"
                    };
                    ui.label(egui::RichText::new(format!("{kind} \u{2013} {state}")).color(t.text_dim));
                });
            });
            ui.add_space(8.0);
            egui::Grid::new(("acct", i)).num_columns(2).spacing(vec2(16.0, 4.0)).show(ui, |ui| {
                ui.label(egui::RichText::new("Sign-in kept in").color(t.text_dim));
                ui.label(if s.demo { "memory (sample mailbox)".to_string() } else { keyring.clone() });
                ui.end_row();
                ui.label(egui::RichText::new("Offline cache").color(t.text_dim));
                let size: u64 = ["", "-wal"].iter().filter_map(|x| std::fs::metadata(format!("{}{x}", s.db_path.display())).ok()).map(|m| m.len()).sum();
                ui.label(format!("{} ({})", s.db_path.display(), widgets::size_text(size as i64)));
                ui.end_row();
                ui.label(egui::RichText::new("Waiting to send").color(t.text_dim));
                ui.label(format!("{} change{}", s.queued, if s.queued == 1 { "" } else { "s" }));
                ui.end_row();
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if s.needs_sign_in.is_some() && widgets::icon_text(ui, "lock", "Sign in again", true).clicked() {
                    reauth = Some(i);
                }
                if widgets::icon_text(ui, "sign_out", "Remove Account", true).clicked() {
                    remove = Some(s.label().to_string());
                }
            });
        });
        ui.add_space(12.0);
    }
    if let Some(i) = reauth {
        app.backstage = None;
        app.reauthenticate(i);
    }
    if let Some(a) = remove {
        app.mail.confirm = Some(Confirm::RemoveAccount(a));
        app.backstage = None;
    }
    ui.label(egui::RichText::new("Removing an account deletes this computer's copy of its mail and its saved sign-in. Nothing changes on the server.").color(t.text_dim).font(regular(12.0)));
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
    ui.label(egui::RichText::new("Mailwright Options").font(semibold(26.0)));
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
    ui.add_space(8.0);
    ui.label(egui::RichText::new("When new messages arrive").font(semibold(14.0)));
    ui.checkbox(&mut d.new_mail_sound, "Play a sound");
    ui.checkbox(&mut d.new_mail_alert, "Display a Desktop Alert");
    ui.checkbox(&mut d.new_mail_envelope, "Show an envelope icon on the taskbar");
    if ui.small_button("Test").on_hover_text("Play the sound and show an alert").clicked() {
        let items = vec![mailwright_sync::NewMail { id: String::new(), from: "Mailwright".into(), subject: "This is what new mail looks like".into(), preview: String::new(), focused: true }];
        crate::notify::announce(crate::notify::Alert { account: String::new(), items, popup: d.new_mail_alert, sound: d.new_mail_sound }, None, || {});
    }
    ui.add_space(8.0);
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
    app.settings = Settings { accounts: app.settings.accounts.clone(), ..d.clone() };
    crate::theme::set_appearance(ctx, d.appearance);
    if days_changed {
        app.send(mailwright_sync::Command::SetWindow(d.offline_days));
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
            ui.label(egui::RichText::new("Mailwright").font(semibold(26.0)));
            ui.label(egui::RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).color(t.text_dim));
        });
    });
    ui.add_space(16.0);
    for line in [
        "A desktop mail and calendar client for Microsoft 365, written in Rust with no unsafe code.",
        "It works with Exchange Online through Microsoft Graph: mail folders, messages and attachments, sending, flags and categories, Focused Inbox, the calendar and meeting responses. Mail from the last few months is kept on this computer (SQLite) and kept in step with Graph delta queries, so it can be read, searched and organised offline; changes made offline are sent when you're back online.",
        "Mailwright is an independent program. It is not made or endorsed by Microsoft. Microsoft 365, Exchange and Outlook are trademarks of the Microsoft group of companies.",
        "Fonts: Inter, Source Sans 3, Source Serif 4 and JetBrains Mono, under the SIL Open Font License 1.1.",
        "Licensed under the MIT or Apache 2.0 licence.",
    ] {
        ui.add(egui::Label::new(egui::RichText::new(line).font(regular(13.0))).wrap());
        ui.add_space(8.0);
    }
}
