#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unreachable)]
//! Headless screenshots of MailCraft against the sample mailbox.
//!
//! `cargo run -p mailcraft-ui --example ui_shot -- out_dir [steps…]` where each step is a command
//! (see `run`) or `shot:name` to save `out_dir/name.png`. Rendering uses wgpu (a software adapter
//! is fine).

use std::time::{Duration, Instant};

use mailcraft_ui::{MailApp, Settings, View};

fn step(h: &mut egui_kittest::Harness<'static, MailApp>, n: usize) {
    for _ in 0..n {
        h.step();
    }
}

fn wait(h: &mut egui_kittest::Harness<'static, MailApp>, ms: u64, until: impl Fn(&MailApp) -> bool) {
    let end = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < end {
        step(h, 1);
        if until(h.state()) {
            break;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    step(h, 3);
}

fn run(h: &mut egui_kittest::Harness<'static, MailApp>, cmd: &str) {
    let (verb, arg) = cmd.split_once(':').unwrap_or((cmd, ""));
    match verb {
        "demo" => {
            h.state_mut().start_demo();
            wait(h, 15_000, |a| !a.mail.list.is_empty() && a.status.starts_with("All folders"));
            wait(h, 1500, |_| false);
        }
        "subject" => {
            let id = h.state().mail.list.iter().find(|m| m.subject.starts_with(arg)).map(|m| m.id.clone());
            if let Some(id) = id {
                h.state_mut().select_message(&id, false, false);
                wait(h, 3000, |a| a.mail.reading.as_ref().is_some_and(|r| r.doc.is_some()));
                wait(h, 500, |_| false);
            } else {
                eprintln!("no message starting {arg:?}");
            }
        }
        "folder" => {
            let id = h.state().mail.folders.iter().find(|f| f.name == arg).map(|f| f.id.clone());
            if let Some(id) = id {
                h.state_mut().select_folder(&id);
                wait(h, 3000, |a| !a.mail.list.is_empty());
            }
        }
        "other" => {
            h.state_mut().mail.other_tab = true;
            h.state_mut().mail.list_dirty = true;
            step(h, 4);
        }
        "view" => {
            let v = if arg == "calendar" { View::Calendar } else { View::Mail };
            h.state_mut().set_view(v);
            wait(h, 2500, |a| !a.cal.events.is_empty() || v == View::Mail);
        }
        "cal" => {
            let v = match arg {
                "day" => mailcraft_ui::settings::CalendarView::Day,
                "week" => mailcraft_ui::settings::CalendarView::Week,
                "month" => mailcraft_ui::settings::CalendarView::Month,
                _ => mailcraft_ui::settings::CalendarView::WorkWeek,
            };
            h.state_mut().cal.set_view(v);
            wait(h, 1500, |_| false);
        }
        "reply" => {
            h.state_mut().reply(if arg.is_empty() { "reply" } else { arg });
            step(h, 6);
        }
        "new-mail" => {
            h.state_mut().new_mail();
            step(h, 6);
        }
        "new-event" => {
            h.state_mut().new_event(arg == "meeting", None);
            step(h, 6);
        }
        "open-event" => {
            let id = h.state().cal.events.iter().find(|e| e.subject.starts_with(arg)).map(|e| e.id.clone());
            if let Some(id) = id {
                h.state_mut().open_event(&id);
                wait(h, 1500, |_| false);
            }
        }
        "close-windows" => {
            h.state_mut().composes.clear();
            h.state_mut().editors.clear();
            step(h, 3);
        }
        "tab" => {
            h.state_mut().tab = arg.parse().unwrap_or(1);
            step(h, 3);
        }
        "backstage" => {
            h.state_mut().backstage = match arg {
                "options" => Some(mailcraft_ui::app::Backstage::Options),
                "about" => Some(mailcraft_ui::app::Backstage::About),
                "" | "none" => None,
                _ => Some(mailcraft_ui::app::Backstage::Info),
            };
            step(h, 4);
        }
        "theme" => {
            let a = if arg == "dark" { mailcraft_ui::theme::Appearance::Dark } else { mailcraft_ui::theme::Appearance::Light };
            h.state_mut().settings.appearance = a;
            mailcraft_ui::theme::set_appearance(&h.ctx, a);
            step(h, 4);
        }
        "pane" => {
            h.state_mut().settings.reading_pane = match arg {
                "bottom" => mailcraft_ui::settings::ReadingPane::Bottom,
                "off" => mailcraft_ui::settings::ReadingPane::Off,
                _ => mailcraft_ui::settings::ReadingPane::Right,
            };
            step(h, 4);
        }
        "search" => {
            h.state_mut().mail.search = arg.to_string();
            h.state_mut().start_search();
            step(h, 4);
        }
        "clear-search" => {
            h.state_mut().clear_search();
            step(h, 4);
        }
        "allow-remote" => {
            if let Some(r) = h.state_mut().mail.reading.as_mut() {
                r.allow_remote = true;
            }
            step(h, 3);
        }
        "wait" => wait(h, arg.parse().unwrap_or(500), |_| false),
        other => eprintln!("unknown step {other}"),
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let out = std::path::PathBuf::from(args.next().unwrap_or_else(|| "shots".into()));
    let _ = std::fs::create_dir_all(&out);
    let steps: Vec<String> = args.collect();
    let data = std::env::temp_dir().join(format!("mailcraft-shot-{}", std::process::id()));
    let mut settings = Settings::default();
    settings.appearance = mailcraft_ui::theme::Appearance::Light;
    let app = MailApp::new(settings, None, Some(data.clone()));
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .with_pixels_per_point(1.5)
        .with_max_steps(1_000_000)
        .wgpu()
        .build_ui_state(
            |ui, app: &mut MailApp| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                app.ui(ui);
            },
            app,
        );
    step(&mut h, 4);
    for s in steps {
        if let Some(name) = s.strip_prefix("shot:") {
            step(&mut h, 4);
            match h.render() {
                Ok(img) => {
                    let p = out.join(format!("{name}.png"));
                    if let Err(e) = img.save(&p) {
                        eprintln!("save {}: {e}", p.display());
                    } else {
                        println!("{}", p.display());
                    }
                }
                Err(e) => eprintln!("render failed: {e}"),
            }
        } else {
            run(&mut h, &s);
        }
    }
    drop(h);
    let _ = std::fs::remove_dir_all(data);
}
