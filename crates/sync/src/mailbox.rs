//! The mailbox operations the sync engine needs, as a trait, so a mailbox can be Microsoft 365
//! (Graph) or a plain IMAP/POP3 + SMTP account behind the same engine. Calendar, contacts, and
//! out-of-office are Graph-only; the default implementations say so, and the plain backends inherit
//! them.

use mailwright_graph::client::{DeltaPage, EventResponse};
use mailwright_graph::types::{Attachment, AutomaticRepliesSetting, Calendar, Event, MailFolder, Message, Person, User};
use mailwright_graph::{AccountConfig, GraphError};
use mailwright_store::{Compose, Store};

/// The result of sending or saving a message.
#[derive(Default)]
pub struct Submit {
    /// The server's id for the saved draft, when there is one.
    pub draft_id: Option<String>,
}

fn unsupported<T>(what: &str) -> Result<T, GraphError> {
    Err(GraphError::Config(format!("{what} isn't available for this kind of account.")))
}

/// Everything the engine does with a mailbox. Implemented by the Graph client and by the plain
/// IMAP/POP3 backends.
pub trait Mailbox: Send + Sync {
    fn config(&self) -> &AccountConfig;
    fn me(&self) -> Result<User, GraphError>;
    fn set_user(&mut self, _username: &str, _display_name: &str) -> Result<(), GraphError> {
        Ok(())
    }
    fn sign_out(&self) -> Result<(), GraphError> {
        Ok(())
    }

    // Folders.
    fn all_folders(&self) -> Result<Vec<MailFolder>, GraphError>;
    fn well_known_folder(&self, name: &str) -> Result<MailFolder, GraphError>;
    fn create_folder(&self, _parent_id: Option<&str>, _name: &str) -> Result<MailFolder, GraphError> {
        unsupported("Creating folders")
    }
    fn rename_folder(&self, _id: &str, _name: &str) -> Result<(), GraphError> {
        unsupported("Renaming folders")
    }
    fn move_folder(&self, _id: &str, _destination: &str) -> Result<(), GraphError> {
        unsupported("Moving folders")
    }
    fn delete_folder(&self, _id: &str) -> Result<(), GraphError> {
        unsupported("Deleting folders")
    }

    // Messages.
    fn messages_delta(&self, _folder_id: &str, _link: Option<&str>, _since: Option<&str>) -> Result<DeltaPage<Message>, GraphError> {
        Ok(DeltaPage::default())
    }
    fn folder_messages_newest(&self, folder_id: &str, max: usize) -> Result<Vec<Message>, GraphError>;
    /// Like `folder_messages_newest`, but told which messages are already cached, so a backend can
    /// skip re-downloading them (IMAP refreshes just their flags; POP3 doesn't fetch them again).
    /// Graph ignores `known`. Used by the reconcile path for plain mailboxes.
    fn fetch_folder(&self, folder_id: &str, _known: &std::collections::HashSet<String>, max: usize) -> Result<Vec<Message>, GraphError> {
        self.folder_messages_newest(folder_id, max)
    }
    fn newest_messages(&self, folder_id: &str, top: usize) -> Result<Vec<Message>, GraphError> {
        self.folder_messages_newest(folder_id, top)
    }
    fn message_brief(&self, id: &str) -> Result<Message, GraphError>;
    fn message_full(&self, id: &str) -> Result<Message, GraphError>;
    fn search_messages(&self, query: &str, top: usize) -> Result<Vec<Message>, GraphError>;

    fn set_read(&self, id: &str, read: bool) -> Result<(), GraphError>;
    fn set_flag(&self, id: &str, status: &str) -> Result<(), GraphError>;
    fn set_categories(&self, _id: &str, _categories: &[String]) -> Result<(), GraphError> {
        Ok(())
    }
    fn move_message(&self, id: &str, destination: &str) -> Result<Message, GraphError>;
    fn delete_message(&self, id: &str) -> Result<(), GraphError>;

    // Attachments.
    fn attachments(&self, message_id: &str) -> Result<Vec<Attachment>, GraphError>;
    fn attachment(&self, message_id: &str, attachment_id: &str) -> Result<Attachment, GraphError>;
    fn attachment_bytes(&self, message_id: &str, attachment_id: &str) -> Result<Vec<u8>, GraphError>;

    // Sending and drafts.
    fn submit(&self, compose: &Compose, send: bool, store: &Store, seq: i64) -> Result<Submit, GraphError>;

