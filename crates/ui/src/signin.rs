//! The first screen: sign in to Microsoft 365 (Exchange Online), or try the sample mailbox.

use egui::{Ui, vec2};
use mailwright_graph::GraphError;

use crate::app::{MailApp, SignInMethod};
use crate::theme::{Tokens, regular, semibold};
use crate::widgets;

/// What a sign-in failure means, in words.
pub fn explain(e: &GraphError) -> String {
    match e {
        GraphError::Network { host, .. } => format!("Can't reach {host}. Check your internet connection (and proxy) and try again."),
        GraphError::SignInRequired(m) if m.contains("AADSTS700016") => {
            "Microsoft couldn't find Mailwright's app registration. If your organisation gave you its own Application (client) ID, check it under Advanced.".into()
        }
        GraphError::SignInRequired(m) | GraphError::SignInCancelled(m) if m.contains("AADSTS65001") || m.contains("AADSTS90094") || m.contains("consent") || m.contains("admin approval") => {
            "Your organisation needs an administrator to approve Mailwright before you can sign in. Ask your IT team to approve \u{201c}Mailwright\u{201d} (they can do it from the sign-in page with an admin account, or in Microsoft Entra under Enterprise applications).".into()
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
    if app.signin.manual.is_some() {
        manual_card(app, ui, t);
        return;
    }
    let adding = app.signin.adding && !app.sessions.is_empty();
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(40.0, 40.0), egui::Sense::hover());
        crate::app_icon::paint(ui.painter(), r);
        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(if adding { "Add an account" } else { "Welcome to Mailwright" }).font(semibold(22.0)));
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
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Not Microsoft 365?").color(t.text_dim));
        if ui.link("Set up an IMAP or POP account").clicked() {
            app.begin_manual();
        }
    });
    ui.add_space(6.0);
    if !app.sessions.iter().any(|s| s.demo) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Just looking?").color(t.text_dim));
            if ui.link("Try Mailwright with a sample mailbox").clicked() {
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
                egui::RichText::new("Only if your organisation asks you to use its own app registration instead of Mailwright's: its Application (client) ID.").font(regular(12.0)).color(t.text_dim),
            )
            .wrap(),
        );
        ui.add(egui::TextEdit::singleline(&mut app.signin.client_id).hint_text("Leave empty to use Mailwright's").desired_width(340.0).margin(vec2(6.0, 4.0)));
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
            Some(SignInMethod::InApp) => "Finish signing in in the Microsoft window. If your organisation asks, approve Mailwright's access to your mail and calendar.",
            _ => "Finish signing in in your browser. When it says you can close the page, come back here.",
        };
        ui.add(egui::Label::new(egui::RichText::new(text).color(t.text_dim)).wrap());
    }
    ui.add_space(12.0);
    if widgets::secondary(ui, "Cancel").clicked() {
        app.cancel_sign_in();
    }
}

