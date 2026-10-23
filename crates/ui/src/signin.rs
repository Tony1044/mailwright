//! The first screen: sign in to Microsoft 365 (Exchange Online), or try the sample mailbox.

use egui::{Ui, vec2};
use mailcraft_graph::GraphError;

use crate::app::MailApp;
use crate::theme::{Tokens, regular, semibold};
use crate::widgets;

/// What a sign-in failure means, in words.
pub fn explain(e: &GraphError) -> String {
    match e {
        GraphError::Network { host, .. } => format!("Can't reach {host}. Check your internet connection (and proxy) and try again."),
        GraphError::SignInRequired(m) if m.contains("AADSTS700016") => {
            "Microsoft Entra doesn't know that Application (client) ID in this tenant. Check the ID, and that the app is registered in your organisation (or set to accept any organisation).".into()
        }
        GraphError::SignInRequired(m) if m.contains("AADSTS65001") || m.contains("consent") => {
            "Your organisation needs to approve MailCraft's permissions first. Ask an administrator to grant admin consent for the app registration.".into()
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
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(40.0, 40.0), egui::Sense::hover());
        crate::app_icon::paint(ui.painter(), r);
        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.label(egui::RichText::new("Welcome to MailCraft").font(semibold(22.0)));
            ui.label(egui::RichText::new("Mail and calendar for Microsoft 365 (Exchange Online)").color(t.text_dim));
        });
    });
    ui.add_space(18.0);
    if app.signin.busy {
        if let Some(dc) = app.signin.device.clone() {
            ui.label(egui::RichText::new("Sign in on another device").font(semibold(15.0)));
            ui.add_space(6.0);
            ui.label(format!("Go to {} and enter this code:", dc.verification_uri));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&dc.user_code).font(semibold(26.0)).color(t.accent_text));
                if ui.button("Copy").clicked() {
                    ui.ctx().copy_text(dc.user_code.clone());
                }
                if ui.button("Open the page").clicked() {
                    let _ = open::that_detached(&dc.verification_uri);
                }
            });
        } else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Finish signing in in your browser\u{2026}");
            });
            ui.label(egui::RichText::new("A Microsoft sign-in page has opened. When it says you can close it, come back here.").color(t.text_dim));
        }
        ui.add_space(12.0);
        if widgets::secondary(ui, "Cancel").clicked() {
            app.cancel_sign_in();
        }
        return;
    }
    ui.label(egui::RichText::new("Sign in with your work or school account").font(semibold(15.0)));
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new("MailCraft talks to Exchange Online through Microsoft Graph, using an app registration in your organisation\u{2019}s Microsoft Entra ID. Enter its Application (client) ID once; your password never passes through MailCraft.")
            .color(t.text_dim),
    );
    ui.add_space(12.0);
    egui::Grid::new("signin_grid").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
        ui.label("Application (client) ID");
        ui.add(egui::TextEdit::singleline(&mut app.signin.client_id).hint_text("00000000-0000-0000-0000-000000000000").desired_width(320.0).margin(vec2(6.0, 4.0)));
        ui.end_row();
        ui.label("Tenant");
        ui.add(egui::TextEdit::singleline(&mut app.signin.tenant).hint_text("organizations, contoso.com or a tenant ID").desired_width(320.0).margin(vec2(6.0, 4.0)));
        ui.end_row();
    });
    if let Some(e) = app.signin.error.clone() {
        ui.add_space(8.0);
        widgets::info_bar(ui, "info", &e, true, None);
    }
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        if widgets::primary(ui, "Sign in").on_hover_text("Opens Microsoft's sign-in page in your browser").clicked() {
            app.begin_sign_in(false);
        }
        if widgets::secondary(ui, "Use a code instead").on_hover_text("Sign in on another device, for when a browser on this computer can't").clicked() {
            app.begin_sign_in(true);
        }
    });
    ui.add_space(16.0);
    ui.separator();
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Just looking?").color(t.text_dim));
        if ui.link("Try MailCraft with a sample mailbox").clicked() {
            app.start_demo();
        }
    });
    ui.add_space(10.0);
    let header = if app.signin.show_help { "\u{25be} Setting up the app registration" } else { "\u{25b8} Setting up the app registration" };
    if ui.add(egui::Button::new(egui::RichText::new(header).font(semibold(12.5))).frame(false)).clicked() {
        app.signin.show_help = !app.signin.show_help;
    }
    if app.signin.show_help {
        ui.add_space(4.0);
        let steps = [
            "In the Microsoft Entra admin centre (entra.microsoft.com), open App registrations \u{203a} New registration. Name it MailCraft and choose who can use it (usually \u{201c}Accounts in this organisational directory only\u{201d}).",
            "Under Redirect URI, choose \u{201c}Public client/native (mobile & desktop)\u{201d} and enter http://localhost. Register.",
            "Copy the Application (client) ID from the Overview page into the box above. For the tenant, use your domain or the Directory (tenant) ID (or leave \u{201c}organizations\u{201d}).",
            "Under Authentication, set \u{201c}Allow public client flows\u{201d} to Yes (needed for \u{201c}Use a code instead\u{201d}).",
            "Under API permissions, add Microsoft Graph delegated permissions: User.Read, Mail.ReadWrite, Mail.Send, Calendars.ReadWrite, People.Read and offline_access. If your organisation requires it, select \u{201c}Grant admin consent\u{201d}.",
        ];
        for (i, s) in steps.iter().enumerate() {
            ui.horizontal_top(|ui| {
                ui.label(egui::RichText::new(format!("{}.", i + 1)).font(semibold(12.5)));
                ui.add(egui::Label::new(egui::RichText::new(*s).font(regular(12.5))).wrap());
            });
            ui.add_space(3.0);
        }
    }
}
