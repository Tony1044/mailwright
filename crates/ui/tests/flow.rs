#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unreachable)]
//! End to end through the interface's own commands, against the sample mailbox.

use std::time::{Duration, Instant};

use mailcraft_ui::{MailApp, Settings};

fn pump(app: &mut MailApp, ctx: &egui::Context, ms: u64, until: impl Fn(&MailApp) -> bool) -> bool {
    let end = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < end {
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        out.textures_delta.clear();
        if until(app) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn reply_flag_move_and_calendar_through_the_interface() {
    let data = std::env::temp_dir().join(format!("mailcraft-ui-flow-{}", std::process::id()));
    let mut app = MailApp::new(Settings::default(), None, Some(data.clone()));
    let ctx = egui::Context::default();
    pump(&mut app, &ctx, 200, |_| false);
    app.start_demo();
    assert!(pump(&mut app, &ctx, 15_000, |a| !a.mail.list.is_empty() && a.status.starts_with("All folders")), "the sample mailbox syncs");

    // Reply.
    let id = app.mail.list.iter().find(|m| m.subject.starts_with("Quick question")).unwrap().id.clone();
    app.select_message(&id, false, false);
    app.reply("reply");
    app.composes[0].body = "Yes, they're final.".into();
    assert_eq!(app.composes[0].to.trim(), "Raj Patel <raj.patel@harbourview.example>;");
    // Ctrl+Enter is what the window would do; call the same path.
    let sent_before = app.demo.as_ref().unwrap().state().sent.len();
    app.composes[0].subject = "RE: Quick question on the Q3 numbers".into();
    send_first_compose(&mut app);
    assert!(pump(&mut app, &ctx, 10_000, |a| a.demo.as_ref().unwrap().state().sent.len() > sent_before), "the reply reaches the server");
    let sent = app.demo.as_ref().unwrap().state().sent.last().cloned().unwrap();
    assert!(sent["body"]["content"].as_str().unwrap().contains("Yes, they&#39;re final.") || sent["body"]["content"].as_str().unwrap().contains("Yes, they're final."));
    assert!(sent["body"]["content"].as_str().unwrap().contains("divRplyFwdMsg"), "the original is quoted");

    // Flag and move.
    app.select_message(&id, false, false);
    app.flag_selected("flagged");
    let archive = app.folder_wk("archive").unwrap();
    app.move_selected(&archive);
    assert!(pump(&mut app, &ctx, 10_000, |a| a.demo.as_ref().unwrap().state().messages.iter().any(|m| m.folder == "AAMkArchive" && m.props["flag"]["flagStatus"] == "flagged")));

    // A new appointment.
    app.set_view(mailcraft_ui::View::Calendar);
    app.new_event(false, None);
    app.editors[0].subject = "Dentist".into();
    app.editors[0].open = true;
    save_first_editor(&mut app, &ctx);
    assert!(pump(&mut app, &ctx, 10_000, |a| a.demo.as_ref().unwrap().state().events.iter().any(|e| e.props["subject"] == "Dentist")));

    drop(app);
    let _ = std::fs::remove_dir_all(data);
}

fn send_first_compose(app: &mut MailApp) {
    app.test_send_compose(0);
}

fn save_first_editor(app: &mut MailApp, _ctx: &egui::Context) {
    app.test_save_event(0);
}