/// The IMAP/POP3 + SMTP setup form.
fn manual_card(app: &mut MailApp, ui: &mut Ui, t: &Tokens) {
    use mailwright_graph::{MailProtocol, Security};
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(40.0, 40.0), egui::Sense::hover());
        crate::app_icon::paint(ui.painter(), r);
        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.label(egui::RichText::new("Set up a mail account").font(semibold(22.0)));
            ui.label(egui::RichText::new("IMAP or POP3 for receiving, SMTP for sending").color(t.text_dim));
        });
    });
    ui.add_space(14.0);
    let Some(f) = app.signin.manual.as_mut() else { return };
    if f.busy {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(egui::RichText::new("Checking the servers and your sign-in\u{2026}").color(t.text_dim));
        });
        return;
    }
    let field = |ui: &mut Ui, label: &str, value: &mut String, hint: &str, width: f32| {
        ui.horizontal(|ui| {
            ui.add_sized(vec2(130.0, 24.0), egui::Label::new(egui::RichText::new(label).color(t.text_dim)));
            ui.add(egui::TextEdit::singleline(value).hint_text(hint).desired_width(width).margin(vec2(6.0, 4.0)));
        });
    };
    field(ui, "Email address", &mut f.email, "name@example.com", 300.0);
    field(ui, "Your name", &mut f.name, "shown on messages you send", 300.0);
    ui.horizontal(|ui| {
        ui.add_sized(vec2(130.0, 24.0), egui::Label::new(egui::RichText::new("Password").color(t.text_dim)));
        ui.add(egui::TextEdit::singleline(&mut f.password).password(true).hint_text("your password or app password").desired_width(300.0).margin(vec2(6.0, 4.0)));
    });
    if !f.note.trim().is_empty() {
        ui.add_space(4.0);
        ui.add(egui::Label::new(egui::RichText::new(&f.note).font(regular(12.0)).color(t.high)).wrap());
    }
    ui.add_space(10.0);

    // Incoming.
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Incoming mail").font(semibold(13.0)));
        egui::ComboBox::from_id_salt("in_proto").selected_text(if f.protocol == MailProtocol::Imap { "IMAP" } else { "POP3" }).show_ui(ui, |ui| {
            ui.selectable_value(&mut f.protocol, MailProtocol::Imap, "IMAP (keeps mail on the server)");
            ui.selectable_value(&mut f.protocol, MailProtocol::Pop, "POP3 (downloads mail)");
        });
    });
    server_row(ui, t, "Server", &mut f.in_host, &mut f.in_port, &mut f.in_security, incoming_kind(f.protocol));
    ui.add_space(8.0);

    // Outgoing.
    ui.label(egui::RichText::new("Outgoing mail (SMTP)").font(semibold(13.0)));
    server_row(ui, t, "Server", &mut f.smtp_host, &mut f.smtp_port, &mut f.smtp_security, mailwright_net::providers::Kind::Smtp);
    ui.add_space(12.0);

    if let Some(e) = &f.error {
        widgets::info_bar(ui, "info", e, true, None);
        ui.add_space(8.0);
    }
    let mut add = false;
    let mut cancel = false;
    ui.horizontal(|ui| {
        if widgets::primary(ui, "Add account").clicked() {
            add = true;
        }
        if widgets::secondary(ui, "Cancel").clicked() {
            cancel = true;
        }
    });
    let _ = Security::Tls;
    if add {
        app.test_and_add_manual();
    }
    if cancel {
        app.signin.manual = None;
    }
}

fn incoming_kind(p: mailwright_graph::MailProtocol) -> mailwright_net::providers::Kind {
    match p {
        mailwright_graph::MailProtocol::Imap => mailwright_net::providers::Kind::Imap,
        mailwright_graph::MailProtocol::Pop => mailwright_net::providers::Kind::Pop,
    }
}

fn server_row(ui: &mut Ui, t: &Tokens, label: &str, host: &mut String, port: &mut String, security: &mut mailwright_graph::Security, kind: mailwright_net::providers::Kind) {
    use mailwright_graph::Security;
    ui.horizontal(|ui| {
        ui.add_sized(vec2(130.0, 24.0), egui::Label::new(egui::RichText::new(label).color(t.text_dim)));
        ui.add(egui::TextEdit::singleline(host).hint_text("imap.example.com").desired_width(240.0).margin(vec2(6.0, 4.0)));
        ui.label(egui::RichText::new("Port").color(t.text_dim));
        ui.add(egui::TextEdit::singleline(port).desired_width(60.0).margin(vec2(6.0, 4.0)));
    });
    ui.horizontal(|ui| {
        ui.add_space(130.0);
        let before = *security;
        egui::ComboBox::from_id_salt(("sec", label as *const str as *const () as usize + host.len()))
            .selected_text(match security {
                Security::Tls => "SSL/TLS",
                Security::StartTls => "STARTTLS",
                Security::None => "None",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(security, Security::Tls, "SSL/TLS");
                ui.selectable_value(security, Security::StartTls, "STARTTLS");
                ui.selectable_value(security, Security::None, "None");
            });
        // When the security changes, offer the standard port for it.
        if *security != before {
            *port = mailwright_net::providers::default_port(kind, *security).to_string();
        }
    });
}
