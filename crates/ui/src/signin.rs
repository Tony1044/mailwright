//! The first screen: sign in to Microsoft 365 (Exchange Online), or try the sample mailbox.

use egui::{Ui, vec2};
use mailcraft_graph::GraphError;

use crate::app::{MailApp, SignInMethod};
use crate::theme::{Tokens, regular, semibold};
use crate::widgets;

/// What a sign-in failure means, in words.
pub fn explain(e: &GraphError) -> String {
    match e {
        GraphError::Network { host, .. } => format!("Can't reach {host}. Check your internet connection (and proxy) and try again."),
        GraphError::SignInRequired(m) if m.contains("AADSTS700016") => {
            "Microsoft couldn't find MailCraft's app registration. If your organisation gave you its own Application (client) ID, check it under Advanced.".into()
        }
        GraphError::SignInRequired(m) | GraphError::SignInCancelled(m) if m.contains("AADSTS65001") || m.contains("AADSTS90094") || m.contains("consent") || m.contains("admin approval") => {
            "Your organisation needs an administrator to approve MailCraft before you can sign in. Ask your IT team to approve \u{201c}MailCraft\u{201d} (they can do it from the sign-in page with an admin account, or in Microsoft Entra under Enterprise applications).".into()
        }
        GraphError::SignInRequired(m) if m.contains("AADSTS50011") => {
            "The redirect URI doesn't match. In the app registration, add the platform \u{201c}Mobile and desktop applications\u{201d} with the redirect URI http://localhost.".into()
        }
        GraphError::SignInRequired(m) if m.contains("AADSTS7000218") => {
            "The app registration needs \u{201c}Allow public client flows\u{201d} turned on (Authentication \u{203a} Advanced settings).".into()
        }
        GraphError::SignInRequired(m) => format!("Sign-in didn't work: {m}"),
        GraphError::SignInCancelled(m) => format!("Sign-in was cancelled: {m}"),
        GraphError::Config(m) => m.clone(),
        GraphError::Api { status: 403, message, .. } => format!("Access denied: {message}. Your organisation may block this app or these permissions."),
        other => other.to_string(),
    }
}

pub fn screen(app: &mut MailApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space((ui.available_height() * 0.08).max(20.0));
        ui.vertical_centered(|ui| {
            let w = 560.0f32.min(ui.available_width() - 32.0);
            egui::Frame::new()
                .fill(t.reading)
                .corner_radius(10.0)
                .stroke(egui::Stroke::new(1.0, t.border))
                .shadow(egui::Shadow { offset: [0, 6], blur: 24, spread: 0, color: egui::Color32::from_black_alpha(if t.dark { 120 } else { 30 }) })
                .inner_margin(egui::Margin::same(28))
                .show(ui, |ui| {
                    ui.set_width(w);
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| card(app, ui, &t));
                });
        });
        ui.add_space(24.0);
    });
}

