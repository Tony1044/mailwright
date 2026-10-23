//! MailCraft's sign-in window: Microsoft's sign-in page in a web view (WebView2 on Windows,
//! WebKitGTK on Linux), run as its own process (`mailcraft --sign-in-window <url>`). It closes
//! itself once the page sends the browser back to MailCraft's loopback address, where the main
//! process is waiting for the authorisation code; closing it early cancels the sign-in.

use std::path::Path;

use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::{NewWindowResponse, PageLoadEvent, WebContext, WebViewBuilder};

#[derive(Debug)]
enum Signal {
    Done,
}

fn is_loopback(url: &str) -> bool {
    url.starts_with("http://localhost:") || url.starts_with("http://127.0.0.1:")
}

pub fn run(url: &str, data_dir: &Path) -> Result<(), String> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("https://login.microsoftonline.com/") || lower.starts_with("https://login.live.com/") || lower.starts_with("http://127.0.0.1:")) {
        return Err("not a Microsoft sign-in address".into());
    }
    let event_loop = EventLoopBuilder::<Signal>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let window = WindowBuilder::new()
        .with_title("Sign in \u{2013} MailCraft")
        .with_inner_size(tao::dpi::LogicalSize::new(520.0, 720.0))
        .with_min_inner_size(tao::dpi::LogicalSize::new(400.0, 520.0))
        .build(&event_loop)
        .map_err(|e| e.to_string())?;
    let _ = std::fs::create_dir_all(data_dir);
    let mut context = WebContext::new(Some(data_dir.to_path_buf()));
    let builder = WebViewBuilder::new_with_web_context(&mut context)
        .with_url(url)
        .with_on_page_load_handler(move |event, url| {
            if matches!(event, PageLoadEvent::Finished) && is_loopback(&url) {
                let _ = proxy.send_event(Signal::Done);
            }
        })
        // Links that open new windows ("Can't access your account?") go to the browser.
        .with_new_window_req_handler(|url, _| {
            let _ = open::that_detached(url);
            NewWindowResponse::Deny
        });
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let _webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        let vbox = window.default_vbox().ok_or("no GTK container")?;
        builder.build_gtk(vbox).map_err(|e| e.to_string())?
    };
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    let _webview = builder.build(&window).map_err(|e| e.to_string())?;

    let mut closing_at: Option<std::time::Instant> = None;
    event_loop.run(move |event, _, flow| {
        *flow = match closing_at {
            Some(t) => ControlFlow::WaitUntil(t),
            None => ControlFlow::Wait,
        };
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => *flow = ControlFlow::Exit,
            // Leave "You're signed in" up for a moment, then go.
            Event::UserEvent(Signal::Done) => {
                let t = std::time::Instant::now() + std::time::Duration::from_millis(900);
                closing_at = Some(t);
                *flow = ControlFlow::WaitUntil(t);
            }
            Event::NewEvents(tao::event::StartCause::ResumeTimeReached { .. }) => *flow = ControlFlow::Exit,
            _ => {}
        }
    });
}
