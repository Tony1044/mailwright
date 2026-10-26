//! Microsoft Graph v1.0 resources, as much of each as Mailwright uses. Every field is optional on
//! the way in (`#[serde(default)]`): Graph omits what wasn't selected, and a missing field must
//! never fail a whole sync.

use serde::{Deserialize, Serialize};

/// A page of a collection: `value`, plus `@odata.nextLink` while there is more and, at the end
/// of a delta round, `@odata.deltaLink` for the next one.
#[derive(Debug, Deserialize)]
#[serde(default, bound(deserialize = "T: Deserialize<'de>"))]
pub struct Page<T> {
    pub value: Vec<T>,
    #[serde(rename = "@odata.nextLink")]
    pub next_link: Option<String>,
    #[serde(rename = "@odata.deltaLink")]
    pub delta_link: Option<String>,
}

impl<T> Default for Page<T> {
    fn default() -> Self {
        Page { value: Vec::new(), next_link: None, delta_link: None }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub display_name: Option<String>,
    pub mail: Option<String>,
    pub user_principal_name: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct MailboxSettings {
    pub time_zone: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct MailFolder {
    pub id: String,
    pub display_name: String,
    pub parent_folder_id: Option<String>,
    pub child_folder_count: i64,
    pub unread_item_count: i64,
    pub total_item_count: i64,
    pub is_hidden: Option<bool>,
    /// Set by delta when the folder was deleted.
    #[serde(rename = "@removed", skip_serializing_if = "Option::is_none")]
    pub removed: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct EmailAddress {
    pub name: Option<String>,
    pub address: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct Recipient {
    pub email_address: EmailAddress,
}

impl Recipient {
    pub fn new(name: &str, address: &str) -> Recipient {
        Recipient {
            email_address: EmailAddress {
                name: if name.is_empty() { None } else { Some(name.to_string()) },
                address: Some(address.to_string()),
            },
        }
    }
    /// The name to show: the display name, or the address.
    pub fn label(&self) -> String {
        match (&self.email_address.name, &self.email_address.address) {
            (Some(n), _) if !n.trim().is_empty() => n.clone(),
            (_, Some(a)) => a.clone(),
            _ => String::new(),
        }
    }
    pub fn address(&self) -> String {
        self.email_address.address.clone().unwrap_or_default()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct ItemBody {
    /// `html` or `text`.
    pub content_type: String,
    pub content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct DateTimeTimeZone {
    /// `2026-10-23T09:00:00.0000000` in `time_zone`.
    pub date_time: String,
    pub time_zone: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct FollowupFlag {
    /// `notFlagged`, `flagged` or `complete`.
    pub flag_status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    #[serde(rename = "@odata.type", skip_serializing_if = "Option::is_none")]
    pub odata_type: Option<String>,
    pub subject: Option<String>,
    pub body_preview: Option<String>,
    pub body: Option<ItemBody>,
    pub from: Option<Recipient>,
    pub sender: Option<Recipient>,
    pub to_recipients: Vec<Recipient>,
    pub cc_recipients: Vec<Recipient>,
    pub bcc_recipients: Vec<Recipient>,
    pub reply_to: Vec<Recipient>,
    pub received_date_time: Option<String>,
    pub sent_date_time: Option<String>,
    pub is_read: Option<bool>,
    pub is_draft: Option<bool>,
    pub has_attachments: Option<bool>,
    /// `low`, `normal`, `high`.
    pub importance: Option<String>,
    pub flag: Option<FollowupFlag>,
    pub conversation_id: Option<String>,
    pub parent_folder_id: Option<String>,
    /// `focused` or `other` (Focused Inbox).
    pub inference_classification: Option<String>,
    pub categories: Vec<String>,
    pub web_link: Option<String>,
    /// For meeting messages (`#microsoft.graph.eventMessage…`).
    pub meeting_message_type: Option<String>,
    pub event: Option<Event>,
    #[serde(rename = "@removed", skip_serializing_if = "Option::is_none")]
    pub removed: Option<serde_json::Value>,
}

impl Message {
    pub fn is_meeting_message(&self) -> bool {
        self.odata_type.as_deref().is_some_and(|t| t.contains("eventMessage"))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct Attachment {
    pub id: String,
    #[serde(rename = "@odata.type", skip_serializing_if = "Option::is_none")]
    pub odata_type: Option<String>,
    pub name: Option<String>,
    pub content_type: Option<String>,
    pub size: i64,
    pub is_inline: bool,
    pub content_id: Option<String>,
    /// Base64, only when fetched.
    pub content_bytes: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct Calendar {
    pub id: String,
    pub name: String,
    pub hex_color: Option<String>,
    pub can_edit: Option<bool>,
    pub is_default_calendar: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct Location {
    pub display_name: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct ResponseStatus {
    /// `none`, `organizer`, `tentativelyAccepted`, `accepted`, `declined`, `notResponded`.
    pub response: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct Attendee {
    pub email_address: EmailAddress,
    /// `required`, `optional`, `resource`.
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub status: Option<ResponseStatus>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct OnlineMeetingInfo {
    pub join_url: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Event {
    pub id: String,
    pub subject: Option<String>,
    pub body_preview: Option<String>,
    pub body: Option<ItemBody>,
    pub start: Option<DateTimeTimeZone>,
    pub end: Option<DateTimeTimeZone>,
    pub is_all_day: Option<bool>,
    pub location: Option<Location>,
    pub organizer: Option<Recipient>,
    pub attendees: Vec<Attendee>,
    /// `free`, `tentative`, `busy`, `oof`, `workingElsewhere`, `unknown`.
    pub show_as: Option<String>,
    pub response_status: Option<ResponseStatus>,
    pub is_organizer: Option<bool>,
    pub is_cancelled: Option<bool>,
    pub is_online_meeting: Option<bool>,
    pub online_meeting: Option<OnlineMeetingInfo>,
    pub series_master_id: Option<String>,
    /// `singleInstance`, `occurrence`, `exception`, `seriesMaster`.
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub categories: Vec<String>,
    pub reminder_minutes_before_start: Option<i64>,
    pub is_reminder_on: Option<bool>,
    pub web_link: Option<String>,
    /// Filled in by Mailwright from the calendar the event was fetched from.
    #[serde(skip)]
    pub calendar_id: String,
    #[serde(rename = "@removed", skip_serializing_if = "Option::is_none")]
    pub removed: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Person {
    pub display_name: Option<String>,
    pub scored_email_addresses: Vec<ScoredEmailAddress>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct ScoredEmailAddress {
    pub address: Option<String>,
}

/// Graph's error body: `{"error": {"code": "...", "message": "..."}}`.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct ErrorBody {
    pub error: ErrorDetail,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct ErrorDetail {
    pub code: String,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_delta_page_with_removed_and_meeting_items() {
        let json = r##"{
            "@odata.context": "x",
            "@odata.nextLink": "https://graph.microsoft.com/v1.0/me/mailFolders/inbox/messages/delta?$skiptoken=abc",
            "value": [
                {"id": "AAA", "subject": "Hello", "isRead": false, "from": {"emailAddress": {"name": "Ada", "address": "ada@contoso.com"}},
                 "toRecipients": [{"emailAddress": {"address": "tony@contoso.com"}}], "flag": {"flagStatus": "flagged"},
                 "receivedDateTime": "2026-10-23T08:15:00Z", "hasAttachments": true},
                {"@odata.type": "#microsoft.graph.eventMessageRequest", "id": "BBB", "subject": "Design review", "meetingMessageType": "meetingRequest"},
                {"id": "CCC", "@removed": {"reason": "deleted"}}
            ]
        }"##;
        let page: Page<Message> = serde_json::from_str(json).unwrap();
        assert_eq!(page.value.len(), 3);
        assert_eq!(page.value[0].from.as_ref().unwrap().label(), "Ada");
        assert_eq!(page.value[0].to_recipients[0].label(), "tony@contoso.com");
        assert!(page.value[1].is_meeting_message());
        assert!(page.value[2].removed.is_some());
        assert!(page.next_link.is_some() && page.delta_link.is_none());
    }
}
