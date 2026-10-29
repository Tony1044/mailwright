//! IMAP (reading, flags, folders, moves) plus SMTP for sending. Messages and folders come back as
//! the same model the Graph client uses, so the store and interface treat an IMAP mailbox the same
//! as a Microsoft 365 one. Connections use implicit TLS (port 993), the common case for every
//! major provider.

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Duration;

use imap::types::{Flag, Name};
use imap::Session;
use mailwright_graph::types::{Attachment, MailFolder, Message};
use mailwright_graph::{ManualConfig, Security};

use crate::tls::Stream;
use crate::{mime, smtp, NetError};

const TIMEOUT: Duration = Duration::from_secs(30);
/// At most this many full messages pulled when filling a folder, to bound bandwidth.
const FILL_CAP: usize = 250;
const SEP: char = '\u{1}';

/// A signed-in IMAP mailbox, with SMTP for sending.
pub struct ImapBackend {
    cfg: ManualConfig,
    password: String,
    address: String,
    inner: Mutex<Inner>,
}

struct Inner {
    session: Option<Session<Stream>>,
    selected: Option<String>,
    /// (full name, is a real/selectable folder, special-use role) learned from LIST.
    folders: Vec<FolderInfo>,
    delimiter: String,
}

#[derive(Clone)]
struct FolderInfo {
    name: String,
    selectable: bool,
    well_known: Option<String>,
}

impl ImapBackend {
    pub fn new(cfg: ManualConfig, password: String, address: String) -> ImapBackend {
        ImapBackend { cfg, password, address, inner: Mutex::new(Inner { session: None, selected: None, folders: Vec::new(), delimiter: "/".into() }) }
    }

    /// Connect and sign in, for the setup page's "Test" button.
    pub fn check(&self) -> Result<(), NetError> {
        let mut g = self.inner.lock().map_err(lock_err)?;
        self.ensure(&mut g)?;
        Ok(())
    }

    pub fn address(&self) -> &str {
        &self.address
    }

    // ---- connection -------------------------------------------------------------------------