    // Calendar and contacts (Graph only).
    fn calendars(&self) -> Result<Vec<Calendar>, GraphError> {
        Ok(Vec::new())
    }
    fn calendar_view(&self, _calendar_id: &str, _start: &str, _end: &str) -> Result<Vec<Event>, GraphError> {
        Ok(Vec::new())
    }
    fn event_full(&self, _id: &str) -> Result<Event, GraphError> {
        unsupported("Calendar")
    }
    fn meeting_event(&self, _id: &str) -> Result<(Option<String>, Option<Event>), GraphError> {
        Ok((None, None))
    }
    fn create_event(&self, _calendar_id: &str, _event: &serde_json::Value) -> Result<Event, GraphError> {
        unsupported("Calendar")
    }
    fn update_event(&self, _id: &str, _patch: &serde_json::Value) -> Result<Event, GraphError> {
        unsupported("Calendar")
    }
    fn delete_event(&self, _id: &str) -> Result<(), GraphError> {
        unsupported("Calendar")
    }
    fn cancel_event(&self, _id: &str, _comment: &str) -> Result<(), GraphError> {
        unsupported("Calendar")
    }
    fn respond_event(&self, _id: &str, _response: EventResponse, _comment: &str, _send_response: bool) -> Result<(), GraphError> {
        unsupported("Calendar")
    }
    fn people(&self, _query: &str) -> Result<Vec<Person>, GraphError> {
        Ok(Vec::new())
    }

    // Out-of-office (Graph only).
    fn automatic_replies(&self) -> Result<AutomaticRepliesSetting, GraphError> {
        unsupported("Automatic replies")
    }
    fn set_automatic_replies(&self, _setting: &AutomaticRepliesSetting) -> Result<(), GraphError> {
        unsupported("Automatic replies")
    }

    /// Whether this is a plain IMAP/POP3 mailbox (no calendar or contacts).
    fn is_manual(&self) -> bool {
        self.config().is_manual()
    }
}

// ---- Microsoft 365 (Graph) --------------------------------------------------------------------

use mailwright_graph::client::ReplyKind;
use mailwright_graph::GraphClient;
use mailwright_store::Action;
use serde_json::json;

impl Mailbox for GraphClient {
    fn config(&self) -> &AccountConfig {
        GraphClient::config(self)
    }
    fn me(&self) -> Result<User, GraphError> {
        GraphClient::me(self)
    }
    fn set_user(&mut self, username: &str, display_name: &str) -> Result<(), GraphError> {
        GraphClient::set_user(self, username, display_name)
    }
    fn sign_out(&self) -> Result<(), GraphError> {
        GraphClient::sign_out(self)
    }
    fn all_folders(&self) -> Result<Vec<MailFolder>, GraphError> {
        GraphClient::all_folders(self)
    }
    fn well_known_folder(&self, name: &str) -> Result<MailFolder, GraphError> {
        GraphClient::well_known_folder(self, name)
    }
    fn create_folder(&self, parent_id: Option<&str>, name: &str) -> Result<MailFolder, GraphError> {
        GraphClient::create_folder(self, parent_id, name)
    }
    fn rename_folder(&self, id: &str, name: &str) -> Result<(), GraphError> {
        GraphClient::rename_folder(self, id, name)
    }
    fn move_folder(&self, id: &str, destination: &str) -> Result<(), GraphError> {
        GraphClient::move_folder(self, id, destination)
    }
    fn delete_folder(&self, id: &str) -> Result<(), GraphError> {
        GraphClient::delete_folder(self, id)
    }
    fn messages_delta(&self, folder_id: &str, link: Option<&str>, since: Option<&str>) -> Result<DeltaPage<Message>, GraphError> {
        GraphClient::messages_delta(self, folder_id, link, since)
    }
    fn folder_messages_newest(&self, folder_id: &str, max: usize) -> Result<Vec<Message>, GraphError> {
        GraphClient::folder_messages_newest(self, folder_id, max)
    }
    fn newest_messages(&self, folder_id: &str, top: usize) -> Result<Vec<Message>, GraphError> {
        GraphClient::newest_messages(self, folder_id, top)
    }
    fn message_brief(&self, id: &str) -> Result<Message, GraphError> {
        GraphClient::message_brief(self, id)
    }
    fn message_full(&self, id: &str) -> Result<Message, GraphError> {
        GraphClient::message_full(self, id)
    }
    fn search_messages(&self, query: &str, top: usize) -> Result<Vec<Message>, GraphError> {
        GraphClient::search_messages(self, query, top)
    }
    fn set_read(&self, id: &str, read: bool) -> Result<(), GraphError> {
        GraphClient::set_read(self, id, read)
    }
    fn set_flag(&self, id: &str, status: &str) -> Result<(), GraphError> {
        GraphClient::set_flag(self, id, status)
    }
    fn set_categories(&self, id: &str, categories: &[String]) -> Result<(), GraphError> {
        GraphClient::set_categories(self, id, categories)
    }
    fn move_message(&self, id: &str, destination: &str) -> Result<Message, GraphError> {
        GraphClient::move_message(self, id, destination)
    }
    fn delete_message(&self, id: &str) -> Result<(), GraphError> {
        GraphClient::delete_message(self, id)
    }
    fn attachments(&self, message_id: &str) -> Result<Vec<Attachment>, GraphError> {
        GraphClient::attachments(self, message_id)
    }
    fn attachment(&self, message_id: &str, attachment_id: &str) -> Result<Attachment, GraphError> {
        GraphClient::attachment(self, message_id, attachment_id)
    }
    fn attachment_bytes(&self, message_id: &str, attachment_id: &str) -> Result<Vec<u8>, GraphError> {
        GraphClient::attachment_bytes(self, message_id, attachment_id)
    }
    fn calendars(&self) -> Result<Vec<Calendar>, GraphError> {
        GraphClient::calendars(self)
    }
    fn calendar_view(&self, calendar_id: &str, start: &str, end: &str) -> Result<Vec<Event>, GraphError> {
        GraphClient::calendar_view(self, calendar_id, start, end)
    }
    fn event_full(&self, id: &str) -> Result<Event, GraphError> {
        GraphClient::event_full(self, id)
    }
    fn meeting_event(&self, id: &str) -> Result<(Option<String>, Option<Event>), GraphError> {
        GraphClient::meeting_event(self, id)
    }
    fn create_event(&self, calendar_id: &str, event: &serde_json::Value) -> Result<Event, GraphError> {
        GraphClient::create_event(self, calendar_id, event)
    }
    fn update_event(&self, id: &str, patch: &serde_json::Value) -> Result<Event, GraphError> {
        GraphClient::update_event(self, id, patch)
    }
    fn delete_event(&self, id: &str) -> Result<(), GraphError> {
        GraphClient::delete_event(self, id)
    }
    fn cancel_event(&self, id: &str, comment: &str) -> Result<(), GraphError> {
        GraphClient::cancel_event(self, id, comment)
    }
    fn respond_event(&self, id: &str, response: EventResponse, comment: &str, send_response: bool) -> Result<(), GraphError> {
        GraphClient::respond_event(self, id, response, comment, send_response)
    }
    fn people(&self, query: &str) -> Result<Vec<Person>, GraphError> {
        GraphClient::people(self, query)
    }
    fn automatic_replies(&self) -> Result<AutomaticRepliesSetting, GraphError> {
        GraphClient::automatic_replies(self)
    }
    fn set_automatic_replies(&self, setting: &AutomaticRepliesSetting) -> Result<(), GraphError> {
        GraphClient::set_automatic_replies(self, setting)
    }

