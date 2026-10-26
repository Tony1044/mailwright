//! New-mail alerts: the sound, and (on Linux) the desktop's own notification and dock count.
//! Windows gets Mailwright's own Desktop Alert window instead (see `alert`). Everything here runs
//! off the interface thread.

use std::path::PathBuf;
use std::sync::mpsc::{Sender, channel};

use mailwright_sync::NewMail;

/// The app's name as notifications show it.
pub const APP_NAME: &str = "Mailwright";
/// Linux: the desktop file's name (without `.desktop`), which is also the icon's name.
pub const DESKTOP_ID: &str = "ai.storyteller.mailwright";

/// One batch of new mail for one account.
pub struct Alert {
    pub account: String,
    pub items: Vec<NewMail>,
    pub popup: bool,
    pub sound: bool,
}

/// Clicked a desktop alert: open this message.
pub struct Clicked {
    pub account: String,
    pub id: String,
}

/// What the alert says: (title, body).
pub fn text(alert: &Alert) -> (String, String) {
    let one_line = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    match alert.items.as_slice() {
        [m] => {
            let from = if m.from.is_empty() { "New mail".to_string() } else { m.from.clone() };
            let subject = if m.subject.trim().is_empty() { "(no subject)".to_string() } else { one_line(&m.subject) };
            let preview: String = one_line(&m.preview).chars().take(160).collect();
            (from, if preview.is_empty() { subject } else { format!("{subject}\n{preview}") })
        }
        items => {
            let mut lines: Vec<String> = items.iter().take(4).map(|m| format!("{}: {}", if m.from.is_empty() { "?" } else { &m.from }, one_line(&m.subject))).collect();
            if items.len() > 4 {
                lines.push(format!("and {} more", items.len() - 4));
            }
            (format!("{} new messages for {}", items.len(), alert.account), lines.join("\n"))
        }
    }
}

