//! The Graph calls MailCraft makes. Requests carry a fresh access token (refreshed when it is
//! about to expire, and once more on a 401), wait out throttling (429/503 with Retry-After), and
//! turn Graph's error bodies into [`GraphError::Api`].

use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::auth::{self, Tokens};
use crate::config::AccountConfig;
use crate::error::GraphError;
use crate::http;
use crate::tokens::TokenStore;
use crate::types::*;

/// Properties fetched for the message list (no bodies: those come when a message is opened).
pub const MESSAGE_LIST_SELECT: &str = "id,subject,bodyPreview,from,sender,toRecipients,ccRecipients,receivedDateTime,sentDateTime,isRead,isDraft,hasAttachments,importance,flag,conversationId,parentFolderId,inferenceClassification,categories,webLink";
/// Properties fetched when a message is opened.
pub const MESSAGE_FULL_SELECT: &str = "id,subject,body,bodyPreview,from,sender,toRecipients,ccRecipients,bccRecipients,replyTo,receivedDateTime,sentDateTime,isRead,isDraft,hasAttachments,importance,flag,conversationId,parentFolderId,categories,webLink";
pub const EVENT_SELECT: &str = "id,subject,bodyPreview,start,end,isAllDay,location,organizer,attendees,showAs,responseStatus,isOrganizer,isCancelled,isOnlineMeeting,onlineMeeting,seriesMasterId,type,categories,reminderMinutesBeforeStart,isReminderOn,webLink";
/// Attachments up to this size go in one request; larger ones use an upload session.
pub const SMALL_ATTACHMENT: usize = 3 * 1024 * 1024;
/// Upload session chunk: a multiple of 320 KiB, under Graph's 4 MiB limit.
const UPLOAD_CHUNK: usize = 320 * 1024 * 10;