fn card(app: &mut MailApp, ui: &mut Ui, t: &Tokens) {
    let adding = app.signin.adding && !app.sessions.is_empty();
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(40.0, 40.0), egui::Sense::hover());
        crate::app_icon::paint(ui.painter(), r);
        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(if adding { "Add an account" } else { "Welcome to MailCraft" }).font(semibold(22.0)));
            ui.label(egui::RichText::new("Mail and calendar for Microsoft 365 and Outlook.com").color(t.text_dim));
        });
    });
    ui.add_space(18.0);
    if app.signin.busy {
        busy(app, ui, t);
        return;
    }
    ui.label(egui::RichText::new("Email address").font(semibold(13.0)));
    ui.add_space(4.0);
    let id = egui::Id::new("signin_email");
    let resp = ui.add(egui::TextEdit::singleline(&mut app.signin.email).id(id).hint_text("name@example.com").desired_width(f32::INFINITY).margin(vec2(8.0, 6.0)).font(regular(15.0)));
    if !ui.memory(|m| m.focused().is_some()) && app.signin.discovery.is_none() {
        resp.request_focus();
    }
    if resp.changed() {
        app.signin.discovery = None;
        app.signin.error = None;
    }
    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    ui.add_space(10.0);
    match app.signin.discovery.clone() {
        None => {
            ui.horizontal(|ui| {
                let connect = ui.add_enabled(!app.signin.discovering, egui::Button::new(egui::RichText::new("Connect").color(t.on_accent).font(crate::theme::medium(13.0))).fill(t.accent).min_size(vec2(110.0, 30.0)));
                if (connect.clicked() || enter) && !app.signin.discovering {
                    app.discover();
                }
                if app.signin.discovering {
                    ui.spinner();
                    ui.label(egui::RichText::new("Finding your account\u{2026}").color(t.text_dim));
                }
                if adding && widgets::secondary(ui, "Cancel").clicked() {
                    app.signin.adding = false;
                }
            });
        }
        Some(d) => {
            let ok = d.tenant().is_some();
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), egui::Sense::hover());
                crate::icons::paint(ui.painter(), r, if ok { "accept" } else { "info" }, t.icon, if ok { t.green } else { t.high });
                ui.add(egui::Label::new(egui::RichText::new(d.describe()).color(t.text)).wrap());
            });
            ui.add_space(10.0);
            if ok {
                ui.horizontal(|ui| {
                    let label = if app.sign_in_window.is_some() { "Sign in" } else { "Sign in with your browser" };
                    if widgets::primary(ui, label).clicked() || enter {
                        app.begin_sign_in(SignInMethod::InApp);
                    }
                    if app.sign_in_window.is_some() && widgets::secondary(ui, "Use my browser instead").clicked() {
                        app.begin_sign_in(SignInMethod::Browser);
                    }
                    if adding && widgets::secondary(ui, "Cancel").clicked() {
                        app.signin.adding = false;
                    }
                });
                ui.add_space(6.0);
                if ui.link(egui::RichText::new("Sign in on another device with a code").font(regular(12.0))).clicked() {
                    app.begin_sign_in(SignInMethod::DeviceCode);
                }
            } else if adding && widgets::secondary(ui, "Cancel").clicked() {
                app.signin.adding = false;
            }
        }
    }
    if let Some(e) = app.signin.error.clone() {
        ui.add_space(10.0);
        widgets::info_bar(ui, "info", &e, true, None);
    }
    ui.add_space(16.0);
    ui.separator();
    ui.add_space(8.0);
    if !app.sessions.iter().any(|s| s.demo) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Just looking?").color(t.text_dim));
            if ui.link("Try MailCraft with a sample mailbox").clicked() {
                app.start_demo();
            }
        });
        ui.add_space(6.0);
    }
    let header = if app.signin.show_advanced { "\u{25be} Advanced" } else { "\u{25b8} Advanced" };
    if ui.add(egui::Button::new(egui::RichText::new(header).font(regular(12.0)).color(t.text_dim)).frame(false)).clicked() {
        app.signin.show_advanced = !app.signin.show_advanced;
    }
    if app.signin.show_advanced {
        ui.add_space(4.0);
        ui.add(
            egui::Label::new(
                egui::RichText::new("Only if your organisation asks you to use its own app registration instead of MailCraft's: its Application (client) ID.").font(regular(12.0)).color(t.text_dim),
            )
            .wrap(),
        );
        ui.add(egui::TextEdit::singleline(&mut app.signin.client_id).hint_text("Leave empty to use MailCraft's").desired_width(340.0).margin(vec2(6.0, 4.0)));
    }
}

fn busy(app: &mut MailApp, ui: &mut Ui, t: &Tokens) {
    let email = app.signin.email.clone();
    if let Some(dc) = app.signin.device.clone() {
        ui.label(egui::RichText::new("Sign in on another device").font(semibold(15.0)));
        ui.add_space(6.0);
        ui.label(format!("On a phone or another computer, go to {} and enter this code:", dc.verification_uri));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&dc.user_code).font(semibold(26.0)).color(t.accent_text));
            if ui.button("Copy").clicked() {
                ui.ctx().copy_text(dc.user_code.clone());
            }
        });
    } else {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(egui::RichText::new(format!("Signing in as {email}\u{2026}")).font(semibold(14.0)));
        });
        ui.add_space(4.0);
        let text = match app.signin.method {
            Some(SignInMethod::InApp) => "Finish signing in in the Microsoft window. If your organisation asks, approve MailCraft's access to your mail and calendar.",
            _ => "Finish signing in in your browser. When it says you can close the page, come back here.",
        };
        ui.add(egui::Label::new(egui::RichText::new(text).color(t.text_dim)).wrap());
    }
    ui.add_space(12.0);
    if widgets::secondary(ui, "Cancel").clicked() {
        app.cancel_sign_in();
    }
}
