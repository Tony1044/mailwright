//! Wraps the IMAP and POP3 backends as a [`Mailbox`], so the engine drives a plain mailbox exactly
//! as it drives a Microsoft 365 one. Calendar, contacts, and out-of-office fall back to the trait's
//! "not available" defaults.

use std::collections::HashSet;

use mailwright_graph::types::{Attachment, EmailAddress, MailFolder, Message, Recipient, User};
use mailwright_graph::{AccountConfig, GraphError, MailProtocol};
use mailwright_net::imap_backend::ImapBackend;
use mailwright_net::pop_backend::{PopBackend, INBOX_ID};
use mailwright_net::{mime, to_graph_error};
use mailwright_store::{Addr, Compose, Store};

use crate::mailbox::{Mailbox, Submit};

enum Backend {
    Imap(ImapBackend),
    Pop(PopBackend),
}

/// A plain IMAP or POP3 mailbox.
pub struct NetMailbox {
    config: AccountConfig,
    address: String,
    display_name: String,
    backend: Backend,
}

impl NetMailbox {
    /// Build a mailbox from a saved account and its password.
    pub fn new(config: AccountConfig, password: String) -> Result<NetMailbox, GraphError> {
        let manual = config.manual.clone().ok_or_else(|| GraphError::Config("not a plain mailbox".into()))?;
        let address = config.username.clone();
        let display_name = if config.display_name.is_empty() { address.clone() } else { config.display_name.clone() };
        let backend = match manual.protocol {
            MailProtocol::Imap => Backend::Imap(ImapBackend::new(manual, password, address.clone())),
            MailProtocol::Pop => Backend::Pop(PopBackend::new(manual, password)),
        };
        Ok(NetMailbox { config, address, display_name, backend })
    }

    /// Check sign-in works (for the setup page).
    pub fn check(config: &AccountConfig, password: &str) -> Result<(), GraphError> {
        let mb = NetMailbox::new(config.clone(), password.to_string())?;
        match &mb.backend {
            Backend::Imap(i) => i.check().map_err(to_graph_error),
            Backend::Pop(p) => p.check().map_err(to_graph_error),
        }
    }

    fn message_id(&self) -> String {
        let n = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
        let domain = self.address.rsplit('@').next().unwrap_or("mailwright.local");
        format!("mw-{n:x}@{domain}")
    }

    /// Build the RFC 822 bytes for a compose.
    fn build(&self, compose: &Compose) -> Vec<u8> {
        let from = Addr::new(&self.display_name, &self.address);
        let text = mime::html_to_text(&compose.html);
        let atts: Vec<mime::OutAttach> = compose
            .attachments
            .iter()
            .map(|a| mime::OutAttach {
                name: &a.name,
                content_type: &a.content_type,
                bytes: &[], // filled below
                content_id: a.content_id.as_deref(),
            })
            .collect();
        // The attachment bytes need decoding; do it into owned buffers the OutAttach can borrow.
        let decoded: Vec<Vec<u8>> = compose
            .attachments
            .iter()
            .map(|a| base64::engine::general_purpose::STANDARD.decode(a.data.as_bytes()).unwrap_or_default())
            .collect();
        let atts: Vec<mime::OutAttach> = atts
            .into_iter()
            .enumerate()
            .map(|(i, mut a)| {
                a.bytes = &decoded[i];
                a
            })
            .collect();
        let mid = self.message_id();
        let mail = mime::OutMail {
            from: &from,
            to: &compose.to,
            cc: &compose.cc,
            subject: &compose.subject,
            html: &compose.html,
            text: &text,
            attachments: &atts,
            message_id: &mid,
            in_reply_to: None,
            importance: &compose.importance,
            request_read_receipt: compose.request_read_receipt,
        };
        mime::build(&mail)
    }
}

use base64::Engine as _;

impl Mailbox for NetMailbox {
    fn config(&self) -> &AccountConfig {
        &self.config
    }

    fn me(&self) -> Result<User, GraphError> {
        Ok(User { display_name: Some(self.display_name.clone()), mail: Some(self.address.clone()), user_principal_name: Some(self.address.clone()), ..Default::default() })
    }

    fn all_folders(&self) -> Result<Vec<MailFolder>, GraphError> {
        match &self.backend {
            Backend::Imap(i) => i.all_folders().map_err(to_graph_error),
            Backend::Pop(_) => Ok(vec![PopBackend::inbox_folder()]),
        }
    }