    fn ensure<'a>(&self, g: &'a mut Inner) -> Result<&'a mut Session<Stream>, NetError> {
        if g.session.is_none() {
            let inc = &self.cfg.incoming;
            let implicit = inc.security != Security::None;
            let stream = Stream::connect(&inc.host, inc.port, implicit, TIMEOUT)?;
            let client = imap::Client::new(stream);
            let session = client.login(&self.cfg.login, &self.password).map_err(|(e, _)| match e {
                imap::Error::No(m) | imap::Error::Bad(m) => NetError::Auth(m),
                other => NetError::Io(other.to_string()),
            })?;
            g.session = Some(session);
            g.selected = None;
            self.load_folders(g)?;
        }
        g.session.as_mut().ok_or_else(|| NetError::Io("no IMAP session".into()))
    }

    /// Run `f` with the live session; on a connection error, drop the session and try once more.
    fn with<T>(&self, f: impl Fn(&mut Session<Stream>, &mut Inner) -> Result<T, NetError>) -> Result<T, NetError> {
        let mut g = self.inner.lock().map_err(lock_err)?;
        self.ensure(&mut g)?;
        // Borrow the session out so `f` can also touch other Inner fields.
        let mut session = g.session.take().ok_or_else(|| NetError::Io("no IMAP session".into()))?;
        let r = f(&mut session, &mut g);
        match &r {
            Err(NetError::Io(_)) | Err(NetError::Connect(_)) | Err(NetError::Tls(_)) => {
                // Drop the broken session so the next call reconnects.
                g.selected = None;
            }
            _ => g.session = Some(session),
        }
        r
    }

    fn select(session: &mut Session<Stream>, g: &mut Inner, folder: &str) -> Result<imap::types::Mailbox, NetError> {
        let mb = session.select(folder).map_err(imap_err)?;
        g.selected = Some(folder.to_string());
        Ok(mb)
    }

    fn load_folders(&self, g: &mut Inner) -> Result<(), NetError> {
        let session = g.session.as_mut().ok_or_else(|| NetError::Io("no IMAP session".into()))?;
        let names = session.list(Some(""), Some("*")).map_err(imap_err)?;
        let mut folders = Vec::new();
        let mut delimiter = "/".to_string();
        for n in names.iter() {
            if let Some(d) = n.delimiter() {
                delimiter = d.to_string();
            }
            folders.push(FolderInfo { name: n.name().to_string(), selectable: !has_attr(n, "\\Noselect"), well_known: well_known_of(n) });
        }
        // INBOX is always the inbox even without special-use attributes.
        if let Some(f) = folders.iter_mut().find(|f| f.name.eq_ignore_ascii_case("INBOX")) {
            f.well_known = Some("inbox".into());
        }
        g.folders = folders;
        g.delimiter = delimiter;
        Ok(())
    }

    // ---- folders ----------------------------------------------------------------------------

    pub fn all_folders(&self) -> Result<Vec<MailFolder>, NetError> {
        self.with(|_s, g| {
            let delim = g.delimiter.clone();
            Ok(g.folders
                .iter()
                .filter(|f| f.selectable)
                .map(|f| {
                    let (parent, display) = split_parent(&f.name, &delim);
                    MailFolder {
                        id: f.name.clone(),
                        display_name: if f.name.eq_ignore_ascii_case("INBOX") { "Inbox".into() } else { display },
                        parent_folder_id: parent,
                        ..Default::default()
                    }
                })
                .collect())
        })
    }

    pub fn well_known_folder(&self, name: &str) -> Result<MailFolder, NetError> {
        self.with(|session, g| {
            let folder = g.folders.iter().find(|f| f.well_known.as_deref() == Some(name)).map(|f| f.name.clone());
            let Some(folder) = folder else {
                return Err(NetError::Protocol(format!("no {name} folder")));
            };
            let mb = Self::select(session, g, &folder)?;
            let unseen = count_unseen(session).unwrap_or(0);
            Ok(MailFolder {
                id: folder.clone(),
                display_name: if folder.eq_ignore_ascii_case("INBOX") { "Inbox".into() } else { folder.rsplit(&g.delimiter).next().unwrap_or(&folder).to_string() },
                unread_item_count: unseen,
                total_item_count: mb.exists as i64,
                ..Default::default()
            })
        })
    }

    pub fn create_folder(&self, parent: Option<&str>, name: &str) -> Result<MailFolder, NetError> {
        let full = self.with(|session, g| {
            let full = match parent {
                Some(p) if !p.is_empty() => format!("{p}{}{name}", g.delimiter),
                _ => name.to_string(),
            };
            session.create(&full).map_err(imap_err)?;
            Ok(full)
        })?;
        self.refresh_folders()?;
        Ok(MailFolder { id: full.clone(), display_name: name.to_string(), parent_folder_id: parent.map(|p| p.to_string()), ..Default::default() })
    }

    pub fn rename_folder(&self, id: &str, name: &str) -> Result<(), NetError> {
        self.with(|session, g| {
            let (parent, _) = split_parent(id, &g.delimiter);
            let target = match parent {
                Some(p) => format!("{p}{}{name}", g.delimiter),
                None => name.to_string(),
            };
            session.rename(id, &target).map_err(imap_err)
        })?;
        self.refresh_folders()
    }

    pub fn delete_folder(&self, id: &str) -> Result<(), NetError> {
        self.with(|session, _g| session.delete(id).map_err(imap_err))?;
        self.refresh_folders()
    }

    pub fn move_folder(&self, id: &str, dest_parent: &str) -> Result<(), NetError> {
        self.with(|session, g| {
            let (_, leaf) = split_parent(id, &g.delimiter);
            let target = if dest_parent.is_empty() { leaf.clone() } else { format!("{dest_parent}{}{leaf}", g.delimiter) };
            session.rename(id, &target).map_err(imap_err)
        })?;
        self.refresh_folders()
    }

    fn refresh_folders(&self) -> Result<(), NetError> {
        let mut g = self.inner.lock().map_err(lock_err)?;
        self.ensure(&mut g)?;
        self.load_folders(&mut g)
    }

    // ---- messages ---------------------------------------------------------------------------

    /// The newest messages in a folder (up to `max`, bounded by FILL_CAP). Messages already known
    /// get a flags-only update (cheap); new ones are downloaded in full.
    pub fn fetch_folder(&self, folder: &str, known: &HashSet<String>, max: usize) -> Result<Vec<Message>, NetError> {
        let cap = max.min(FILL_CAP);
        self.with(|session, g| {
            let uidvalidity = Self::select(session, g, folder)?.uid_validity.unwrap_or(0);
            let mut uids: Vec<u32> = session.uid_search("ALL").map_err(imap_err)?.into_iter().collect();
            uids.sort_unstable();
            let take: Vec<u32> = uids.iter().rev().take(cap).copied().collect();
            if take.is_empty() {
                return Ok(Vec::new());
            }
            let (seen, fresh): (Vec<u32>, Vec<u32>) = take.iter().partition(|&&uid| known.contains(&make_id(folder, uidvalidity, uid)));
            let mut out = Vec::new();
            // Cached messages: just refresh their flags (a partial update in the store).
            if !seen.is_empty() {
                let fetches = session.uid_fetch(&uids_to_set(&seen), "(UID FLAGS)").map_err(imap_err)?;
                for f in fetches.iter() {
                    if let Some(uid) = f.uid {
                        out.push(flags_update(folder, uidvalidity, uid, f.flags()));
                    }
                }
            }
            // New messages: download in full.
            if !fresh.is_empty() {
                let fetches = session.uid_fetch(&uids_to_set(&fresh), "(UID FLAGS BODY.PEEK[])").map_err(imap_err)?;
                for f in fetches.iter() {
                    if let (Some(uid), Some(body)) = (f.uid, f.body()) {
                        let id = make_id(folder, uidvalidity, uid);
                        out.push(mime::parse(body, &id, folder, has_flag(f.flags(), Flag::Seen), has_flag(f.flags(), Flag::Flagged)).message);
                    }
                }
            }
            Ok(out)
        })
    }

    /// One message, fully parsed (with body and attachments available).
    pub fn message_full(&self, id: &str) -> Result<mime::Parsed, NetError> {
        let (folder, _uv, uid) = parse_id(id)?;
        self.with(|session, g| {
            Self::select(session, g, &folder)?;
            let fetches = session.uid_fetch(uid.to_string(), "(UID FLAGS BODY.PEEK[])").map_err(imap_err)?;
            let f = fetches.iter().next().ok_or_else(|| NetError::Protocol("message not found".into()))?;
            let body = f.body().ok_or_else(|| NetError::Protocol("empty message".into()))?;
            Ok(mime::parse(body, id, &folder, has_flag(f.flags(), Flag::Seen), has_flag(f.flags(), Flag::Flagged)))
        })
    }

    /// Header-only parse (no body), for the missed-mail checks.
    pub fn message_brief(&self, id: &str) -> Result<Message, NetError> {
        let (folder, _uv, uid) = parse_id(id)?;
        self.with(|session, g| {
            Self::select(session, g, &folder)?;
            let fetches = session.uid_fetch(uid.to_string(), "(UID FLAGS BODY.PEEK[HEADER])").map_err(imap_err)?;
            let f = fetches.iter().next().ok_or_else(|| NetError::Protocol("message not found".into()))?;
            let header = f.header().unwrap_or(&[]);
            Ok(mime::parse(header, id, &folder, has_flag(f.flags(), Flag::Seen), has_flag(f.flags(), Flag::Flagged)).message)
        })
    }

    pub fn search(&self, folder: &str, query: &str) -> Result<Vec<Message>, NetError> {
        self.with(|session, g| {
            Self::select(session, g, folder)?;
            let uidvalidity = session.select(folder).map_err(imap_err)?.uid_validity.unwrap_or(0);
            let safe = query.replace('"', " ");
            let uids: Vec<u32> = session.uid_search(format!("TEXT \"{safe}\"")).map_err(imap_err)?.into_iter().collect();
            let take: Vec<u32> = {
                let mut u = uids;
                u.sort_unstable();
                u.into_iter().rev().take(100).collect()
            };
            if take.is_empty() {
                return Ok(Vec::new());
            }
            let fetches = session.uid_fetch(&uids_to_set(&take), "(UID FLAGS BODY.PEEK[])").map_err(imap_err)?;
            let mut out = Vec::new();
            for f in fetches.iter() {
                if let (Some(uid), Some(body)) = (f.uid, f.body()) {
                    let id = make_id(folder, uidvalidity, uid);
                    out.push(mime::parse(body, &id, folder, has_flag(f.flags(), Flag::Seen), has_flag(f.flags(), Flag::Flagged)).message);
                }
            }
            Ok(out)
        })
    }

    pub fn set_read(&self, id: &str, read: bool) -> Result<(), NetError> {
        self.store_flag(id, "\\Seen", read)
    }

    pub fn set_flagged(&self, id: &str, flagged: bool) -> Result<(), NetError> {
        self.store_flag(id, "\\Flagged", flagged)
    }

    fn store_flag(&self, id: &str, flag: &str, on: bool) -> Result<(), NetError> {
        let (folder, _uv, uid) = parse_id(id)?;
        self.with(|session, g| {
            Self::select(session, g, &folder)?;
            let op = if on { "+FLAGS" } else { "-FLAGS" };
            session.uid_store(uid.to_string(), format!("{op} ({flag})")).map_err(imap_err)?;
            Ok(())
        })
    }

    /// Move a message; returns its new id when it can be found again, for the store to follow.
    pub fn move_message(&self, id: &str, dest: &str) -> Result<Option<String>, NetError> {
        let (folder, _uv, uid) = parse_id(id)?;
        self.with(|session, g| {
            Self::select(session, g, &folder)?;
            // The message's Message-ID, to find it again in the destination.
            let msgid = fetch_message_id(session, uid);
            if session.uid_mv(uid.to_string(), dest).is_err() {
                session.uid_copy(uid.to_string(), dest).map_err(imap_err)?;
                session.uid_store(uid.to_string(), "+FLAGS (\\Deleted)").map_err(imap_err)?;
                let _ = session.expunge();
            }
            let dest_uv = Self::select(session, g, dest)?.uid_validity.unwrap_or(0);
            if let Some(mid) = msgid {
                let found: Vec<u32> = session.uid_search(format!("HEADER Message-ID \"{mid}\"")).map_err(imap_err)?.into_iter().collect();
                if let Some(new_uid) = found.into_iter().max() {
                    return Ok(Some(make_id(dest, dest_uv, new_uid)));
                }
            }
            Ok(None)
        })
    }

    pub fn delete_message(&self, id: &str) -> Result<(), NetError> {
        let (folder, _uv, uid) = parse_id(id)?;
        let trash = {
            let mut g = self.inner.lock().map_err(lock_err)?;
            self.ensure(&mut g)?;
            g.folders.iter().find(|f| f.well_known.as_deref() == Some("deleteditems")).map(|f| f.name.clone())
        };
        self.with(|session, g| {
            Self::select(session, g, &folder)?;
            match &trash {
                Some(t) if *t != folder => {
                    if session.uid_mv(uid.to_string(), t).is_err() {
                        session.uid_copy(uid.to_string(), t).map_err(imap_err)?;
                        session.uid_store(uid.to_string(), "+FLAGS (\\Deleted)").map_err(imap_err)?;
                        let _ = session.expunge();
                    }
                }
                _ => {
                    session.uid_store(uid.to_string(), "+FLAGS (\\Deleted)").map_err(imap_err)?;
                    let _ = session.expunge();
                }
            }
            Ok(())
        })
    }

    pub fn attachments(&self, id: &str) -> Result<Vec<(Attachment, Vec<u8>)>, NetError> {
        let parsed = self.message_full(id)?;
        let mut out = Vec::new();
        for (i, a) in parsed.attachments.into_iter().enumerate() {
            out.push((
                Attachment {
                    id: format!("att:{i}"),
                    name: Some(a.name),
                    content_type: Some(a.content_type),
                    size: a.bytes.len() as i64,
                    is_inline: a.is_inline,
                    content_id: a.content_id,
                    ..Default::default()
                },
                a.bytes,
            ));
        }
        Ok(out)
    }

    // ---- sending ----------------------------------------------------------------------------

    /// Send a built RFC 822 message over SMTP, then file a copy in the Sent folder.
    pub fn send(&self, from: &str, rcpts: &[String], raw: &[u8]) -> Result<(), NetError> {
        smtp::send(&self.cfg.smtp, &self.cfg.login, &self.password, from, rcpts, raw)?;
        // Keep a copy in Sent, best effort.
        let sent = {
            let mut g = self.inner.lock().map_err(lock_err)?;
            self.ensure(&mut g).ok();
            g.folders.iter().find(|f| f.well_known.as_deref() == Some("sentitems")).map(|f| f.name.clone())
        };
        if let Some(folder) = sent {
            let _ = self.with(|session, _g| session.append_with_flags(&folder, raw, &[Flag::Seen]).map_err(imap_err));
        }
        Ok(())
    }

    /// Save a draft to the Drafts folder.
    pub fn save_draft(&self, raw: &[u8]) -> Result<(), NetError> {
        let drafts = {
            let mut g = self.inner.lock().map_err(lock_err)?;
            self.ensure(&mut g)?;
            g.folders.iter().find(|f| f.well_known.as_deref() == Some("drafts")).map(|f| f.name.clone())
        };
        let Some(folder) = drafts else {
            return Err(NetError::Unsupported("This account has no Drafts folder.".into()));
        };
        self.with(|session, _g| session.append_with_flags(&folder, raw, &[Flag::Draft, Flag::Seen]).map_err(imap_err))
    }
}

