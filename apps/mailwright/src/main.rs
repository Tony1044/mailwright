//! Mailwright, the desktop app.

#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod graphics;
mod logging;
#[cfg(all(unix, not(target_os = "macos")))]
mod signin_window;

use mailwright_ui::{MailApp, Settings};

struct App(MailApp, std::time::Instant);

/// At most this many frames a second. On Linux, frames aren't paced by the display (see
/// `main`), so this keeps an animation such as the sync spinner from running flat out.
const MAX_FPS: u64 = 60;

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if cfg!(all(unix, not(target_os = "macos"))) {
            let frame = std::time::Duration::from_millis(1000 / MAX_FPS);
            let since = self.1.elapsed();
            if since < frame {
                std::thread::sleep(frame - since);
            }
            self.1 = std::time::Instant::now();
        }
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

/// How Mailwright shows Microsoft's sign-in page in a window of its own.
#[cfg(all(unix, not(target_os = "macos")))]
fn sign_in_window(_data: &std::path::Path) -> Option<mailwright_ui::app::SignInWindow> {
    let exe = std::env::current_exe().ok()?;
    Some(std::sync::Arc::new(move |url: &str| std::process::Command::new(&exe).arg("--sign-in-window").arg(url).spawn()))
}

/// Windows: Microsoft Edge (part of Windows 10 and 11) as a bare app window, with its own profile.
#[cfg(target_os = "windows")]
fn sign_in_window(data: &std::path::Path) -> Option<mailwright_ui::app::SignInWindow> {
    let edge = ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"]
        .iter()
        .filter_map(|v| std::env::var_os(v))
        .map(|base| std::path::PathBuf::from(base).join("Microsoft").join("Edge").join("Application").join("msedge.exe"))
        .find(|p| p.exists())?;
    let profile = data.join("signin-profile");
    Some(std::sync::Arc::new(move |url: &str| {
        std::process::Command::new(&edge)
            .arg(format!("--app={url}"))
            .arg(format!("--user-data-dir={}", profile.display()))
            .args(["--no-first-run", "--no-default-browser-check", "--window-size=520,720", "--disable-sync"])
            .spawn()
    }))
}

#[cfg(not(any(target_os = "windows", all(unix, not(target_os = "macos")))))]
fn sign_in_window(_data: &std::path::Path) -> Option<mailwright_ui::app::SignInWindow> {
    None
}

/// Windows: register the identity new-mail alerts are shown under (name and icon), for this
/// user only. Windows needs this for an app without an installer to show its own name on alerts.
#[cfg(target_os = "windows")]
fn register_alert_identity(data: std::path::PathBuf) {
    use std::os::windows::process::CommandExt as _;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::thread::spawn(move || {
        let icon = data.join("mailwright-icon.png");
        let _ = std::fs::create_dir_all(&data);
        if std::fs::write(&icon, mailwright_ui::app_icon::png(64)).is_err() {
            return;
        }
        let key = format!(r"HKCU\Software\Classes\AppUserModelId\{}", mailwright_ui::notify::APP_ID);
        for (name, value) in [("DisplayName", mailwright_ui::notify::APP_NAME.to_string()), ("IconUri", icon.display().to_string())] {
            let r = std::process::Command::new("reg").args(["add", &key, "/v", name, "/t", "REG_SZ", "/d", &value, "/f"]).creation_flags(CREATE_NO_WINDOW).output();
            if let Err(e) = r {
                log::warn!("registering alerts: {e}");
            }
        }
    });
}

fn main() -> eframe::Result {
    mailwright_ui::settings::migrate_old_folders();
    let config = mailwright_ui::settings::config_dir();
    let data = mailwright_ui::settings::data_dir();
    // The sign-in window, run by the main process as a child.
    let args: Vec<String> = std::env::args().collect();
    let _ = &args;
    #[cfg(all(unix, not(target_os = "macos")))]
    if let Some(p) = args.iter().position(|a| a == "--sign-in-window") {
        let url = args.get(p + 1).cloned().unwrap_or_default();
        if let Err(e) = signin_window::run(&url, &data.join("webview")) {
            eprintln!("sign-in window: {e}");
            std::process::exit(2);
        }
        return Ok(());
    }
    logging::init(&data);
    log::info!("Mailwright {} starting", env!("CARGO_PKG_VERSION"));
    #[cfg(target_os = "windows")]
    register_alert_identity(data.clone());
    let settings_path = config.join("settings.json");
    let settings = Settings::load(&settings_path);
    let demo = std::env::args().any(|a| a == "--sample-mailbox");
    // Opened as the mailto: handler: start a message to that address.
    let mailto = std::env::args().find_map(|a| a.strip_prefix("mailto:").map(|r| r.split('?').next().unwrap_or("").to_string()));

    let icon = egui::IconData { rgba: mailwright_ui::app_icon::rgba(256), width: 256, height: 256 };
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Mailwright")
            .with_app_id("ai.storyteller.mailwright")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([820.0, 520.0])
            .with_drag_and_drop(true)
            .with_icon(icon),
        ..Default::default()
    };
    #[cfg(target_os = "windows")]
    graphics::configure(&mut options, eframe::wgpu::Backends::from_env());
    // Linux: don't wait on the display for each frame. With vsync, Wayland compositors stop
    // answering while the window is hidden (another workspace, minimised, screen locked), and the
    // whole interface can stay frozen. MAILWRIGHT_VSYNC=1 turns it back on.
    #[cfg(all(unix, not(target_os = "macos")))]
    if std::env::var_os("MAILWRIGHT_VSYNC").is_none() {
        options.wgpu_options.surface.present_mode = eframe::wgpu::PresentMode::AutoNoVsync;
    }
    #[cfg(not(target_os = "windows"))]
    let _ = &mut options;
    eframe::run_native(
        "Mailwright",
        options,
        Box::new(move |cc| {
            let mut app = MailApp::new(settings, Some(settings_path), Some(data));
            app.sign_in_window = sign_in_window(&mailwright_ui::settings::data_dir());
            mailwright_ui::theme::apply(&cc.egui_ctx, app.settings.appearance);
            if demo {
                app.start_demo();
            } else {
                app.resume();
            }
            if let Some(to) = mailto.filter(|t| !t.is_empty())
                && !app.sessions.is_empty() {
                    app.new_mail_to(&to);
                }
            Ok(Box::new(App(app, std::time::Instant::now())))
        }),
    )
}
