//! Settings kept between runs (`settings.json` in the config folder).

use std::path::{Path, PathBuf};

use mailwright_graph::AccountConfig;
use serde::{Deserialize, Serialize};

use crate::theme::Appearance;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReadingPane {
    #[default]
    Right,
    Bottom,
    Off,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CalendarView {
    Day,
    #[default]
    WorkWeek,
    Week,
    Month,
}

impl CalendarView {
    pub fn label(self) -> &'static str {
        match self {
            CalendarView::Day => "Day",
            CalendarView::WorkWeek => "Work Week",
            CalendarView::Week => "Week",
            CalendarView::Month => "Month",
        }
    }
}

/// A named signature, as in Outlook.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Signature {
    pub name: String,
    pub body: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// The mailboxes signed in to, in the order they show in the folder pane.
    pub accounts: Vec<AccountConfig>,
    /// From before multiple accounts: read into `accounts`.
    #[serde(skip_serializing)]
    pub account: Option<AccountConfig>,
    /// An organisation's own app registration, used instead of the built-in one.
    pub client_id_override: String,
    /// The client ID and tenant typed on the sign-in screen, kept for next time.
    pub last_client_id: String,
    pub last_tenant: String,
    pub appearance: Appearance,
    pub reading_pane: ReadingPane,
    pub folder_pane: bool,
    pub focused_inbox: bool,
    /// Mark a message read after it's been in the reading pane this many seconds (None: never).
    pub mark_read_after: Option<u32>,
    /// Download pictures in mail automatically.
    pub remote_images: bool,
    /// Senders whose pictures always download.
    pub safe_senders: Vec<String>,
    /// How far back mail is kept offline, in days.
    pub offline_days: i64,
    /// From before named signatures: migrated into `signatures` on load.
    pub signature: String,
    pub signature_on_replies: bool,
    /// Named signatures the person can pick between when composing.
    pub signatures: Vec<Signature>,
    /// The signature added to new messages (a name from `signatures`, or empty for none).
    pub default_signature: String,
    /// The signature added to replies and forwards (empty for none).
    pub reply_signature: String,
    pub compose_font: String,
    pub compose_size: f32,
    pub favorites: Vec<String>,
    pub expanded: Vec<String>,
    pub calendar_view: CalendarView,
    pub work_start: u32,
    pub work_end: u32,
    /// The first day of the week: 0 = Monday, 6 = Sunday.
    pub week_start: u32,
    pub default_reminder: i64,
    pub work_offline: bool,
    pub message_preview: bool,
    pub folder_pane_width: f32,
    pub list_width: f32,
    pub show_bcc: bool,
    /// When new mail arrives: play a sound.
    pub new_mail_sound: bool,
    /// When new mail arrives: show a desktop alert.
    pub new_mail_alert: bool,
    /// When new mail arrives: put an envelope on the taskbar icon until it's seen.
    pub new_mail_envelope: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            accounts: Vec::new(),
            account: None,
            client_id_override: String::new(),
            last_client_id: String::new(),
            last_tenant: "organizations".into(),
            appearance: Appearance::System,
            reading_pane: ReadingPane::Right,
            folder_pane: true,
            focused_inbox: true,
            mark_read_after: Some(1),
            remote_images: false,
            safe_senders: Vec::new(),
            offline_days: 90,
            signature: String::new(),
            signature_on_replies: true,
            signatures: Vec::new(),
            default_signature: String::new(),
            reply_signature: String::new(),
            compose_font: "Calibri".into(),
            compose_size: 11.0,
            favorites: Vec::new(),
            expanded: Vec::new(),
            calendar_view: CalendarView::WorkWeek,
            work_start: 8,
            work_end: 17,
            week_start: 0,
            default_reminder: 15,
            work_offline: false,
            message_preview: true,
            folder_pane_width: 230.0,
            list_width: 400.0,
            show_bcc: false,
            new_mail_sound: true,
            new_mail_alert: true,
            new_mail_envelope: true,
        }
    }
}

/// The folder name under the system's config and data folders.
pub const APP_FOLDER: &str = "Mailwright";

pub fn config_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join(APP_FOLDER)
}

pub fn data_dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join(APP_FOLDER)
}

/// After the rename: move the settings and offline-mail folders across once, so accounts, mail
/// and settings carry on as they were.
pub fn migrate_old_folders() {
    let old_name = concat!("Mail", "Craft");
    for base in [dirs::config_dir(), dirs::data_local_dir()].into_iter().flatten() {
        let (old, new) = (base.join(old_name), base.join(APP_FOLDER));
        if old != new && old.is_dir() && !new.exists() {
            match std::fs::rename(&old, &new) {
                Ok(()) => log::info!("moved {} to {}", old.display(), new.display()),
                Err(e) => log::warn!("couldn't move {} to {}: {e}", old.display(), new.display()),
            }
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Settings {
        let mut s: Settings = std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        if let Some(a) = s.account.take() {
            if !a.username.is_empty() && !s.accounts.iter().any(|x| x.username.eq_ignore_ascii_case(&a.username)) {
                s.accounts.push(a);
            }
        }
        if s.client_id_override.is_empty() && mailwright_graph::config::is_guid(s.last_client_id.trim()) && mailwright_graph::config::BUILTIN_CLIENT_ID.is_none() {
            s.client_id_override = s.last_client_id.trim().to_string();
        }
        // The old single signature becomes a named one.
        if s.signatures.is_empty() && !s.signature.trim().is_empty() {
            s.signatures.push(Signature { name: "My signature".into(), body: s.signature.clone() });
            s.default_signature = "My signature".into();
            if s.signature_on_replies {
                s.reply_signature = "My signature".into();
            }
        }
        s
    }

    /// The body of the signature named `name`, if it exists and isn't empty.
    pub fn signature_body(&self, name: &str) -> Option<&str> {
        self.signatures.iter().find(|x| x.name == name).map(|x| x.body.trim_end()).filter(|b| !b.is_empty())
    }

    pub fn save(&self, path: &Path) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match serde_json::to_string_pretty(self) {
            Ok(s) => {
                let tmp = path.with_extension("json.tmp");
                if std::fs::write(&tmp, s).is_ok() {
                    let _ = std::fs::rename(&tmp, path);
                }
            }
            Err(e) => log::warn!("settings: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_or_missing_fields_fall_back_to_defaults() {
        let s: Settings = serde_json::from_str(r#"{"readingPane":"bottom","nonsense":1}"#).unwrap();
        assert_eq!(s.reading_pane, ReadingPane::Bottom);
        assert_eq!(s.offline_days, 90);
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }
}
