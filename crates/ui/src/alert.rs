//! Mailwright's own Desktop Alert, as classic Outlook shows one: a small window in the
//! bottom-right corner with the sender, subject, and the start of the message, which fades
//! after a few seconds (or stays while the pointer is over it). Clicking it opens the message.
//! Used on Windows, where an app without an installer can't rely on Windows' own notifications.

use std::time::{Duration, Instant};

use egui::{Align2, Context, Sense, pos2, vec2};

use crate::app::MailApp;
use crate::theme::{Tokens, regular, semibold};

/// How long an alert stays up.
const SHOW_FOR: Duration = Duration::from_secs(7);
const SIZE: egui::Vec2 = egui::Vec2::new(372.0, 104.0);

pub struct DesktopAlert {
    pub account: String,
    /// The message to open when clicked (one new message), if any.
    pub id: Option<String>,
    pub title: String,
    pub subject: String,
    pub preview: String,
    pub count: usize,
    pub shown: Instant,
    pub hovered: bool,
    /// Not drawn yet: the clock starts when it first appears.
    pub fresh: bool,
}

impl DesktopAlert {
    pub fn new(account: &str, items: &[mailwright_sync::NewMail]) -> Option<DesktopAlert> {
        let first = items.first()?;
        let one_line = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
        Some(if items.len() == 1 {
            DesktopAlert {
                account: account.to_string(),
                id: Some(first.id.clone()),
                title: if first.from.is_empty() { "New mail".into() } else { first.from.clone() },
                subject: if first.subject.trim().is_empty() { "(no subject)".into() } else { one_line(&first.subject) },
                preview: one_line(&first.preview),
                count: 1,
                shown: Instant::now(),
                hovered: false,
                fresh: true,
            }
        } else {
            DesktopAlert {
                account: account.to_string(),
                id: None,
                title: format!("{} new messages", items.len()),
                subject: items.iter().take(3).map(|m| if m.from.is_empty() { one_line(&m.subject) } else { m.from.clone() }).collect::<Vec<_>>().join(", "),
                preview: account.to_string(),
                count: items.len(),
                shown: Instant::now(),
                hovered: false,
                fresh: true,
            }
        })
    }
}

enum Act {
    Open,
    Close,
}

pub fn show(app: &mut MailApp, ctx: &Context) {
    let Some(alert) = app.desktop_alert.as_mut() else { return };
    if alert.fresh {
        log::info!("desktop alert: {} (monitor {:?})", alert.title, ctx.input(|i| i.viewport().monitor_size));
    }
    if alert.hovered || alert.fresh {
        alert.fresh = false;
        alert.shown = Instant::now();
    }
    let left = SHOW_FOR.saturating_sub(alert.shown.elapsed());
    if left.is_zero() {
        app.desktop_alert = None;
        return;
    }
    // Fade over the last second.
    let fade = (left.as_secs_f32()).min(1.0);
    let monitor = ctx.input(|i| i.viewport().monitor_size).unwrap_or(vec2(1920.0, 1080.0));
    let pos = pos2(monitor.x - SIZE.x - 18.0, monitor.y - SIZE.y - 64.0);
    let builder = egui::ViewportBuilder::default()
        .with_title("New mail \u{2013} Mailwright")
        .with_decorations(false)
        .with_always_on_top()
        .with_taskbar(false)
        .with_resizable(false)
        .with_active(false)
        .with_inner_size(SIZE)
        .with_position(pos);
    let mut act = None;
    let (title, subject, preview) = (alert.title.clone(), alert.subject.clone(), alert.preview.clone());
    let openable = alert.id.is_some();
    let mut hovered = false;
    ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("desktop_alert"), builder, |ui, class| {
        if class == egui::ViewportClass::EmbeddedWindow {
            ui.set_min_size(SIZE);
        }
        let t = Tokens::get(ui.ctx());
        let full = ui.max_rect();
        let resp = ui.interact(full, egui::Id::new("alert_body"), Sense::click());
        hovered = resp.hovered() || ui.rect_contains_pointer(full);
        let p = ui.painter();
        p.rect_filled(full, 0.0, t.reading.gamma_multiply(fade.max(0.2)));
        p.rect_stroke(full.shrink(0.5), 0.0, egui::Stroke::new(1.0, t.border_strong.gamma_multiply(fade)), egui::StrokeKind::Inside);
        // The app colour down the left edge, and the icon.
        p.rect_filled(egui::Rect::from_min_size(full.min, vec2(4.0, full.height())), 0.0, crate::theme::APP_COLOR.gamma_multiply(fade));
        let icon = egui::Rect::from_min_size(full.min + vec2(16.0, 16.0), vec2(36.0, 36.0));
        crate::app_icon::paint(p, icon);
        let x = icon.max.x + 12.0;
        let w = full.max.x - x - 34.0;
        let text = |c: egui::Color32| c.gamma_multiply(fade);
        p.text(pos2(x, full.min.y + 22.0), Align2::LEFT_CENTER, crate::list::elide(p, &title, semibold(14.0), w), semibold(14.0), text(t.text));
        p.text(pos2(x, full.min.y + 44.0), Align2::LEFT_CENTER, crate::list::elide(p, &subject, regular(12.5), w + 20.0), regular(12.5), text(t.accent_text));
        p.text(pos2(x, full.min.y + 64.0), Align2::LEFT_CENTER, crate::list::elide(p, &preview, regular(12.0), w + 20.0), regular(12.0), text(t.text_dim));
        if openable {
            p.text(pos2(x, full.min.y + 86.0), Align2::LEFT_CENTER, "Click to open", regular(11.0), text(t.text_dim));
        }
        let close = egui::Rect::from_center_size(pos2(full.max.x - 18.0, full.min.y + 18.0), vec2(22.0, 22.0));
        let cr = ui.interact(close, egui::Id::new("alert_close"), Sense::click());
        if cr.hovered() {
            p.rect_filled(close, 3.0, t.hover);
        }
        let (c, k) = (close.center(), 4.5);
        let stroke = egui::Stroke::new(1.4, text(t.text_dim));
        p.line_segment([c + vec2(-k, -k), c + vec2(k, k)], stroke);
        p.line_segment([c + vec2(-k, k), c + vec2(k, -k)], stroke);
        if cr.clicked() {
            act = Some(Act::Close);
        } else if resp.clicked() {
            act = Some(Act::Open);
        }
        if ui.ctx().input(|i| i.viewport().close_requested()) {
            act = Some(Act::Close);
        }
    });
    if let Some(alert) = app.desktop_alert.as_mut() {
        alert.hovered = hovered;
    }
    match act {
        Some(Act::Open) => {
            if let Some(a) = app.desktop_alert.take() {
                app.open_alert(ctx, &a.account, a.id.as_deref());
            }
        }
        Some(Act::Close) => app.desktop_alert = None,
        None => ctx.request_repaint_after(Duration::from_millis(if left < Duration::from_secs(1) { 40 } else { 250 })),
    }
}
