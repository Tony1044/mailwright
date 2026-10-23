//! Settings kept between runs (`settings.json` in the config folder).

use std::path::{Path, PathBuf};

use mailcraft_graph::AccountConfig;
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// The mailbox signed in to (its client ID, tenant and address).
    pub account: Option<AccountConfig>,
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
    pub signature: String,
    pub signature_on_replies: bool,
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
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            account: None,
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
        }
    }
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("MailCraft")
}

pub fn data_dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("MailCraft")
}

impl Settings {
    pub fn load(path: &Path) -> Settings {
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
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