// ---- helpers --------------------------------------------------------------------------------

fn lock_err<T>(_: std::sync::PoisonError<T>) -> NetError {
    NetError::Io("internal lock error".into())
}

fn imap_err(e: imap::Error) -> NetError {
    match e {
        imap::Error::No(m) => NetError::Protocol(m),
        imap::Error::Bad(m) => NetError::Protocol(m),
        imap::Error::ConnectionLost => NetError::Io("the connection was lost".into()),
        imap::Error::Io(e) => NetError::Io(e.to_string()),
        other => NetError::Io(other.to_string()),
    }
}

fn has_attr(n: &Name, attr: &str) -> bool {
    n.attributes().iter().any(|a| format!("{a:?}").contains(attr.trim_start_matches('\\')))
}

fn has_flag(flags: &[Flag], want: Flag) -> bool {
    flags.iter().any(|f| std::mem::discriminant(f) == std::mem::discriminant(&want))
}

/// Map a folder's special-use attributes to a Graph-style well-known name.
fn well_known_of(n: &Name) -> Option<String> {
    for a in n.attributes() {
        let s = format!("{a:?}");
        let found = if s.contains("Sent") {
            "sentitems"
        } else if s.contains("Drafts") {
            "drafts"
        } else if s.contains("Trash") || s.contains("Deleted") {
            "deleteditems"
        } else if s.contains("Junk") || s.contains("Spam") {
            "junkemail"
        } else if s.contains("Archive") {
            "archive"
        } else {
            continue;
        };
        return Some(found.to_string());
    }
    // Fall back to common folder names.
    let lower = n.name().to_ascii_lowercase();
    let guess = match lower.rsplit(['/', '.']).next().unwrap_or(&lower) {
        "sent" | "sent items" | "sent mail" => "sentitems",
        "drafts" => "drafts",
        "trash" | "deleted" | "deleted items" | "bin" => "deleteditems",
        "junk" | "spam" | "junk email" => "junkemail",
        "archive" => "archive",
        _ => return None,
    };
    Some(guess.to_string())
}

