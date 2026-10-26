//! Automatic Replies (out-of-office), as Outlook has them under File › Info: on or off, an
//! optional time range, and separate messages for inside and outside the organisation. The
//! messages are stored on the server as HTML; here they're edited as plain text.

use chrono::{Local, TimeZone};
use egui::vec2;
use mailwright_graph::types::{AutomaticRepliesSetting, DateTimeTimeZone};

use crate::app::MailApp;
use crate::theme::{Tokens, semibold};
use crate::widgets;

/// The Automatic Replies dialog's state while it's open.
pub struct AutoReplyUi {
    /// The account (session index) this is for.
    pub account: usize,
    pub loading: bool,
    pub saving: bool,
    pub error: Option<String>,
    pub enabled: bool,
    pub scheduled: bool,
    /// Local times, "YYYY-MM-DD HH:MM".
    pub start: String,
    pub end: String,
    pub also_external: bool,
    /// Outside replies go to everyone, rather than only known contacts.
    pub external_all: bool,
    pub internal: String,
    pub external: String,
}

impl AutoReplyUi {
    pub fn loading(account: usize) -> AutoReplyUi {
        let now = Local::now();
        let later = now + chrono::Duration::hours(24);
        AutoReplyUi {
            account,
            loading: true,
            saving: false,
            error: None,
            enabled: false,
            scheduled: false,
            start: now.format("%Y-%m-%d %H:%M").to_string(),
            end: later.format("%Y-%m-%d %H:%M").to_string(),
            also_external: true,
            external_all: false,
            internal: String::new(),
            external: String::new(),
        }
    }

    /// Fill from what the server has.
    pub fn apply(&mut self, s: &AutomaticRepliesSetting) {
        self.loading = false;
        self.saving = false;
        self.enabled = s.status != "disabled" && !s.status.is_empty();
        self.scheduled = s.status == "scheduled";
        if let Some(d) = &s.scheduled_start_date_time {
            if let Some(t) = local_string(d) {
                self.start = t;
            }
        }
        if let Some(d) = &s.scheduled_end_date_time {
            if let Some(t) = local_string(d) {
                self.end = t;
            }
        }
        self.also_external = s.external_audience != "none";
        self.external_all = s.external_audience == "all";
        self.internal = mailwright_html::to_text(&s.internal_reply_message);
        self.external = mailwright_html::to_text(&s.external_reply_message);
        if self.external.trim().is_empty() {
            self.external = self.internal.clone();
        }
    }

    /// What to send to the server.
    fn to_setting(&self) -> AutomaticRepliesSetting {
        let status = if !self.enabled {
            "disabled"
        } else if self.scheduled {
            "scheduled"
        } else {
            "alwaysEnabled"
        };
        let audience = if !self.also_external {
            "none"
        } else if self.external_all {
            "all"
        } else {
            "contactsOnly"
        };
        AutomaticRepliesSetting {
            status: status.into(),
            external_audience: audience.into(),
            scheduled_start_date_time: self.scheduled.then(|| utc_datetime(&self.start)).flatten(),
            scheduled_end_date_time: self.scheduled.then(|| utc_datetime(&self.end)).flatten(),
            internal_reply_message: mailwright_html::text_to_html(&self.internal, "Calibri", 11.0),
            external_reply_message: mailwright_html::text_to_html(if self.also_external { &self.external } else { "" }, "Calibri", 11.0),
        }
    }
}

/// A Graph date-time (UTC or a named zone) shown in the person's local time.
fn local_string(d: &DateTimeTimeZone) -> Option<String> {
    let naive = chrono::NaiveDateTime::parse_from_str(d.date_time.split('.').next().unwrap_or(&d.date_time), "%Y-%m-%dT%H:%M:%S").ok()?;
    // Graph gives these in UTC when we send them that way; treat a non-UTC zone as local.
    if d.time_zone.eq_ignore_ascii_case("UTC") {
        let utc = chrono::Utc.from_utc_datetime(&naive);
        Some(utc.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string())
    } else {
        Some(naive.format("%Y-%m-%d %H:%M").to_string())
    }
}

/// "YYYY-MM-DD HH:MM" in local time to a UTC Graph date-time.
fn utc_datetime(s: &str) -> Option<DateTimeTimeZone> {
    let naive = chrono::NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M").ok()?;
    let local = Local.from_local_datetime(&naive).single()?;
    Some(DateTimeTimeZone { date_time: local.with_timezone(&chrono::Utc).format("%Y-%m-%dT%H:%M:%S").to_string(), time_zone: "UTC".into() })
}

