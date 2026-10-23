//! MailCraft, the desktop app.

#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod graphics;
mod logging;
mod signin_window;

use mailcraft_ui::{MailApp, Settings};

struct App(MailApp);

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.0.logic(ctx);
        if self.0.quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.ui(ui);
    }

    fn on_exit(&mut self) {
        self.0.save_settings();
    }
}

fn main() -> eframe::Result {
    let config = mailcraft_ui::settings::config_dir();
    let data = mailcraft_ui::settings::data_dir();
    // The sign-in window, run by the main process as a child.
    let args: Vec<String> = std::env::args().collect();
    if let Some(p) = args.iter().position(|a| a == "--sign-in-window") {
        let url = args.get(p + 1).cloned().unwrap_or_default();
        if let Err(e) = signin_window::run(&url, &data.join("webview")) {
            eprintln!("sign-in window: {e}");
            std::process::exit(2);
        }
        return Ok(());
    }
    logging::init(&data);
    log::info!("MailCraft {} starting", env!("CARGO_PKG_VERSION"));
    let settings_path = config.join("settings.json");
    let settings = Settings::load(&settings_path);
    let demo = std::env::args().any(|a| a == "--sample-mailbox");
    // Opened as the mailto: handler: start a message to that address.
    let mailto = std::env::args().find_map(|a| a.strip_prefix("mailto:").map(|r| r.split('?').next().unwrap_or("").to_string()));

    let icon = egui::IconData { rgba: mailcraft_ui::app_icon::rgba(256), width: 256, height: 256 };
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("MailCraft")
            .with_app_id("ai.storyteller.mailcraft")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([820.0, 520.0])
            .with_drag_and_drop(true)
            .with_icon(icon),
        ..Default::default()
    };
    #[cfg(target_os = "windows")]
    graphics::configure(&mut options, eframe::wgpu::Backends::from_env());
    #[cfg(not(target_os = "windows"))]
    let _ = &mut options;
    eframe::run_native(
        "MailCraft",
        options,
        Box::new(move |cc| {
            let mut app = MailApp::new(settings, Some(settings_path), Some(data));
            if let Ok(exe) = std::env::current_exe() {
                app.sign_in_window = Some(std::sync::Arc::new(move |url: &str| std::process::Command::new(&exe).arg("--sign-in-window").arg(url).spawn()));
            }
            mailcraft_ui::theme::apply(&cc.egui_ctx, app.settings.appearance);
            if demo {
                app.start_demo();
            } else {
                app.resume();
            }
            if let Some(to) = mailto.filter(|t| !t.is_empty())
                && !app.sessions.is_empty() {
                    app.new_mail_to(&to);
                }
            Ok(Box::new(App(app)))
        }),
    )
}