fn split_parent(name: &str, delim: &str) -> (Option<String>, String) {
    match name.rfind(delim) {
        Some(i) => (Some(name[..i].to_string()), name[i + delim.len()..].to_string()),
        None => (None, name.to_string()),
    }
}

/// A partial message carrying only the current flags, so the store updates read/flag state
/// without touching the rest (it has no subject or date, which marks it as a partial update).
fn flags_update(folder: &str, uidvalidity: u32, uid: u32, flags: &[Flag]) -> Message {
    let flag = has_flag(flags, Flag::Flagged).then(|| mailwright_graph::types::FollowupFlag { flag_status: "flagged".into() }).or_else(|| Some(mailwright_graph::types::FollowupFlag { flag_status: "notFlagged".into() }));
    Message {
        id: make_id(folder, uidvalidity, uid),
        parent_folder_id: Some(folder.to_string()),
        is_read: Some(has_flag(flags, Flag::Seen)),
        flag,
        ..Default::default()
    }
}

fn make_id(folder: &str, uidvalidity: u32, uid: u32) -> String {
    format!("imap{SEP}{folder}{SEP}{uidvalidity}{SEP}{uid}")
}

fn parse_id(id: &str) -> Result<(String, u32, u32), NetError> {
    let parts: Vec<&str> = id.split(SEP).collect();
    if parts.len() == 4 && parts[0] == "imap" {
        Ok((parts[1].to_string(), parts[2].parse().unwrap_or(0), parts[3].parse().map_err(|_| NetError::Protocol("bad message id".into()))?))
    } else {
        Err(NetError::Protocol(format!("not an IMAP message id: {id}")))
    }
}

fn uids_to_set(uids: &[u32]) -> String {
    uids.iter().map(|u| u.to_string()).collect::<Vec<_>>().join(",")
}

fn count_unseen(session: &mut Session<Stream>) -> Option<i64> {
    session.uid_search("UNSEEN").ok().map(|s| s.len() as i64)
}

fn fetch_message_id(session: &mut Session<Stream>, uid: u32) -> Option<String> {
    let fetches = session.uid_fetch(uid.to_string(), "(BODY.PEEK[HEADER.FIELDS (MESSAGE-ID)])").ok()?;
    let f = fetches.iter().next()?;
    let header = f.header()?;
    let text = String::from_utf8_lossy(header);
    let line = text.lines().find(|l| l.to_ascii_lowercase().starts_with("message-id:"))?;
    let v = line.splitn(2, ':').nth(1)?.trim();
    Some(v.trim_matches(['<', '>']).to_string())
}