    fn well_known_folder(&self, name: &str) -> Result<MailFolder, GraphError> {
        match &self.backend {
            Backend::Imap(i) => i.well_known_folder(name).map_err(to_graph_error),
            Backend::Pop(_) => {
                if name == "inbox" {
                    Ok(PopBackend::inbox_folder())
                } else {
                    Err(GraphError::Api { status: 404, code: "notFound".into(), message: "no such folder".into() })
                }
            }
        }
    }

    fn create_folder(&self, parent_id: Option<&str>, name: &str) -> Result<MailFolder, GraphError> {
        match &self.backend {
            Backend::Imap(i) => i.create_folder(parent_id, name).map_err(to_graph_error),
            Backend::Pop(_) => Err(GraphError::Config("POP3 accounts have only an Inbox.".into())),
        }
    }
    fn rename_folder(&self, id: &str, name: &str) -> Result<(), GraphError> {
        match &self.backend {
            Backend::Imap(i) => i.rename_folder(id, name).map_err(to_graph_error),
            Backend::Pop(_) => Err(GraphError::Config("POP3 accounts have only an Inbox.".into())),
        }
    }
    fn move_folder(&self, id: &str, destination: &str) -> Result<(), GraphError> {
        match &self.backend {
            Backend::Imap(i) => i.move_folder(id, destination).map_err(to_graph_error),
            Backend::Pop(_) => Err(GraphError::Config("POP3 accounts have only an Inbox.".into())),
        }
    }
    fn delete_folder(&self, id: &str) -> Result<(), GraphError> {
        match &self.backend {
            Backend::Imap(i) => i.delete_folder(id).map_err(to_graph_error),
            Backend::Pop(_) => Err(GraphError::Config("POP3 accounts have only an Inbox.".into())),
        }
    }

    fn folder_messages_newest(&self, folder_id: &str, max: usize) -> Result<Vec<Message>, GraphError> {
        self.fetch_folder(folder_id, &HashSet::new(), max)
    }

    fn fetch_folder(&self, folder_id: &str, known: &HashSet<String>, max: usize) -> Result<Vec<Message>, GraphError> {
        match &self.backend {
            Backend::Imap(i) => i.fetch_folder(folder_id, known, max).map_err(to_graph_error),
            Backend::Pop(p) => {
                if folder_id != INBOX_ID {
                    return Ok(Vec::new());
                }
                p.fetch_new(known, max).map_err(to_graph_error)
            }
        }
    }

    fn message_brief(&self, id: &str) -> Result<Message, GraphError> {
        match &self.backend {
            Backend::Imap(i) => i.message_brief(id).map_err(to_graph_error),
            Backend::Pop(p) => p.fetch_full(id).map(|parsed| parsed.message).map_err(to_graph_error),
        }
    }

    fn message_full(&self, id: &str) -> Result<Message, GraphError> {
        match &self.backend {
            Backend::Imap(i) => i.message_full(id).map(|parsed| parsed.message).map_err(to_graph_error),
            Backend::Pop(p) => p.fetch_full(id).map(|parsed| parsed.message).map_err(to_graph_error),
        }
    }

    fn search_messages(&self, query: &str, _top: usize) -> Result<Vec<Message>, GraphError> {
        match &self.backend {
            Backend::Imap(i) => i.search("INBOX", query).map_err(to_graph_error),
            Backend::Pop(_) => Ok(Vec::new()),
        }
    }

    fn set_read(&self, id: &str, read: bool) -> Result<(), GraphError> {
        match &self.backend {
            Backend::Imap(i) => i.set_read(id, read).map_err(to_graph_error),
            Backend::Pop(_) => Ok(()),
        }
    }
    fn set_flag(&self, id: &str, status: &str) -> Result<(), GraphError> {
        match &self.backend {
            Backend::Imap(i) => i.set_flagged(id, status == "flagged").map_err(to_graph_error),
            Backend::Pop(_) => Ok(()),
        }
    }

    fn move_message(&self, id: &str, destination: &str) -> Result<Message, GraphError> {
        match &self.backend {
            Backend::Imap(i) => {
                let new_id = i.move_message(id, destination).map_err(to_graph_error)?;
                Ok(Message { id: new_id.unwrap_or_default(), ..Default::default() })
            }
            Backend::Pop(_) => Err(GraphError::Config("POP3 accounts can't move messages.".into())),
        }
    }