enum Act {
    Save,
    TurnOff,
    Close,
}

pub fn dialog(app: &mut MailApp, ctx: &egui::Context) {
    if app.auto_reply.is_none() {
        return;
    }
    let t = Tokens::get(ctx);
    let label = app.sessions.get(app.auto_reply.as_ref().map_or(0, |a| a.account)).map(|s| s.label().to_string()).unwrap_or_default();
    let mut act = None;
    egui::Modal::new(egui::Id::new("auto_reply")).show(ctx, |ui| {
        ui.set_width(560.0);
        let Some(a) = app.auto_reply.as_mut() else { return };
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Automatic Replies").font(semibold(18.0)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(&label).color(t.text_dim));
            });
        });
        ui.add_space(8.0);
        if a.loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Checking with the server\u{2026}");
            });
            ui.add_space(8.0);
            if widgets::secondary(ui, "Cancel").clicked() {
                act = Some(Act::Close);
            }
            return;
        }
        if let Some(e) = &a.error {
            widgets::info_bar(ui, "info", e, true, None);
            ui.add_space(8.0);
        }
        ui.radio_value(&mut a.enabled, false, "Don't send automatic replies");
        ui.radio_value(&mut a.enabled, true, "Send automatic replies");
        ui.add_space(6.0);
        ui.add_enabled_ui(a.enabled, |ui| {
            ui.indent("ar_body", |ui| {
                ui.checkbox(&mut a.scheduled, "Only send during this time range:");
                ui.add_enabled_ui(a.scheduled, |ui| {
                    ui.indent("ar_times", |ui| {
                        egui::Grid::new("ar_grid").spacing(vec2(8.0, 6.0)).show(ui, |ui| {
                            ui.label("Start time");
                            ui.add(egui::TextEdit::singleline(&mut a.start).desired_width(160.0));
                            ui.label(egui::RichText::new("YYYY-MM-DD HH:MM").color(t.text_dim));
                            ui.end_row();
                            ui.label("End time");
                            ui.add(egui::TextEdit::singleline(&mut a.end).desired_width(160.0));
                            ui.label(egui::RichText::new("your local time").color(t.text_dim));
                            ui.end_row();
                        });
                    });
                });
                ui.add_space(8.0);
                ui.label(egui::RichText::new("Inside my organisation").font(semibold(13.0)));
                ui.add(egui::TextEdit::multiline(&mut a.internal).desired_width(f32::INFINITY).desired_rows(4).hint_text("I'm currently out of the office\u{2026}"));
                ui.add_space(8.0);
                ui.checkbox(&mut a.also_external, "Also reply to people outside my organisation");
                ui.add_enabled_ui(a.also_external, |ui| {
                    ui.indent("ar_ext", |ui| {
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut a.external_all, false, "My contacts only");
                            ui.radio_value(&mut a.external_all, true, "Anyone outside");
                        });
                        ui.add(egui::TextEdit::multiline(&mut a.external).desired_width(f32::INFINITY).desired_rows(4));
                    });
                });
            });
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            let save = if a.saving { "Saving\u{2026}" } else { "Save" };
            if ui.add_enabled(!a.saving, egui::Button::new(egui::RichText::new(save).color(t.on_accent)).fill(t.accent).min_size(vec2(90.0, 30.0))).clicked() {
                act = Some(Act::Save);
            }
            if a.enabled && widgets::secondary(ui, "Turn off").clicked() {
                act = Some(Act::TurnOff);
            }
            if widgets::secondary(ui, "Cancel").clicked() {
                act = Some(Act::Close);
            }
        });
    });
    match act {
        Some(Act::Close) => app.auto_reply = None,
        Some(Act::TurnOff) => {
            if let Some(a) = app.auto_reply.as_mut() {
                a.enabled = false;
                a.saving = true;
                a.error = None;
                let (i, setting) = (a.account, a.to_setting());
                app.send_to(i, mailwright_sync::Command::SetAutoReply(Box::new(setting)));
            }
        }
        Some(Act::Save) => {
            if let Some(a) = app.auto_reply.as_mut() {
                a.saving = true;
                a.error = None;
                let (i, setting) = (a.account, a.to_setting());
                app.send_to(i, mailwright_sync::Command::SetAutoReply(Box::new(setting)));
            }
        }
        None => {}
    }
}