    fn submit(&self, compose: &Compose, send: bool, store: &Store, seq: i64) -> Result<Submit, GraphError> {
        let mut compose = compose.clone();
        // 1. A draft on the server.
        let draft_id = match (&compose.draft_id, &compose.reply_to) {
            (Some(d), _) => match self.update_draft(d, &crate::compose_patch(&compose, true)) {
                Ok(m) => m.id,
                Err(e) if e.is_not_found() => {
                    let mut full = crate::compose_patch(&compose, true);
                    full["body"] = json!({"contentType": "html", "content": compose.html});
                    self.create_draft(&full)?.id
                }
                Err(e) => return Err(e),
            },
            (None, Some((orig, kind))) => {
                let kind = match kind.as_str() {
                    "replyAll" => ReplyKind::ReplyAll,
                    "forward" => ReplyKind::Forward,
                    _ => ReplyKind::Reply,
                };
                let d = self.create_reply(orig, kind)?;
                self.update_draft(&d.id, &crate::compose_patch(&compose, true))?;
                d.id
            }
            (None, None) => self.create_draft(&crate::compose_patch(&compose, true))?.id,
        };
        compose.draft_id = Some(draft_id.clone());
        compose.reply_to = None;
        let remember = |c: &Compose| if send { Action::Send { compose: c.clone() } } else { Action::SaveDraft { compose: c.clone() } };
        let _ = store.update_action(seq, &remember(&compose));
        // 2. Attachments, one at a time, each remembered once it's there.
        while let Some(a) = compose.attachments.first().cloned() {
            let bytes = base64::engine::general_purpose::STANDARD.decode(a.data.as_bytes()).map_err(|e| GraphError::BadResponse(format!("{}: {e}", a.name)))?;
            self.add_attachment(&draft_id, &a.name, &a.content_type, &bytes, a.content_id.as_deref())?;
            compose.attachments.remove(0);
            let _ = store.update_action(seq, &remember(&compose));
        }
        // 3. Send.
        if send {
            self.send_draft(&draft_id)?;
        }
        Ok(Submit { draft_id: Some(draft_id) })
    }
}

use base64::Engine as _;