    fn delete_message(&self, id: &str) -> Result<(), GraphError> {
        match &self.backend {
            Backend::Imap(i) => i.delete_message(id).map_err(to_graph_error),
            Backend::Pop(p) => p.delete(std::slice::from_ref(&id.to_string())).map_err(to_graph_error),
        }
    }

    fn attachments(&self, message_id: &str) -> Result<Vec<Attachment>, GraphError> {
        match &self.backend {
            Backend::Imap(i) => Ok(i.attachments(message_id).map_err(to_graph_error)?.into_iter().map(|(a, _)| a).collect()),
            Backend::Pop(p) => {
                let parsed = p.fetch_full(message_id).map_err(to_graph_error)?;
                Ok(parsed.attachments.into_iter().enumerate().map(|(i, a)| attachment_meta(i, &a)).collect())
            }
        }
    }

    fn attachment(&self, message_id: &str, attachment_id: &str) -> Result<Attachment, GraphError> {
        let (meta, bytes) = self.part(message_id, attachment_id)?;
        let mut meta = meta;
        meta.content_bytes = Some(base64::engine::general_purpose::STANDARD.encode(&bytes));
        Ok(meta)
    }

    fn attachment_bytes(&self, message_id: &str, attachment_id: &str) -> Result<Vec<u8>, GraphError> {
        Ok(self.part(message_id, attachment_id)?.1)
    }

    fn submit(&self, compose: &Compose, send: bool, _store: &Store, _seq: i64) -> Result<Submit, GraphError> {
        let raw = self.build(compose);
        if send {
            let mut rcpts: Vec<String> = Vec::new();
            for a in compose.to.iter().chain(compose.cc.iter()).chain(compose.bcc.iter()) {
                if !a.address.trim().is_empty() {
                    rcpts.push(a.address.clone());
                }
            }
            if rcpts.is_empty() {
                return Err(GraphError::Config("There are no recipients to send to.".into()));
            }
            match &self.backend {
                Backend::Imap(i) => i.send(&self.address, &rcpts, &raw).map_err(to_graph_error)?,
                Backend::Pop(p) => p.send(&self.address, &rcpts, &raw).map_err(to_graph_error)?,
            }
            Ok(Submit::default())
        } else {
            match &self.backend {
                Backend::Imap(i) => i.save_draft(&raw).map_err(to_graph_error)?,
                Backend::Pop(_) => {} // POP3 has no server folders; the draft stays on this computer.
            }
            Ok(Submit::default())
        }
    }
}

impl NetMailbox {
    /// One attachment's metadata and bytes, by its `att:N` id.
    fn part(&self, message_id: &str, attachment_id: &str) -> Result<(Attachment, Vec<u8>), GraphError> {
        let idx: usize = attachment_id.strip_prefix("att:").and_then(|n| n.parse().ok()).ok_or_else(|| GraphError::Config("unknown attachment".into()))?;
        match &self.backend {
            Backend::Imap(i) => {
                let mut atts = i.attachments(message_id).map_err(to_graph_error)?;
                if idx < atts.len() {
                    Ok(atts.swap_remove(idx))
                } else {
                    Err(GraphError::Api { status: 404, code: "notFound".into(), message: "attachment not found".into() })
                }
            }
            Backend::Pop(p) => {
                let parsed = p.fetch_full(message_id).map_err(to_graph_error)?;
                let a = parsed.attachments.into_iter().nth(idx).ok_or_else(|| GraphError::Api { status: 404, code: "notFound".into(), message: "attachment not found".into() })?;
                Ok((attachment_meta(idx, &a), a.bytes))
            }
        }
    }
}

fn attachment_meta(i: usize, a: &mime::ParsedAttachment) -> Attachment {
    Attachment {
        id: format!("att:{i}"),
        name: Some(a.name.clone()),
        content_type: Some(a.content_type.clone()),
        size: a.bytes.len() as i64,
        is_inline: a.is_inline,
        content_id: a.content_id.clone(),
        ..Default::default()
    }
}

/// Build a Recipient (used when constructing model messages directly, if needed).
#[allow(dead_code)]
fn recipient(name: &str, addr: &str) -> Recipient {
    Recipient { email_address: EmailAddress { name: Some(name.to_string()), address: Some(addr.to_string()) } }
}