/// Raise the alert: sound and pop-up, in the background.
pub fn announce(alert: Alert, clicked: Option<Sender<Clicked>>, wake: impl Fn() + Send + 'static) {
    if alert.items.is_empty() || !(alert.popup || alert.sound) {
        return;
    }
    let spawned = std::thread::Builder::new().name("new-mail-alert".into()).spawn(move || {
        if alert.sound {
            play_sound();
        }
        if alert.popup {
            popup(&alert, clicked, wake);
        }
    });
    if let Err(e) = spawned {
        log::warn!("new-mail alert: {e}");
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn popup(alert: &Alert, clicked: Option<Sender<Clicked>>, wake: impl Fn()) {
    use notify_rust::{Hint, Notification, Timeout};
    let (title, body) = text(alert);
    let mut n = Notification::new();
    n.appname(APP_NAME)
        .summary(&title)
        .body(&escape(&body))
        .icon(DESKTOP_ID)
        .hint(Hint::Category("email.arrived".into()))
        .hint(Hint::DesktopEntry(DESKTOP_ID.into()))
        .timeout(Timeout::Milliseconds(8000));
    if alert.items.len() == 1 {
        n.action("default", "Open");
    }
    match n.show() {
        Ok(handle) => {
            if let (Some(tx), [m]) = (clicked, alert.items.as_slice()) {
                let (account, id) = (alert.account.clone(), m.id.clone());
                handle.wait_for_action(|action| {
                    if action == "default" {
                        let _ = tx.send(Clicked { account, id });
                        wake();
                    }
                });
            }
        }
        Err(e) => log::warn!("desktop alert: {e}"),
    }
}

/// Notification bodies are a small subset of HTML on Linux.
#[cfg(all(unix, not(target_os = "macos")))]
fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Elsewhere (Windows), Mailwright shows its own Desktop Alert window instead.
#[cfg(not(all(unix, not(target_os = "macos"))))]
fn popup(_alert: &Alert, _clicked: Option<Sender<Clicked>>, _wake: impl Fn()) {}

/// The new-mail sound: the system's own where there is one.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn play_sound() {
    use std::process::{Command, Stdio};
    let quiet = |c: &mut Command| c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
    if quiet(Command::new("canberra-gtk-play").args(["--id=message-new-email", "--description=New mail"])) {
        return;
    }
    let theme = ["message-new-email.oga", "message-new-instant.oga", "message.oga"]
        .iter()
        .map(|f| PathBuf::from("/usr/share/sounds/freedesktop/stereo").join(f))
        .find(|p| p.exists());
    if let Some(f) = &theme {
        for player in ["pw-play", "paplay"] {
            if quiet(Command::new(player).arg(f)) {
                return;
            }
        }
    }
    let Some(wav) = chime_file() else { return };
    for (player, args) in [("pw-play", vec![]), ("paplay", vec![]), ("aplay", vec!["-q"])] {
        if quiet(Command::new(player).args(args).arg(&wav)) {
            return;
        }
    }
    log::info!("no sound player found for the new-mail sound");
}

#[cfg(target_os = "windows")]
pub fn play_sound() {
    // The sound Windows (and Outlook) use for new mail, unless it has been removed.
    let windir = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let system = windir.join("Media").join("Windows Notify Email.wav");
    let file = if system.exists() { Some(system) } else { chime_file() };
    match file {
        Some(f) if mailwright_winshell::play_wav(&f) => {}
        Some(f) => log::warn!("couldn't play {}", f.display()),
        None => {}
    }
}

#[cfg(not(any(target_os = "windows", all(unix, not(target_os = "macos")))))]
pub fn play_sound() {}

/// Mailwright's own chime, for systems without a sound theme: written once to the cache.
fn chime_file() -> Option<PathBuf> {
    let dir = dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join(APP_NAME);
    let path = dir.join("new-mail.wav");
    if !path.exists() {
        std::fs::create_dir_all(&dir).ok()?;
        std::fs::write(&path, chime_wav()).ok()?;
    }
    Some(path)
}

/// Two soft rising notes, as a 16-bit mono WAV.
pub fn chime_wav() -> Vec<u8> {
    const RATE: u32 = 22_050;
    let mut samples: Vec<i16> = Vec::new();
    for (freq, start, len) in [(880.0f32, 0.0f32, 0.32f32), (1318.5, 0.14, 0.45)] {
        let first = (start * RATE as f32) as usize;
        let n = (len * RATE as f32) as usize;
        if samples.len() < first + n {
            samples.resize(first + n, 0);
        }
        for i in 0..n {
            let t = i as f32 / RATE as f32;
            let attack = (t / 0.01).min(1.0);
            let decay = (-t * 7.0).exp();
            let v = (2.0 * std::f32::consts::PI * freq * t).sin() * attack * decay * 0.35 * i16::MAX as f32;
            samples[first + i] = samples[first + i].saturating_add(v as i16);
        }
    }
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// The unread count on the dock icon (Linux docks that follow the Unity launcher API).
pub struct Badge {
    tx: Option<Sender<i64>>,
    last: Option<i64>,
}

impl Badge {
    pub fn new() -> Badge {
        Badge { tx: badge_thread(), last: None }
    }

    pub fn set(&mut self, count: i64) {
        if self.last == Some(count) {
            return;
        }
        self.last = Some(count);
        if let Some(tx) = &self.tx {
            let _ = tx.send(count);
        }
    }
}

impl Default for Badge {
    fn default() -> Self {
        Badge::new()
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn badge_thread() -> Option<Sender<i64>> {
    let (tx, rx) = channel::<i64>();
    std::thread::Builder::new()
        .name("dock-badge".into())
        .spawn(move || {
            let conn = match zbus::blocking::Connection::session() {
                Ok(c) => c,
                Err(e) => {
                    log::info!("no session bus for the dock badge: {e}");
                    return;
                }
            };
            for count in rx {
                let mut props: std::collections::HashMap<&str, zbus::zvariant::Value<'_>> = std::collections::HashMap::new();
                props.insert("count", zbus::zvariant::Value::I64(count));
                props.insert("count-visible", zbus::zvariant::Value::Bool(count > 0));
                let r = conn.emit_signal(
                    None::<zbus::names::BusName<'_>>,
                    "/com/canonical/unity/launcherentry/mailwright",
                    "com.canonical.Unity.LauncherEntry",
                    "Update",
                    &(format!("application://{DESKTOP_ID}.desktop"), props),
                );
                if let Err(e) = r {
                    log::info!("dock badge: {e}");
                }
            }
        })
        .ok()?;
    Some(tx)
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn badge_thread() -> Option<Sender<i64>> {
    let _ = channel::<i64>;
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mail(from: &str, subject: &str) -> NewMail {
        NewMail { id: "1".into(), from: from.into(), subject: subject.into(), preview: "Hello\n there".into(), focused: true }
    }

    #[test]
    fn one_message_shows_sender_subject_and_preview() {
        let a = Alert { account: "me@example.com".into(), items: vec![mail("Sam Lee", "Lunch?")], popup: true, sound: true };
        assert_eq!(text(&a), ("Sam Lee".into(), "Lunch?\nHello there".into()));
    }

    #[test]
    fn several_messages_are_summed_up() {
        let items: Vec<NewMail> = (0..6).map(|i| mail(&format!("P{i}"), "Hi")).collect();
        let a = Alert { account: "me@example.com".into(), items, popup: true, sound: true };
        let (title, body) = text(&a);
        assert_eq!(title, "6 new messages for me@example.com");
        assert!(body.ends_with("and 2 more"));
    }

    #[test]
    fn the_chime_is_a_valid_wav() {
        let w = chime_wav();
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes([w[4], w[5], w[6], w[7]]) as usize, w.len() - 8);
    }
}
