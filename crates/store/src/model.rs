//! What the cache hands the interface, and the changes it queues for the server.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Addr {
    pub name: String,
    pub address: String,
}

impl Addr {
    pub fn new(name: &str, address: &str) -> Addr {
        Addr { name: name.to_string(), address: address.to_string() }
    }
    /// The display name, or the address.
    pub fn label(&self) -> &str {
        if self.name.trim().is_empty() { &self.address } else { &self.name }
    }
    /// `Name <address>`, or just the address.
    pub fn full(&self) -> String {
        if self.name.trim().is_empty() || self.name == self.address { self.address.clone() } else { format!("{} <{}>", self.name, self.address) }
    }
    pub fn from_graph(r: &mailwright_graph::types::Recipient) -> Addr {
        Addr { name: r.email_address.name.clone().unwrap_or_default(), address: r.email_address.address.clone().unwrap_or_default() }
    }
    pub fn to_graph(&self) -> serde_json::Value {
        if self.name.trim().is_empty() {
            serde_json::json!({"emailAddress": {"address": self.address}})
        } else {
            serde_json::json!({"emailAddress": {"name": self.name, "address": self.address}})
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FolderRow {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    /// `inbox`, `drafts`, `sentitems`, `deleteditems`, `junkemail`, `archive`, `outbox`, …
    pub well_known: Option<String>,
    pub unread: i64,
    pub total: i64,
    /// Has been synced at least once (so its list is in the cache).
    pub synced: bool,
    pub hidden: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MessageRow {
    pub rowid: i64,
    pub id: String,
    pub folder_id: String,
    pub subject: String,
    pub preview: String,
    pub from: Addr,
    pub to: Vec<Addr>,
    pub cc: Vec<Addr>,
    pub bcc: Vec<Addr>,
    /// Unix seconds.
    pub received: i64,
    pub sent: i64,
    pub is_read: bool,
    pub is_draft: bool,
    pub has_attachments: bool,
    /// `low`, `normal`, `high`.
    pub importance: String,
    /// `notFlagged`, `flagged`, `complete`.
    pub flag: String,
    pub conversation_id: String,
    /// Focused Inbox: false for "Other".
    pub focused: bool,
    pub categories: Vec<String>,
    /// For meeting messages: `meetingRequest`, `meetingCancelled`, `meetingAccepted`, …
    pub meeting_type: Option<String>,
    pub is_meeting: bool,
    /// The event a meeting message is about (once fetched).
    pub event_id: Option<String>,
    pub web_link: String,
    /// HTML, once fetched.
    pub body: Option<String>,
    /// Waiting for a change to reach the server.
    pub pending: bool,
}

impl MessageRow {
    pub fn flagged(&self) -> bool {
        self.flag == "flagged"
    }
    pub fn completed(&self) -> bool {
        self.flag == "complete"
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AttachmentRow {
    pub message_id: String,
    pub id: String,
    pub name: String,
    pub content_type: String,
    pub size: i64,
    pub inline: bool,
    pub content_id: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CalendarRow {
    pub id: String,
    pub name: String,
    /// `#rrggbb`, when the calendar has one.
    pub color: String,
    pub can_edit: bool,
    pub is_default: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttendeeRow {
    pub addr: Addr,
    /// `required`, `optional`, `resource`.
    pub kind: String,
    /// `none`, `accepted`, `tentativelyAccepted`, `declined`, `notResponded`, `organizer`.
    pub response: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EventRow {
    pub id: String,
    pub calendar_id: String,
    pub subject: String,
    /// Unix seconds, UTC. For all-day events: midnight UTC of the first day.
    pub start: i64,
    /// Exclusive. For all-day events: midnight UTC after the last day.
    pub end: i64,
    pub all_day: bool,
    pub location: String,
    pub organizer: Addr,
    pub attendees: Vec<AttendeeRow>,
    /// `free`, `tentative`, `busy`, `oof`, `workingElsewhere`.
    pub show_as: String,
    /// My response: `organizer`, `accepted`, `tentativelyAccepted`, `declined`, `notResponded`, `none`.
    pub response: String,
    pub is_organizer: bool,
    pub is_cancelled: bool,
    pub join_url: String,
    /// `singleInstance`, `occurrence`, `exception`, `seriesMaster`.
    pub kind: String,
    pub series_master_id: String,
    pub categories: Vec<String>,
    pub reminder_minutes: Option<i64>,
    pub preview: String,
    pub body: Option<String>,
    pub web_link: String,
    /// Made here and not yet on the server.
    pub local: bool,
}

/// A file attached to an outgoing message.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutAttachment {
    pub name: String,
    pub content_type: String,
    /// Base64.
    pub data: String,
    #[serde(default)]
    pub content_id: Option<String>,
}

/// A message being written.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Compose {
    /// The local id of its Outbox entry.
    #[serde(default)]
    pub local_id: String,
    /// A draft on the server this replaces (opened from Drafts).
    #[serde(default)]
    pub draft_id: Option<String>,
    /// Replying to or forwarding: the original's id and `reply`, `replyAll` or `forward`.
    #[serde(default)]
    pub reply_to: Option<(String, String)>,
    pub subject: String,
    pub to: Vec<Addr>,
    pub cc: Vec<Addr>,
    pub bcc: Vec<Addr>,
    /// The whole body, HTML (with the quoted original for replies).
    pub html: String,
    /// `low`, `normal`, `high`.
    pub importance: String,
    pub attachments: Vec<OutAttachment>,
    #[serde(default)]
    pub request_read_receipt: bool,
}

/// A change made here, applied to the cache at once and to the server when it can be.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Action {
    SetRead { id: String, read: bool },
    SetFlag { id: String, status: String },
    SetCategories { id: String, categories: Vec<String> },
    Move { id: String, dest: String },
    /// Gone for good (deleting from Deleted Items). Ordinary deleting is a move.
    Delete { id: String },
    Send { compose: Compose },
    SaveDraft { compose: Compose },
    CreateEvent { local_id: String, calendar_id: String, body: serde_json::Value },
    UpdateEvent { id: String, patch: serde_json::Value },
    DeleteEvent { id: String },
    CancelEvent { id: String, comment: String },
    /// `accept`, `tentative` or `decline`.
    RespondEvent { id: String, response: String, comment: String, send: bool },
}

impl Action {
    /// The message this changes, if any.
    pub fn message_id(&self) -> Option<&str> {
        match self {
            Action::SetRead { id, .. } | Action::SetFlag { id, .. } | Action::SetCategories { id, .. } | Action::Move { id, .. } | Action::Delete { id } => Some(id),
            _ => None,
        }
    }

    /// A few words for the status bar.
    pub fn describe(&self) -> String {
        match self {
            Action::SetRead { read: true, .. } => "Mark as read".into(),
            Action::SetRead { read: false, .. } => "Mark as unread".into(),
            Action::SetFlag { .. } => "Flag".into(),
            Action::SetCategories { .. } => "Categorise".into(),
            Action::Move { .. } => "Move".into(),
            Action::Delete { .. } => "Delete".into(),
            Action::Send { compose } => format!("Send \u{201c}{}\u{201d}", compose.subject),
            Action::SaveDraft { compose } => format!("Save draft \u{201c}{}\u{201d}", compose.subject),
            Action::CreateEvent { .. } => "New appointment".into(),
            Action::UpdateEvent { .. } => "Change appointment".into(),
            Action::DeleteEvent { .. } => "Delete appointment".into(),
            Action::CancelEvent { .. } => "Cancel meeting".into(),
            Action::RespondEvent { response, .. } => format!("Meeting response ({response})"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct QueuedAction {
    pub seq: i64,
    pub action: Action,
    pub attempts: i64,
    pub last_error: String,
}

/// How the message list is filtered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ListFilter {
    #[default]
    All,
    Unread,
    Flagged,
    Attachments,
    Focused,
    Other,
    ToMe,
}

/// How the message list is sorted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortBy {
    #[default]
    Date,
    From,
    Subject,
    Importance,
    Flag,
}