/// One page of a delta round.
#[derive(Debug, Default)]
pub struct DeltaPage<T> {
    pub changed: Vec<T>,
    pub removed: Vec<String>,
    /// Follow this for the next page of the same round.
    pub next_link: Option<String>,
    /// The round is complete: use this next time to get only what changed.
    pub delta_link: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplyKind {
    Reply,
    ReplyAll,
    Forward,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventResponse {
    Accept,
    Tentative,
    Decline,
}

pub struct GraphClient {
    cfg: AccountConfig,
    base: String,
    tokens: Mutex<Tokens>,
    store: Arc<dyn TokenStore>,
}

impl GraphClient {
    pub fn new(cfg: AccountConfig, tokens: Tokens, store: Arc<dyn TokenStore>) -> GraphClient {
        let base = cfg.graph_base().trim_end_matches('/').to_string();
        GraphClient { cfg, base, tokens: Mutex::new(tokens), store }
    }

    /// Resume the saved sign-in for `cfg.username` (refreshing now to check it still works).
    pub fn from_saved(cfg: AccountConfig, store: Arc<dyn TokenStore>) -> Result<GraphClient, GraphError> {
        let rt = store.load(&cfg.username)?.ok_or_else(|| GraphError::SignInRequired("no saved sign-in".into()))?;
        let t = auth::refresh(&cfg, &rt)?;
        store.save(&cfg.username, &t.refresh_token)?;
        Ok(GraphClient::new(cfg, t, store))
    }

    pub fn config(&self) -> &AccountConfig {
        &self.cfg
    }

    /// Remember the signed-in user (after `me()`), saving the refresh token under their address.
    pub fn set_user(&mut self, username: &str, display_name: &str) -> Result<(), GraphError> {
        self.cfg.username = username.to_string();
        self.cfg.display_name = display_name.to_string();
        let rt = self.tokens.lock().map_err(|_| GraphError::Credentials("poisoned".into()))?.refresh_token.clone();
        if !rt.is_empty() {
            self.store.save(username, &rt)?;
        }
        Ok(())
    }

    pub fn sign_out(&self) -> Result<(), GraphError> {
        self.store.clear(&self.cfg.username)
    }

    fn access_token(&self, force_refresh: bool) -> Result<String, GraphError> {
        let mut t = self.tokens.lock().map_err(|_| GraphError::Credentials("poisoned".into()))?;
        if !force_refresh && t.is_fresh() {
            return Ok(t.access_token.clone());
        }
        let fresh = auth::refresh(&self.cfg, &t.refresh_token)?;
        if !self.cfg.username.is_empty() && fresh.refresh_token != t.refresh_token {
            self.store.save(&self.cfg.username, &fresh.refresh_token)?;
        }
        *t = fresh;
        Ok(t.access_token.clone())
    }

    fn url(&self, path: &str) -> String {
        if path.starts_with("http://") || path.starts_with("https://") { path.to_string() } else { format!("{}{}", self.base, path) }
    }

    /// Send a request; `body` is JSON when given. Returns the status and the raw response.
    fn send(&self, method: &str, path: &str, body: Option<&Value>, prefer: &[&str]) -> Result<(u16, ureq::http::Response<ureq::Body>), GraphError> {
        let url = self.url(path);
        let mut refreshed = false;
        let mut attempts = 0;
        loop {
            attempts += 1;
            let token = self.access_token(false)?;
            let agent = http::agent();
            let auth_header = format!("Bearer {token}");
            let prefer_header = prefer.join(", ");
            let result = match (method, body) {
                ("GET", _) => {
                    let mut r = agent.get(&url).header("Authorization", &auth_header);
                    if !prefer.is_empty() {
                        r = r.header("Prefer", &prefer_header);
                    }
                    r.call()
                }
                ("DELETE", _) => agent.delete(&url).header("Authorization", &auth_header).call(),
                (m, b) => {
                    let mut r = match m {
                        "PATCH" => agent.patch(&url),
                        "PUT" => agent.put(&url),
                        _ => agent.post(&url),
                    }
                    .header("Authorization", &auth_header);
                    if !prefer.is_empty() {
                        r = r.header("Prefer", &prefer_header);
                    }
                    match b {
                        Some(v) => r.send_json(v),
                        None => r.header("Content-Length", "0").send_empty(),
                    }
                }
            };
            let resp = result.map_err(|e| http::network_error(&url, e))?;
            let status = resp.status().as_u16();
            if status == 401 && !refreshed {
                refreshed = true;
                self.access_token(true)?;
                continue;
            }
            if matches!(status, 429 | 503 | 504) && attempts < 4 {
                let wait = http::retry_after(&resp).unwrap_or(2u64.pow(attempts));
                std::thread::sleep(Duration::from_secs(wait));
                continue;
            }
            return Ok((status, resp));
        }
    }

    fn error_from(status: u16, text: &str) -> GraphError {
        let eb: ErrorBody = serde_json::from_str(text).unwrap_or_default();
        if status == 401 || eb.error.code == "InvalidAuthenticationToken" {
            return GraphError::SignInRequired(eb.error.message);
        }
        GraphError::Api { status, code: eb.error.code, message: eb.error.message }
    }

    /// A JSON request whose response body is parsed as `T`.
    fn json<T: DeserializeOwned>(&self, method: &str, path: &str, body: Option<&Value>, prefer: &[&str]) -> Result<T, GraphError> {
        let url = self.url(path);
        let (status, mut resp) = self.send(method, path, body, prefer)?;
        let text = http::read_text(&mut resp, &url)?;
        if status >= 400 {
            return Err(Self::error_from(status, &text));
        }
        serde_json::from_str(&text).map_err(|e| GraphError::BadResponse(format!("{path}: {e}")))
    }

    /// A request with no useful response body.
    fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<(), GraphError> {
        let url = self.url(path);
        let (status, mut resp) = self.send(method, path, body, &[])?;
        if status >= 400 {
            let text = http::read_text(&mut resp, &url).unwrap_or_default();
            return Err(Self::error_from(status, &text));
        }
        Ok(())
    }

    /// Every page of a (non-delta) collection, up to `max` items.
    fn all_pages<T: DeserializeOwned>(&self, first: &str, prefer: &[&str], max: usize) -> Result<Vec<T>, GraphError> {
        let mut out = Vec::new();
        let mut next = Some(first.to_string());
        while let Some(link) = next {
            let page: Page<T> = self.json("GET", &link, None, prefer)?;
            out.extend(page.value);
            if out.len() >= max {
                out.truncate(max);
                break;
            }
            next = page.next_link;
        }
        Ok(out)
    }

    // ---- account ---------------------------------------------------------------------------

    pub fn me(&self) -> Result<User, GraphError> {
        self.json("GET", "/me?$select=id,displayName,mail,userPrincipalName", None, &[])
    }

    pub fn mailbox_settings(&self) -> Result<MailboxSettings, GraphError> {
        self.json("GET", "/me/mailboxSettings?$select=timeZone", None, &[])
    }

    // ---- folders ---------------------------------------------------------------------------

    /// A well-known folder (`inbox`, `drafts`, `sentitems`, `deleteditems`, `junkemail`,
    /// `archive`, `outbox`), for its id.
    pub fn well_known_folder(&self, name: &str) -> Result<MailFolder, GraphError> {
        self.json("GET", &format!("/me/mailFolders/{name}?$select=id,displayName,parentFolderId,childFolderCount,unreadItemCount,totalItemCount"), None, &[])
    }

    /// The whole folder tree, depth-first, at most 2,000 folders and 10 levels deep.
    pub fn all_folders(&self) -> Result<Vec<MailFolder>, GraphError> {
        const SELECT: &str = "$select=id,displayName,parentFolderId,childFolderCount,unreadItemCount,totalItemCount,isHidden";
        let mut out: Vec<MailFolder> = Vec::new();
        let top = self.all_pages::<MailFolder>(&format!("/me/mailFolders?$top=250&{SELECT}"), &[], 2000)?;
        let mut stack: Vec<(MailFolder, usize)> = top.into_iter().rev().map(|f| (f, 0)).collect();
        while let Some((f, depth)) = stack.pop() {
            let has_children = f.child_folder_count > 0 && depth < 10 && out.len() < 2000;
            let id = f.id.clone();
            out.push(f);
            if has_children {
                let kids = self.all_pages::<MailFolder>(&format!("/me/mailFolders/{id}/childFolders?$top=250&{SELECT}"), &[], 2000)?;
                stack.extend(kids.into_iter().rev().map(|k| (k, depth + 1)));
            }
        }
        Ok(out)
    }

    pub fn create_folder(&self, parent_id: Option<&str>, name: &str) -> Result<MailFolder, GraphError> {
        let path = match parent_id {
            Some(p) => format!("/me/mailFolders/{p}/childFolders"),
            None => "/me/mailFolders".to_string(),
        };
        self.json("POST", &path, Some(&json!({"displayName": name})), &[])
    }

    pub fn rename_folder(&self, id: &str, name: &str) -> Result<(), GraphError> {
        self.call("PATCH", &format!("/me/mailFolders/{id}"), Some(&json!({"displayName": name})))
    }

    /// Move a folder under another (Deleted Items, when deleting).
    pub fn move_folder(&self, id: &str, destination: &str) -> Result<(), GraphError> {
        self.json::<serde_json::Value>("POST", &format!("/me/mailFolders/{id}/move"), Some(&json!({"destinationId": destination})), &[]).map(|_| ())
    }

    pub fn delete_folder(&self, id: &str) -> Result<(), GraphError> {
        self.call("DELETE", &format!("/me/mailFolders/{id}"), None)
    }

    // ---- messages --------------------------------------------------------------------------

    /// One page of a folder's message delta. Start a round with `link = None` (and `since`, the
    /// oldest received date to fetch on the first sync); continue with the page's `next_link`;
    /// later rounds start from the saved `delta_link`.
    pub fn messages_delta(&self, folder_id: &str, link: Option<&str>, since: Option<&str>) -> Result<DeltaPage<Message>, GraphError> {
        let path = match link {
            Some(l) => l.to_string(),
            None => {
                let mut p = format!("/me/mailFolders/{folder_id}/messages/delta?$select={MESSAGE_LIST_SELECT}");
                if let Some(s) = since {
                    p.push_str(&format!("&$filter=receivedDateTime+ge+{s}&$orderby=receivedDateTime+desc"));
                }
                p
            }
        };
        let page: Page<Message> = self.json("GET", &path, None, &["odata.maxpagesize=100"])?;
        let mut out = DeltaPage { next_link: page.next_link, delta_link: page.delta_link, ..Default::default() };
        for m in page.value {
            if m.removed.is_some() {
                out.removed.push(m.id);
            } else {
                out.changed.push(m);
            }
        }
        Ok(out)
    }

    /// A message with its body (HTML) and all recipients.
    pub fn message_full(&self, id: &str) -> Result<Message, GraphError> {
        self.json("GET", &format!("/me/messages/{id}?$select={MESSAGE_FULL_SELECT}"), None, &["outlook.body-content-type=\"html\""])
    }

    /// For a meeting message (its list entry's `@odata.type` names an eventMessage): the meeting
    /// message type and the calendar event it is about.
    pub fn meeting_event(&self, id: &str) -> Result<(Option<String>, Option<Event>), GraphError> {
        let em: Message = self.json(
            "GET",
            &format!("/me/messages/{id}?$expand=microsoft.graph.eventMessage/event($select={EVENT_SELECT})"),
            None,
            &["outlook.timezone=\"UTC\""],
        )?;
        Ok((em.meeting_message_type, em.event))
    }

    /// Server-side search across the whole mailbox (beyond the local cache).
    pub fn search_messages(&self, query: &str, top: usize) -> Result<Vec<Message>, GraphError> {
        let q = query.replace('"', "");
        let mut u = url::Url::parse(&self.url("/me/messages")).map_err(|e| GraphError::Config(e.to_string()))?;
        u.query_pairs_mut()
            .append_pair("$search", &format!("\"{q}\""))
            .append_pair("$select", MESSAGE_LIST_SELECT)
            .append_pair("$top", &top.clamp(1, 250).to_string());
        self.all_pages(u.as_str(), &[], top)
    }

    pub fn set_read(&self, id: &str, read: bool) -> Result<(), GraphError> {
        self.call("PATCH", &format!("/me/messages/{id}"), Some(&json!({"isRead": read})))
    }

    /// `notFlagged`, `flagged` or `complete`.
    pub fn set_flag(&self, id: &str, status: &str) -> Result<(), GraphError> {
        self.call("PATCH", &format!("/me/messages/{id}"), Some(&json!({"flag": {"flagStatus": status}})))
    }

    pub fn set_categories(&self, id: &str, categories: &[String]) -> Result<(), GraphError> {
        self.call("PATCH", &format!("/me/messages/{id}"), Some(&json!({"categories": categories})))
    }

    /// Move to another folder. The message gets a new id there.
    pub fn move_message(&self, id: &str, destination: &str) -> Result<Message, GraphError> {
        self.json("POST", &format!("/me/messages/{id}/move"), Some(&json!({"destinationId": destination})), &[])
    }

    /// Delete for good (Deleted Items → gone). Ordinary deleting is a move to `deleteditems`.
    pub fn delete_message(&self, id: &str) -> Result<(), GraphError> {
        self.call("DELETE", &format!("/me/messages/{id}"), None)
    }

    // ---- sending ---------------------------------------------------------------------------

    /// A new draft in Drafts.
    pub fn create_draft(&self, draft: &Value) -> Result<Message, GraphError> {
        self.json("POST", "/me/messages", Some(draft), &[])
    }

    pub fn update_draft(&self, id: &str, patch: &Value) -> Result<Message, GraphError> {
        self.json("PATCH", &format!("/me/messages/{id}"), Some(patch), &["outlook.body-content-type=\"html\""])
    }

    /// A reply, reply-all or forward draft, with the original quoted in its body.
    pub fn create_reply(&self, id: &str, kind: ReplyKind) -> Result<Message, GraphError> {
        let action = match kind {
            ReplyKind::Reply => "createReply",
            ReplyKind::ReplyAll => "createReplyAll",
            ReplyKind::Forward => "createForward",
        };
        let draft: Message = self.json("POST", &format!("/me/messages/{id}/{action}"), Some(&json!({})), &[])?;
        // The body comes back as HTML only when asked for.
        self.json("GET", &format!("/me/messages/{}?$select={MESSAGE_FULL_SELECT}", draft.id), None, &["outlook.body-content-type=\"html\""])
    }

    /// Attach a file to a draft (an upload session for anything over 3 MB).
    pub fn add_attachment(&self, draft_id: &str, name: &str, content_type: &str, bytes: &[u8], inline_content_id: Option<&str>) -> Result<(), GraphError> {
        if bytes.len() <= SMALL_ATTACHMENT {
            let mut body = json!({
                "@odata.type": "#microsoft.graph.fileAttachment",
                "name": name,
                "contentType": content_type,
                "contentBytes": base64::engine::general_purpose::STANDARD.encode(bytes),
            });
            if let Some(cid) = inline_content_id {
                body["isInline"] = json!(true);
                body["contentId"] = json!(cid);
            }
            return self.call("POST", &format!("/me/messages/{draft_id}/attachments"), Some(&body));
        }
        #[derive(serde::Deserialize, Default)]
        #[serde(default, rename_all = "camelCase")]
        struct Session {
            upload_url: String,
        }
        let mut item = json!({"attachmentType": "file", "name": name, "size": bytes.len()});
        if let Some(cid) = inline_content_id {
            item["isInline"] = json!(true);
            item["contentId"] = json!(cid);
        }
        let s: Session = self.json("POST", &format!("/me/messages/{draft_id}/attachments/createUploadSession"), Some(&json!({"AttachmentItem": item})), &[])?;
        if s.upload_url.is_empty() {
            return Err(GraphError::BadResponse("no upload URL".into()));
        }
        let total = bytes.len();
        let mut start = 0;
        while start < total {
            let end = (start + UPLOAD_CHUNK).min(total);
            // The upload URL carries its own authorisation: no bearer token here.
            let mut resp = http::agent()
                .put(&s.upload_url)
                .header("Content-Range", &format!("bytes {start}-{}/{total}", end - 1))
                .send(&bytes[start..end])
                .map_err(|e| http::network_error(&s.upload_url, e))?;
            let status = resp.status().as_u16();
            if status >= 400 {
                let text = http::read_text(&mut resp, &s.upload_url).unwrap_or_default();
                return Err(Self::error_from(status, &text));
            }
            start = end;
        }
        Ok(())
    }

    pub fn send_draft(&self, id: &str) -> Result<(), GraphError> {
        self.call("POST", &format!("/me/messages/{id}/send"), None)
    }

    // ---- attachments -----------------------------------------------------------------------

    /// The attachments of a message, without their contents.
    pub fn attachments(&self, message_id: &str) -> Result<Vec<Attachment>, GraphError> {
        self.all_pages(&format!("/me/messages/{message_id}/attachments?$select=id,name,contentType,size,isInline"), &[], 500)
    }

    /// One attachment with its content (base64) and content id.
    pub fn attachment(&self, message_id: &str, attachment_id: &str) -> Result<Attachment, GraphError> {
        self.json("GET", &format!("/me/messages/{message_id}/attachments/{attachment_id}"), None, &[])
    }

    /// An attachment's raw bytes.
    pub fn attachment_bytes(&self, message_id: &str, attachment_id: &str) -> Result<Vec<u8>, GraphError> {
        let path = format!("/me/messages/{message_id}/attachments/{attachment_id}/$value");
        let url = self.url(&path);
        let (status, mut resp) = self.send("GET", &path, None, &[])?;
        if status >= 400 {
            let text = http::read_text(&mut resp, &url).unwrap_or_default();
            return Err(Self::error_from(status, &text));
        }
        http::read_bytes(&mut resp, &url, http::MAX_DOWNLOAD)
    }

    // ---- calendar --------------------------------------------------------------------------

    pub fn calendars(&self) -> Result<Vec<Calendar>, GraphError> {
        self.all_pages("/me/calendars?$select=id,name,hexColor,canEdit,isDefaultCalendar&$top=100", &[], 500)
    }

    /// Events (recurring ones expanded into occurrences) between two UTC instants
    /// (`2026-10-01T00:00:00Z`), with times in UTC.
    pub fn calendar_view(&self, calendar_id: &str, start: &str, end: &str) -> Result<Vec<Event>, GraphError> {
        let path = format!("/me/calendars/{calendar_id}/calendarView?startDateTime={start}&endDateTime={end}&$select={EVENT_SELECT}&$top=250&$orderby=start/dateTime");
        let mut events: Vec<Event> = self.all_pages(&path, &["outlook.timezone=\"UTC\""], 5000)?;
        for e in &mut events {
            e.calendar_id = calendar_id.to_string();
        }
        Ok(events)
    }

    pub fn event_full(&self, id: &str) -> Result<Event, GraphError> {
        self.json("GET", &format!("/me/events/{id}?$select={EVENT_SELECT},body"), None, &["outlook.timezone=\"UTC\"", "outlook.body-content-type=\"html\""])
    }

    pub fn create_event(&self, calendar_id: &str, event: &Value) -> Result<Event, GraphError> {
        let mut e: Event = self.json("POST", &format!("/me/calendars/{calendar_id}/events"), Some(event), &["outlook.timezone=\"UTC\""])?;
        e.calendar_id = calendar_id.to_string();
        Ok(e)
    }

    pub fn update_event(&self, id: &str, patch: &Value) -> Result<Event, GraphError> {
        self.json("PATCH", &format!("/me/events/{id}"), Some(patch), &["outlook.timezone=\"UTC\""])
    }

    pub fn delete_event(&self, id: &str) -> Result<(), GraphError> {
        self.call("DELETE", &format!("/me/events/{id}"), None)
    }

    /// Cancel a meeting you organise, telling the attendees.
    pub fn cancel_event(&self, id: &str, comment: &str) -> Result<(), GraphError> {
        self.call("POST", &format!("/me/events/{id}/cancel"), Some(&json!({"comment": comment})))
    }

    pub fn respond_event(&self, id: &str, response: EventResponse, comment: &str, send_response: bool) -> Result<(), GraphError> {
        let action = match response {
            EventResponse::Accept => "accept",
            EventResponse::Tentative => "tentativelyAccept",
            EventResponse::Decline => "decline",
        };
        self.call("POST", &format!("/me/events/{id}/{action}"), Some(&json!({"comment": comment, "sendResponse": send_response})))
    }

    // ---- people ----------------------------------------------------------------------------

    /// People relevant to the user matching `query` (for address completion).
    pub fn people(&self, query: &str) -> Result<Vec<Person>, GraphError> {
        let mut u = url::Url::parse(&self.url("/me/people")).map_err(|e| GraphError::Config(e.to_string()))?;
        u.query_pairs_mut().append_pair("$search", &format!("\"{}\"", query.replace('"', ""))).append_pair("$top", "10");
        let page: Page<Person> = self.json("GET", u.as_str(), None, &[])?;
        Ok(page.value)
    }
}
