//! The sync engine: one background thread that owns the Graph client and a connection to the
//! cache. The interface sends it [`Command`]s and gets [`Update`]s back (and a wake-up call so it
//! can repaint).
//!
//! Each round (every minute, or on Send/Receive) it first sends queued changes, then runs delta
//! queries for the core folders and every folder already in the cache, refreshes the folder tree
//! and counts now and then, refreshes the calendar window on screen, and in quiet moments fetches
//! the bodies of recent mail so they can be read offline. When the network is down it says so,
//! keeps changes queued and tries again.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod mailbox;
pub mod net_mailbox;

pub use mailbox::Mailbox;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::time::{Duration, Instant};

use base64::Engine as _;
use mailwright_graph::client::EventResponse;
use mailwright_graph::GraphError;
use mailwright_store::{Action, Addr, Compose, QueuedAction, Store};
use serde_json::json;

/// Folders always kept in step, by well-known name.
pub const CORE_FOLDERS: &[&str] = &["inbox", "drafts", "sentitems", "deleteditems", "outbox"];
const WELL_KNOWN: &[&str] = &["inbox", "drafts", "sentitems", "deleteditems", "junkemail", "archive", "outbox", "conversationhistory"];

/// What the interface asks the engine to do.
pub enum Command {
    /// Signed in: start using this mailbox.
    Attach(Box<dyn Mailbox>),
    /// Send/Receive: a full round now.
    SyncAll,
    /// A folder was opened: bring it into the cache (first time) or up to date.
    SyncFolder(String),
    /// A message was opened: fetch its body, attachments, inline images and meeting details.
    Open(String),
    Do(Action),
    /// The calendar is showing this window (Unix seconds).
    CalendarWindow { from: i64, to: i64 },
    OpenEvent(String),
    SaveAttachment { message_id: String, attachment_id: String, path: PathBuf, open: bool },
    /// Search the whole mailbox on the server.
    SearchServer(String),
    People(String),
    CreateFolder { parent: Option<String>, name: String },
    RenameFolder { id: String, name: String },
    DeleteFolder { id: String },
    WorkOffline(bool),
    /// How far back to keep mail in the cache, in days.
    SetWindow(i64),
    /// Fetch the mailbox's automatic-replies (out-of-office) setting.
    GetAutoReply,
    /// Save the mailbox's automatic-replies setting.
    SetAutoReply(Box<mailwright_graph::types::AutomaticRepliesSetting>),
    SignOut,
    Shutdown,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Update {
    /// Status bar text.
    Status(String),
    Connected(bool),
    /// Waiting changes.
    Queued(i64),
    Account { name: String, address: String },
    FoldersChanged,
    MessagesChanged,
    MessageReady(String),
    CalendarChanged,
    People(String, Vec<Addr>),
    SearchResults(String, Vec<String>),
    AttachmentSaved { path: PathBuf, open: bool },
    Sent(String),
    /// Mail that arrived in the Inbox since the last round (not on the first sync).
    NewMail(Vec<NewMail>),
    /// A draft reached the server: the compose window with this local id now edits that draft.
    DraftSaved { local_id: String, draft_id: String },
    /// The mailbox's automatic-replies setting (after a Get or a successful Set).
    AutoReply(Box<mailwright_graph::types::AutomaticRepliesSetting>),
    /// Automatic replies couldn't be reached (e.g. the extra permission isn't granted yet).
    AutoReplyError(String),
    /// Something went wrong that the user should see.
    Error(String),
    SignInRequired(String),
    SignedOut,
}

/// One newly arrived message, for the notification.
#[derive(Debug, Clone, PartialEq)]
pub struct NewMail {
    pub id: String,
    pub from: String,
    pub subject: String,
    pub preview: String,
    pub focused: bool,
}

/// After this many failed tries a queued change is given up on, so it can't hold up the rest.
const MAX_ATTEMPTS: i64 = 8;

/// The most messages to pull when a folder is first opened (newest first). Enough that even a
/// large archive folder looks complete, without downloading tens of thousands at once.
const MAX_FOLDER_FILL: usize = 2000;

pub struct SyncHandle {
    tx: Sender<Command>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl SyncHandle {
    pub fn send(&self, c: Command) {
        let _ = self.tx.send(c);
    }
}

impl Drop for SyncHandle {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// Start the engine on the cache at `db`. Updates arrive on the receiver; `wake` is called after
/// each one (to repaint).
pub fn start(db: PathBuf, window_days: i64, wake: Wake) -> std::io::Result<(SyncHandle, Receiver<Update>)> {
    let (tx, rx) = mpsc::channel();
    let (utx, urx) = mpsc::channel();
    let thread = std::thread::Builder::new().name("mailwright-sync".into()).spawn(move || {
        let store = match Store::open(&db) {
            Ok(s) => s,
            Err(e) => {
                let _ = utx.send(Update::Error(format!("Couldn't open the offline cache: {e}")));
                wake();
                return;
            }
        };
        let mut engine = Engine::new(store, utx, wake, window_days);
        engine.run(rx);
    })?;
    Ok((SyncHandle { tx, thread: Some(thread) }, urx))
}

enum Bg {
    Body(String),
}

pub struct Engine {
    store: Store,
    client: Option<Arc<dyn Mailbox>>,
    /// True for a plain IMAP/POP3 account, which syncs by reconciling each folder rather than by
    /// Graph delta queries.
    manual: bool,
    tx: Sender<Update>,
    wake: Wake,
    offline: bool,
    connected: bool,
    window_days: i64,
    selected: Option<String>,
    calendar: Option<(i64, i64)>,
    last_round: Option<Instant>,
    last_tree: Option<Instant>,
    bg: VecDeque<Bg>,
    me: String,
}

fn iso(t: i64) -> String {
    chrono::DateTime::from_timestamp(t, 0).unwrap_or_default().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// What to tell the user about an error.
/// Automatic replies need one permission the first sign-in may not have granted. A 403 almost
/// always means just that, so point the person at signing in again rather than at an error code.
pub fn describe_auto_reply(e: &GraphError) -> String {
    match e {
        GraphError::Api { status: 403, .. } | GraphError::SignInRequired(_) => {
            "Mailwright needs your permission to read and change automatic replies. Sign out of this account and sign in again, and approve the request.".into()
        }
        other => describe(other),
    }
}

pub fn describe(e: &GraphError) -> String {
    match e {
        GraphError::Network { host, .. } => format!("Can't reach {host}. Working offline."),
        GraphError::Api { status: 403, message, .. } => format!("Access denied: {message}"),
        GraphError::Api { status: 413, .. } => "That's too large for the server.".into(),
        GraphError::Api { message, code, .. } if !message.is_empty() => {
            let m = message.split(", ").next().unwrap_or(message);
            if code.is_empty() { m.to_string() } else { format!("{m} ({code})") }
        }
        other => other.to_string(),
    }
}

impl Engine {
    pub fn new(store: Store, tx: Sender<Update>, wake: Wake, window_days: i64) -> Engine {
        Engine {
            store,
            client: None,
            tx,
            wake,
            manual: false,
            offline: false,
            connected: false,
            window_days: window_days.clamp(7, 3650),
            selected: None,
            calendar: None,
            last_round: None,
            last_tree: None,
            bg: VecDeque::new(),
            me: String::new(),
        }
    }

    fn emit(&self, u: Update) {
        let _ = self.tx.send(u);
        (self.wake)();
    }

    fn status(&self, s: impl Into<String>) {
        self.emit(Update::Status(s.into()));
    }

    fn set_connected(&mut self, c: bool) {
        if self.connected != c {
            self.connected = c;
            self.emit(Update::Connected(c));
        }
    }

    fn online(&self) -> bool {
        !self.offline && self.client.is_some()
    }

    /// Note a failure: network trouble means offline, an expired sign-in means sign in again.
    /// Returns true when it's worth carrying on with other work.
    fn trouble(&mut self, e: &GraphError) -> bool {
        match e {
            GraphError::Network { .. } => {
                if self.connected {
                    self.status(describe(e));
                }
                self.set_connected(false);
                false
            }
            GraphError::SignInRequired(m) | GraphError::SignInCancelled(m) => {
                self.client = None;
                self.set_connected(false);
                self.emit(Update::SignInRequired(m.clone()));
                false
            }
            other => {
                log::warn!("{other}");
                true
            }
        }
    }

    pub fn run(&mut self, rx: Receiver<Command>) {
        let mut pending: VecDeque<Command> = VecDeque::new();
        loop {
            let cmd = if let Some(c) = pending.pop_front() {
                Some(c)
            } else if self.bg.is_empty() || !self.online() {
                let interval = if self.connected { 60 } else { 30 };
                let due = self.last_round.map(|t| Duration::from_secs(interval).saturating_sub(t.elapsed())).unwrap_or(Duration::from_secs(interval));
                match rx.recv_timeout(due) {
                    Ok(c) => Some(c),
                    Err(RecvTimeoutError::Timeout) => {
                        if self.online() {
                            self.round(false);
                        } else {
                            self.last_round = Some(Instant::now());
                        }
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            } else {
                let interval = if self.connected { 60 } else { 30 };
                if self.last_round.is_none_or(|t| t.elapsed() >= Duration::from_secs(interval)) {
                    self.round(false);
                    continue;
                }
                match rx.try_recv() {
                    Ok(c) => Some(c),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => return,
                }
            };
            match cmd {
                Some(Command::Shutdown) => return,
                Some(c) => {
                    // Collapse a burst of the same kind of request (scrolling through mail).
                    while let Ok(next) = rx.try_recv() {
                        pending.push_back(next);
                    }
                    self.handle(c);
                }
                None => self.background(),
            }
        }
    }

    fn handle(&mut self, c: Command) {
        match c {
            Command::Attach(mut client) => {
                self.offline = false;
                self.last_tree = None;
                let me = client.me();
                if let Ok(me) = &me {
                    let address = me.mail.clone().filter(|m| !m.is_empty()).or(me.user_principal_name.clone()).unwrap_or_default();
                    let _ = client.set_user(&address, me.display_name.as_deref().unwrap_or(""));
                }
                self.manual = client.is_manual();
                self.client = Some(Arc::from(client));
                match me {
                    Ok(me) => {
                        let address = me.mail.clone().filter(|m| !m.is_empty()).or(me.user_principal_name.clone()).unwrap_or_default();
                        self.me = address.clone();
                        let _ = self.store.set_meta("me", &address);
                        self.set_connected(true);
                        self.emit(Update::Account { name: me.display_name.unwrap_or_default(), address });
                    }
                    Err(e) => {
                        self.trouble(&e);
                    }
                }
                self.repair();
                self.round(true);
            }
            Command::SyncAll => {
                if self.client.is_none() {
                    self.emit(Update::SignInRequired("Sign in to send and receive.".into()));
                } else if self.offline {
                    self.status("Working offline. Turn off Work Offline to send and receive.");
                } else {
                    self.round(true);
                }
            }
            Command::SyncFolder(id) => {
                self.selected = Some(id.clone());
                if self.online() {
                    self.flush();
                    if self.manual { self.reconcile_folder(&id); } else { self.delta(&id); }
                }
            }
            Command::Open(id) => self.open(&id),
            Command::Do(action) => {
                let describe_it = action.describe();
                match self.store.apply(action) {
                    Ok(_) => {
                        self.emit(Update::MessagesChanged);
                        self.emit(Update::FoldersChanged);
                        self.emit(Update::CalendarChanged);
                        self.queued_count();
                        if self.online() {
                            self.flush();
                        }
                    }
                    Err(e) => self.emit(Update::Error(format!("{describe_it}: {e}"))),
                }
            }
            Command::CalendarWindow { from, to } => {
                self.calendar = Some((from, to));
                if self.online() {
                    self.refresh_calendar(false);
                }
            }
            Command::OpenEvent(id) => {
                if let (true, Some(c)) = (self.online(), self.client.clone()) {
                    match c.event_full(&id) {
                        Ok(e) => {
                            let cal = self.store.event(&id).ok().flatten().map(|r| r.calendar_id).unwrap_or_default();
                            let _ = self.store.save_event(&cal, &e);
                            self.emit(Update::CalendarChanged);
                        }
                        Err(e) => {
                            self.trouble(&e);
                        }
                    }
                }
            }
            Command::SaveAttachment { message_id, attachment_id, path, open } => self.save_attachment(&message_id, &attachment_id, path, open),
            Command::SearchServer(q) => {
                let Some(c) = self.client.clone().filter(|_| !self.offline) else {
                    self.emit(Update::SearchResults(q, Vec::new()));
                    return;
                };
                self.status(format!("Searching the server for \u{201c}{q}\u{201d}\u{2026}"));
                match c.search_messages(&q, 250) {
                    Ok(list) => {
                        let ids = list.iter().map(|m| m.id.clone()).collect();
                        let folders: std::collections::HashSet<String> = self.store.folders().unwrap_or_default().into_iter().map(|f| f.id).collect();
                        for m in &list {
                            // Only into a folder we know by that id (stored anywhere else, it couldn't be seen).
                            let Some(folder) = m.parent_folder_id.clone().filter(|f| folders.contains(f)) else { continue };
                            let _ = self.store.upsert_messages(&folder, std::slice::from_ref(m));
                        }
                        self.status(format!("{} results from the server", list.len()));
                        self.emit(Update::SearchResults(q, ids));
                    }
                    Err(e) => {
                        self.trouble(&e);
                        self.emit(Update::SearchResults(q, Vec::new()));
                    }
                }
            }
            Command::People(q) => {
                let mut found: Vec<Addr> = Vec::new();
                if let (true, Some(c)) = (self.online(), self.client.clone()) {
                    match c.people(&q) {
                        Ok(list) => {
                            for p in list {
                                for a in p.scored_email_addresses {
                                    if let Some(addr) = a.address.filter(|a| a.contains('@')) {
                                        found.push(Addr::new(p.display_name.as_deref().unwrap_or(""), &addr));
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            self.trouble(&e);
                        }
                    }
                }
                for a in self.store.known_addresses(&q, 10).unwrap_or_default() {
                    if !found.iter().any(|f| f.address.eq_ignore_ascii_case(&a.address)) {
                        found.push(a);
                    }
                }
                found.truncate(12);
                self.emit(Update::People(q, found));
            }
            Command::CreateFolder { parent, name } => self.folder_op(|c| c.create_folder(parent.as_deref(), &name).map(|_| ()), "Couldn't create the folder"),
            Command::RenameFolder { id, name } => self.folder_op(|c| c.rename_folder(&id, &name), "Couldn't rename the folder"),
            Command::DeleteFolder { id } => {
                // Deleting a folder moves it to Deleted Items, as Outlook does; from there it's gone.
                let deleted = self.store.folder_by_well_known("deleteditems").ok().flatten();
                let in_deleted = self.store.folders().unwrap_or_default().iter().any(|f| f.id == id && f.parent_id == deleted);
                self.folder_op(
                    |c| {
                        if in_deleted || deleted.is_none() {
                            c.delete_folder(&id)
                        } else {
                            move_folder(c, &id, deleted.as_deref().unwrap_or("deleteditems"))
                        }
                    },
                    "Couldn't delete the folder",
                )
            }
            Command::WorkOffline(on) => {
                self.offline = on;
                if on {
                    self.set_connected(false);
                    self.status("Working offline");
                } else if self.client.is_some() {
                    self.round(true);
                }
            }
            Command::SetWindow(days) => self.window_days = days.clamp(7, 3650),
            Command::GetAutoReply => {
                if let Some(c) = self.client.clone().filter(|_| !self.offline) {
                    match c.automatic_replies() {
                        Ok(s) => self.emit(Update::AutoReply(Box::new(s))),
                        Err(e) => {
                            log::warn!("reading automatic replies: {e}");
                            self.emit(Update::AutoReplyError(describe_auto_reply(&e)));
                        }
                    }
                } else {
                    self.emit(Update::AutoReplyError("You're offline, so automatic replies can't be checked.".into()));
                }
            }
            Command::SetAutoReply(setting) => {
                if let Some(c) = self.client.clone().filter(|_| !self.offline) {
                    match c.set_automatic_replies(&setting) {
                        Ok(()) => match c.automatic_replies() {
                            Ok(s) => self.emit(Update::AutoReply(Box::new(s))),
                            Err(_) => self.emit(Update::AutoReply(setting)),
                        },
                        Err(e) => {
                            log::warn!("saving automatic replies: {e}");
                            self.emit(Update::AutoReplyError(describe_auto_reply(&e)));
                        }
                    }
                } else {
                    self.emit(Update::AutoReplyError("You're offline, so automatic replies can't be changed.".into()));
                }
            }
            Command::SignOut => {
                if let Some(c) = self.client.take() {
                    let _ = c.sign_out();
                }
                let _ = self.store.clear();
                self.set_connected(false);
                self.emit(Update::SignedOut);
            }
            Command::Shutdown => {}
        }
    }

    fn folder_op(&mut self, f: impl FnOnce(&dyn Mailbox) -> Result<(), GraphError>, what: &str) {
        let Some(c) = self.client.clone().filter(|_| !self.offline) else {
            self.emit(Update::Error(format!("{what}: you're offline.")));
            return;
        };
        match f(c.as_ref()) {
            Ok(()) => {
                self.last_tree = None;
                self.refresh_tree();
            }
            Err(e) => {
                if self.trouble(&e) {
                    self.emit(Update::Error(format!("{what}: {}", describe(&e))));
                }
            }
        }
    }

    fn queued_count(&self) {
        self.emit(Update::Queued(self.store.queued_count().unwrap_or(0)));
    }

    /// One full round.
    fn round(&mut self, manual: bool) {
        self.last_round = Some(Instant::now());
        if !self.online() {
            return;
        }
        if manual {
            self.status("Sending and receiving\u{2026}");
        }
        let started = Instant::now();
        if !self.flush() && !self.online() {
            return;
        }
        let tree_due = self.last_tree.is_none_or(|t| t.elapsed() > Duration::from_secs(300)) || manual;
        if tree_due && !self.refresh_tree() {
            return;
        }
        let mut folders: Vec<String> = CORE_FOLDERS.iter().filter_map(|wk| self.store.folder_by_well_known(wk).ok().flatten()).collect();
        for f in self.store.synced_folders().unwrap_or_default() {
            if !folders.contains(&f) {
                folders.push(f);
            }
        }
        if let Some(s) = &self.selected
            && !folders.contains(s) {
                folders.push(s.clone());
            }
        for f in folders {
            let ok = if self.manual { self.reconcile_folder(&f) } else { self.delta(&f) };
            if !ok {
                log::info!("round stopped after {:.1}s", started.elapsed().as_secs_f32());
                return;
            }
        }
        if !self.manual {
            // The reconcile path already covers the inbox and there is no calendar for plain mailboxes.
            self.check_inbox();
            self.refresh_calendar(tree_due);
        }
        // Quiet-time work: bodies of the last fortnight's mail, for reading offline.
        let core: Vec<String> = ["inbox", "sentitems", "drafts"].iter().filter_map(|wk| self.store.folder_by_well_known(wk).ok().flatten()).collect();
        let since = chrono::Utc::now().timestamp() - 14 * 86_400;
        self.bg.clear();
        for id in self.store.missing_bodies(&core, since, 150).unwrap_or_default() {
            self.bg.push_back(Bg::Body(id));
        }
        log::info!("round done in {:.1}s", started.elapsed().as_secs_f32());
        if self.online() {
            let when = chrono::Local::now().format("%H:%M");
            self.status(format!("All folders are up to date. Last updated {when}."));
        }
    }

    fn background(&mut self) {
        if let Some(Bg::Body(id)) = self.bg.pop_front() {
            let Some(c) = self.client.clone() else { return };
            match c.message_full(&id) {
                Ok(m) => {
                    let _ = self.store.save_body(&m);
                }
                Err(e) if e.is_not_found() => {}
                Err(e) => {
                    if !self.trouble(&e) {
                        self.bg.clear();
                    }
                }
            }
        }
    }

    /// Folder tree and well-known ids. False when the round should stop.
    fn refresh_tree(&mut self) -> bool {
        let Some(c) = self.client.clone() else { return false };
        let mut well_known: Vec<(String, String)> = Vec::new();
        for wk in WELL_KNOWN {
            match c.well_known_folder(wk) {
                Ok(f) => well_known.push((wk.to_string(), f.id)),
                Err(e) if matches!(e, GraphError::Api { status: 404 | 400, .. }) => {}
                Err(e) => {
                    self.trouble(&e);
                    return false;
                }
            }
        }
        match c.all_folders() {
            Ok(list) => {
                self.set_connected(true);
                if let Err(e) = self.store.save_folders(&list, &well_known) {
                    self.emit(Update::Error(e.to_string()));
                }
                self.last_tree = Some(Instant::now());
                self.emit(Update::FoldersChanged);
                true
            }
            Err(e) => {
                self.trouble(&e);
                false
            }
        }
    }

    /// Bring one folder up to date. False when the round should stop.
    /// Bring one folder up to date for a plain IMAP/POP3 account: fetch its newest messages
    /// (cached ones get a flags-only refresh), announce anything new, and drop what the server no
    /// longer has in that window. False when the round should stop (offline/signed out).
    fn reconcile_folder(&mut self, folder: &str) -> bool {
        let Some(c) = self.client.clone() else { return false };
        let is_inbox = self.store.folder_by_well_known("inbox").ok().flatten().as_deref() == Some(folder);
        let known = self.store.folder_ids(folder).unwrap_or_default();
        let msgs = match c.fetch_folder(folder, &known, MAX_FOLDER_FILL) {
            Ok(m) => m,
            Err(e) => return self.trouble(&e),
        };
        // New, unread, recent messages to announce (only full ones are genuinely new).
        let lately = chrono::Utc::now().timestamp() - 6 * 3600;
        let arrived: Vec<NewMail> = msgs
            .iter()
            .filter(|m| is_inbox && !known.contains(&m.id))
            .filter(|m| m.is_read != Some(true) && m.is_draft != Some(true) && mailwright_store::is_whole(m))
            .filter(|m| m.received_date_time.as_deref().and_then(mailwright_store::parse_time).is_some_and(|t| t >= lately))
            .map(new_mail)
            .collect();
        let seen: std::collections::HashSet<String> = msgs.iter().map(|m| m.id.clone()).collect();
        if !self.save_listed(c.as_ref(), folder, &msgs) {
            self.emit(Update::Error("Couldn't save mail to the offline cache. Trying again shortly.".into()));
            return true;
        }
        // Deletions: anything cached within the fetched window that the server didn't return.
        if seen.len() >= MAX_FOLDER_FILL.min(250) {
            // The folder is larger than one fetch; don't prune what we didn't look at.
        } else {
            let since = self.store.oldest_received(&seen).unwrap_or(0);
            if since > 0 {
                match self.store.prune_unseen(folder, since, &seen) {
                    Ok(0) => {}
                    Ok(n) => log::info!("{folder}: {n} message(s) no longer on the server"),
                    Err(e) => log::error!("tidying {folder}: {e}"),
                }
            }
        }
        if !arrived.is_empty() {
            self.emit(Update::NewMail(arrived));
        }
        self.set_connected(true);
        self.emit(Update::MessagesChanged);
        self.emit(Update::FoldersChanged);
        true
    }

    fn delta(&mut self, folder: &str) -> bool {
        let Some(c) = self.client.clone() else { return false };
        let saved = self.store.delta_link(folder).ok().flatten();
        let first = saved.is_none();
        // First time a folder is synced, fill it newest-first ignoring the offline window, so an
        // archive folder shows its mail as Outlook does rather than just the last few months.
        if first && !self.fill_folder(c.as_ref(), folder) {
            return false;
        }
        let since = iso(chrono::Utc::now().timestamp() - self.window_days * 86_400);
        let mut link = saved.clone();
        let mut changed = 0usize;
        let mut pages = 0;
        let is_inbox = self.store.folder_by_well_known("inbox").ok().flatten().as_deref() == Some(folder);
        let mut arrived: Vec<NewMail> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        loop {
            let page = match c.messages_delta(folder, link.as_deref(), if link.is_none() { Some(&since) } else { None }) {
                Ok(p) => p,
                Err(GraphError::Api { status: 410, .. }) | Err(GraphError::Api { status: 400, .. }) if saved.is_some() && pages == 0 => {
                    // The sync state expired: start the folder again.
                    let _ = self.store.set_delta_link(folder, None);
                    return self.delta(folder);
                }
                Err(e) => {
                    let carry_on = self.trouble(&e);
                    if carry_on && e.is_not_found() {
                        // The folder went away; the next tree refresh drops it.
                        self.last_tree = None;
                    }
                    return carry_on;
                }
            };
            pages += 1;
            self.set_connected(true);
            changed += page.changed.len() + page.removed.len();
            let fresh: std::collections::HashSet<String> = if is_inbox && !first {
                let ids: Vec<String> = page.changed.iter().map(|m| m.id.clone()).collect();
                let known = self.store.known_ids(&ids).unwrap_or_default();
                ids.into_iter().filter(|i| !known.contains(i)).collect()
            } else {
                Default::default()
            };
            let mut page = page;
            self.confirm_removals(c.as_ref(), folder, &mut page.changed, &mut page.removed);
            let partial = page.changed.iter().filter(|m| !mailwright_store::is_whole(m)).count();
            let elsewhere = page.changed.iter().filter(|m| m.parent_folder_id.as_deref().is_some_and(|p| !p.is_empty() && p != folder)).count();
            if partial > 0 || elsewhere > 0 {
                log::info!("{folder}: {partial} partial update(s), {elsewhere} naming another folder id");
            }
            seen.extend(page.changed.iter().map(|m| m.id.clone()));
            let saved_ok = (page.changed.is_empty() || self.save_listed(c.as_ref(), folder, &page.changed))
                && (page.removed.is_empty() || self.store.remove_messages(folder, &page.removed).map_err(|e| log::error!("removing mail in {folder}: {e}")).is_ok());
            if !saved_ok {
                // Keep the old sync point, so the next round asks for these again.
                self.emit(Update::Error("Couldn't save new mail to the offline cache. Trying again shortly.".into()));
                return true;
            }
            for m in page.changed.iter().filter(|m| fresh.contains(&m.id) && m.is_read != Some(true) && m.is_draft != Some(true)) {
                arrived.push(new_mail(m));
            }
            if first && (page.next_link.is_some() || pages > 1) {
                // A big folder fills in page by page.
                self.emit(Update::MessagesChanged);
            }
            if let Some(d) = page.delta_link {
                if first {
                    // A full listing: anything stored here that the server didn't list has gone.
                    let since = chrono::Utc::now().timestamp() - self.window_days * 86_400;
                    match self.store.prune_unseen(folder, since, &seen) {
                        Ok(0) => {}
                        Ok(n) => log::info!("{folder}: {n} message(s) no longer on the server"),
                        Err(e) => log::error!("tidying {folder}: {e}"),
                    }
                }
                if let Err(e) = self.store.set_delta_link(folder, Some(&d)) {
                    log::error!("saving the sync point for {folder}: {e}");
                }
                break;
            }
            match page.next_link {
                Some(n) => link = Some(n),
                None => break,
            }
        }
        if changed > 0 {
            log::info!("{folder}: {changed} changes, {} new", arrived.len());
        }
        if !arrived.is_empty() {
            self.emit(Update::NewMail(arrived));
        }
        if changed > 0 || first {
            if let Some(c) = self.client.clone()
                && let Ok(f) = c.well_known_folder(folder) {
                    let _ = self.store.set_counts(folder, f.unread_item_count, f.total_item_count);
                }
            self.emit(Update::MessagesChanged);
            self.emit(Update::FoldersChanged);
        }
        true
    }

    /// Fill a folder with its newest messages, ignoring the offline window, up to a generous cap.
    /// False only when the connection failed (so the round should stop).
    fn fill_folder(&mut self, c: &dyn Mailbox, folder: &str) -> bool {
        match c.folder_messages_newest(folder, MAX_FOLDER_FILL) {
            Ok(list) => {
                if !list.is_empty() {
                    log::info!("{folder}: initial fill of {} message(s)", list.len());
                    if let Err(e) = self.store.upsert_messages(folder, &list) {
                        log::error!("saving mail in {folder}: {e}");
                    } else {
                        self.emit(Update::MessagesChanged);
                    }
                }
                true
            }
            Err(e) => self.trouble(&e),
        }
    }

    /// Save messages listed in `folder`; partial ones not stored yet are fetched whole first.
    /// False when saving failed.
    fn save_listed(&mut self, c: &dyn Mailbox, folder: &str, msgs: &[mailwright_graph::types::Message]) -> bool {
        let skipped = match self.store.upsert_messages(folder, msgs) {
            Ok(s) => s,
            Err(e) => {
                log::error!("saving mail in {folder}: {e}");
                return false;
            }
        };
        let mut whole = Vec::new();
        for id in skipped.iter().take(100) {
            match c.message_brief(id) {
                Ok(m) => whole.push(m),
                Err(e) if e.is_not_found() => {}
                Err(e) => log::warn!("fetching {}: {e}", mailwright_graph::client::short_id(id)),
            }
        }
        whole.is_empty() || self.store.upsert_messages(folder, &whole).map_err(|e| log::error!("saving mail in {folder}: {e}")).is_ok()
    }

    /// Mail an earlier version stored where it can't be seen: take it out and sync again from
    /// scratch, which brings it back properly.
    fn repair(&mut self) {
        match self.store.remove_damaged() {
            Ok(0) => {}
            Ok(n) => {
                log::warn!("repair: {n} message(s) were stored where they couldn't be seen; syncing all folders again");
                for f in self.store.folders().unwrap_or_default() {
                    let _ = self.store.set_delta_link(&f.id, None);
                }
                self.emit(Update::MessagesChanged);
            }
            Err(e) => log::error!("repair: {e}"),
        }
    }

    /// Before taking a recent message out of the cache because delta said so, ask the server
    /// whether it's really gone. If it's still in the folder, keep it (and say so in the log).
    fn confirm_removals(&mut self, c: &dyn Mailbox, folder: &str, changed: &mut Vec<mailwright_graph::types::Message>, removed: &mut Vec<String>) {
        if removed.is_empty() {
            return;
        }
        let recent = chrono::Utc::now().timestamp() - 7 * 86_400;
        let mut checks = 0;
        removed.retain(|id| {
            let Some(row) = self.store.message(id).ok().flatten() else { return true };
            if row.folder_id != folder || row.received < recent || checks >= 20 {
                return true;
            }
            checks += 1;
            match c.message_brief(id) {
                Ok(m) if m.parent_folder_id.as_deref().is_none_or(|p| p == folder) => {
                    log::warn!("delta said {} left {folder}, but it's still there: kept", mailwright_graph::client::short_id(id));
                    changed.push(m);
                    false
                }
                Ok(_) => true,
                Err(e) if e.is_not_found() => true,
                Err(e) => {
                    log::warn!("checking {} before removing it: {e}; kept for now", mailwright_graph::client::short_id(id));
                    false
                }
            }
        });
    }

    /// A check on the Inbox that doesn't rely on delta: the newest messages, straight from the
    /// folder. Anything delta missed is added (and announced, if it's new and unread).
    fn check_inbox(&mut self) {
        let Some(c) = self.client.clone() else { return };
        let Some(inbox) = self.store.folder_by_well_known("inbox").ok().flatten() else { return };
        if self.store.delta_link(&inbox).ok().flatten().is_none() {
            return;
        }
        let list = match c.newest_messages(&inbox, 25) {
            Ok(l) => l,
            Err(e) => {
                self.trouble(&e);
                return;
            }
        };
        let ids: Vec<String> = list.iter().map(|m| m.id.clone()).collect();
        let known = self.store.known_ids(&ids).unwrap_or_default();
        let wrong: std::collections::HashSet<String> = self.store.differing(&inbox, &list).unwrap_or_default().into_iter().collect();
        let fix: Vec<_> = list.into_iter().filter(|m| wrong.contains(&m.id)).collect();
        if fix.is_empty() {
            return;
        }
        let new_count = fix.iter().filter(|m| !known.contains(&m.id)).count();
        log::warn!(
            "Inbox check: {} message(s) missing and {} stored wrongly, put right: {}",
            new_count,
            fix.len() - new_count,
            fix.iter().map(|m| mailwright_graph::client::short_id(&m.id)).collect::<Vec<_>>().join(", ")
        );
        if let Err(e) = self.store.upsert_messages(&inbox, &fix) {
            log::error!("saving mail in the Inbox: {e}");
            return;
        }
        let missing: Vec<_> = fix.into_iter().filter(|m| !known.contains(&m.id)).collect();
        let lately = chrono::Utc::now().timestamp() - 6 * 3600;
        let arrived: Vec<NewMail> = missing
            .iter()
            .filter(|m| m.is_read != Some(true) && m.is_draft != Some(true))
            .filter(|m| m.received_date_time.as_deref().and_then(mailwright_store::parse_time).is_some_and(|t| t >= lately))
            .map(new_mail)
            .collect();
        if !arrived.is_empty() {
            self.emit(Update::NewMail(arrived));
        }
        self.emit(Update::MessagesChanged);
        self.emit(Update::FoldersChanged);
    }

    fn refresh_calendar(&mut self, list_calendars: bool) {
        let Some(c) = self.client.clone() else { return };
        let mut calendars = self.store.calendars().unwrap_or_default();
        if list_calendars || calendars.is_empty() {
            match c.calendars() {
                Ok(list) => {
                    let _ = self.store.save_calendars(&list);
                    calendars = self.store.calendars().unwrap_or_default();
                }
                Err(e) => {
                    self.trouble(&e);
                    return;
                }
            }
        }
        let now = chrono::Utc::now().timestamp();
        // Around what's on screen, or the coming month for reminders and the To-Do bar.
        let (from, to) = self.calendar.unwrap_or((now - 7 * 86_400, now + 35 * 86_400));
        let (from, to) = (from - 86_400, to + 86_400);
        for cal in calendars {
            match c.calendar_view(&cal.id, &iso(from), &iso(to)) {
                Ok(events) => {
                    let _ = self.store.save_calendar_window(&cal.id, from, to, &events);
                }
                Err(e) => {
                    if !self.trouble(&e) {
                        return;
                    }
                }
            }
        }
        self.emit(Update::CalendarChanged);
    }

    fn open(&mut self, id: &str) {
        let Some(row) = self.store.message(id).ok().flatten() else {
            self.emit(Update::MessageReady(id.to_string()));
            return;
        };
        if id.starts_with("local:") || !self.online() {
            self.emit(Update::MessageReady(id.to_string()));
            return;
        }
        let Some(c) = self.client.clone() else { return };
        if row.body.is_none() {
            match c.message_full(id) {
                Ok(m) => {
                    let _ = self.store.save_body(&m);
                }
                Err(e) if e.is_not_found() => {
                    let _ = self.store.remove_messages(&row.folder_id, &[id.to_string()]);
                    self.emit(Update::MessagesChanged);
                    return;
                }
                Err(e) => {
                    self.trouble(&e);
                    self.emit(Update::MessageReady(id.to_string()));
                    return;
                }
            }
        }
        let body = self.store.message(id).ok().flatten().and_then(|m| m.body).unwrap_or_default();
        let needs_list = row.has_attachments || body.to_ascii_lowercase().contains("cid:");
        if needs_list && !self.store.has_attachment_list(id).unwrap_or(false) {
            match c.attachments(id) {
                Ok(list) => {
                    let _ = self.store.save_attachments(id, &list);
                }
                Err(e) => {
                    self.trouble(&e);
                }
            }
        }
        // Inline images are part of reading the message: fetch them now.
        for att in self.store.missing_inline(id).unwrap_or_default() {
            match c.attachment(id, &att) {
                Ok(a) => {
                    let bytes = a.content_bytes.as_deref().and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok()).unwrap_or_default();
                    let _ = self.store.save_attachment_data(id, &att, &bytes, a.content_id.as_deref());
                }
                Err(e) => {
                    if !self.trouble(&e) {
                        break;
                    }
                }
            }
        }
        if row.is_meeting && row.event_id.is_none()
            && let Ok((kind, Some(event))) = c.meeting_event(id) {
                let cal = self.store.default_calendar().ok().flatten().unwrap_or_default();
                let _ = self.store.save_event(&cal, &event);
                let _ = self.store.set_meeting(id, kind.as_deref(), Some(&event.id));
            }
        self.emit(Update::MessageReady(id.to_string()));
    }

    fn save_attachment(&mut self, message_id: &str, attachment_id: &str, path: PathBuf, open: bool) {
        let cached = self.store.attachment_data(message_id, attachment_id).ok().flatten();
        let data = match cached {
            Some(d) => d,
            None => {
                let Some(c) = self.client.clone().filter(|_| !self.offline) else {
                    self.emit(Update::Error("That attachment isn't available offline.".into()));
                    return;
                };
                self.status("Downloading the attachment\u{2026}");
                match c.attachment_bytes(message_id, attachment_id) {
                    Ok(d) => d,
                    Err(e) => {
                        if self.trouble(&e) {
                            self.emit(Update::Error(format!("Couldn't download the attachment: {}", describe(&e))));
                        }
                        return;
                    }
                }
            }
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match std::fs::write(&path, &data) {
            Ok(()) => {
                self.status(format!("Saved {}", path.display()));
                self.emit(Update::AttachmentSaved { path, open });
            }
            Err(e) => self.emit(Update::Error(format!("Couldn't save {}: {e}", path.display()))),
        }
    }

    /// Send queued changes. False when the round should stop (offline, signed out).
    fn flush(&mut self) -> bool {
        if self.store.queued_count().unwrap_or(0) == 0 {
            return true;
        }
        let mut touched_mail = false;
        let mut touched_calendar = false;
        // One at a time from the head of the queue: a move can rename a message that later
        // changes refer to.
        let mut guard = 0;
        while let Some(q) = self.store.queued().unwrap_or_default().into_iter().next() {
            guard += 1;
            if guard > 10_000 {
                break;
            }
            let Some(c) = self.client.clone() else { return false };
            let result = perform(c.as_ref(), &self.store, &q);
            match result {
                Ok(outcome) => {
                    self.set_connected(true);
                    if let Some((old, new)) = &outcome.renamed {
                        let _ = self.store.rename_message(old, new);
                    }
                    let _ = self.store.done(&q);
                    match &q.action {
                        Action::Send { compose } => self.emit(Update::Sent(compose.subject.clone())),
                        Action::SaveDraft { compose } => {
                            if let Some(d) = outcome.draft_id.clone() {
                                self.emit(Update::DraftSaved { local_id: compose.local_id.clone(), draft_id: d });
                            }
                        }
                        Action::CreateEvent { calendar_id, .. } => {
                            if let Some(e) = outcome.event {
                                let _ = self.store.save_event(calendar_id, &e);
                            }
                        }
                        Action::UpdateEvent { id, .. } => {
                            if let Some(e) = outcome.event {
                                let cal = self.store.event(id).ok().flatten().map(|r| r.calendar_id).unwrap_or_default();
                                let _ = self.store.save_event(&cal, &e);
                            }
                        }
                        _ => {}
                    }
                    touched_mail |= q.action.message_id().is_some() || matches!(q.action, Action::Send { .. } | Action::SaveDraft { .. } | Action::RespondEvent { .. });
                    touched_calendar |= matches!(q.action, Action::CreateEvent { .. } | Action::UpdateEvent { .. } | Action::DeleteEvent { .. } | Action::CancelEvent { .. } | Action::RespondEvent { .. });
                }
                Err(e) if matches!(e, GraphError::Network { .. } | GraphError::SignInRequired(_)) => {
                    let _ = self.store.failed(&q, &e.to_string());
                    self.trouble(&e);
                    self.queued_count();
                    return false;
                }
                Err(e) if e.is_transient() && q.attempts + 1 < MAX_ATTEMPTS => {
                    // Busy or throttled: leave it (and what follows it) for the next round, and carry on receiving.
                    log::warn!("{} will be retried: {e}", q.action.describe());
                    let _ = self.store.failed(&q, &e.to_string());
                    break;
                }
                Err(e) if e.is_not_found() && q.action.message_id().is_some() => {
                    // The message is already gone on the server; nothing to do.
                    let _ = self.store.done(&q);
                    touched_mail = true;
                }
                Err(e) => {
                    let _ = self.store.discard(&q);
                    let what = q.action.describe();
                    let extra = if matches!(q.action, Action::Send { .. }) { " The message is still in your Outbox." } else { "" };
                    self.emit(Update::Error(format!("{what} didn't work: {}.{extra}", describe(&e).trim_end_matches('.'))));
                    touched_mail = true;
                }
            }
        }
        self.queued_count();
        if touched_mail {
            self.emit(Update::MessagesChanged);
            self.emit(Update::FoldersChanged);
        }
        if touched_calendar {
            self.emit(Update::CalendarChanged);
        }
        true
    }
}

fn new_mail(m: &mailwright_graph::types::Message) -> NewMail {
    let from = m.from.as_ref().or(m.sender.as_ref()).map(Addr::from_graph).unwrap_or_default();
    NewMail {
        id: m.id.clone(),
        from: from.label().to_string(),
        subject: m.subject.clone().unwrap_or_default(),
        preview: m.body_preview.clone().unwrap_or_default(),
        focused: m.inference_classification.as_deref() != Some("other"),
    }
}

fn move_folder(c: &dyn Mailbox, id: &str, dest: &str) -> Result<(), GraphError> {
    c.move_folder(id, dest)
}

#[derive(Default)]
struct Outcome {
    renamed: Option<(String, String)>,
    draft_id: Option<String>,
    event: Option<mailwright_graph::types::Event>,
}

pub(crate) fn compose_patch(compose: &Compose, include_body: bool) -> serde_json::Value {
    let mut v = json!({
        "subject": compose.subject,
        "toRecipients": compose.to.iter().map(Addr::to_graph).collect::<Vec<_>>(),
        "ccRecipients": compose.cc.iter().map(Addr::to_graph).collect::<Vec<_>>(),
        "bccRecipients": compose.bcc.iter().map(Addr::to_graph).collect::<Vec<_>>(),
        "importance": if compose.importance.is_empty() { "normal" } else { compose.importance.as_str() },
        "isReadReceiptRequested": compose.request_read_receipt,
    });
    if include_body {
        v["body"] = json!({"contentType": "html", "content": compose.html});
    }
    v
}

/// Do one queued change on the server.
fn perform(c: &dyn Mailbox, store: &Store, q: &QueuedAction) -> Result<Outcome, GraphError> {
    let local = |id: &str| id.starts_with("local:");
    match &q.action {
        Action::SetRead { id, .. } | Action::SetFlag { id, .. } | Action::SetCategories { id, .. } | Action::Delete { id } | Action::Move { id, .. } if local(id) => Ok(Outcome::default()),
        Action::SetRead { id, read } => c.set_read(id, *read).map(|_| Outcome::default()),
        Action::SetFlag { id, status } => c.set_flag(id, status).map(|_| Outcome::default()),
        Action::SetCategories { id, categories } => c.set_categories(id, categories).map(|_| Outcome::default()),
        Action::Move { id, dest } => {
            let m = c.move_message(id, dest)?;
            Ok(Outcome { renamed: (!m.id.is_empty() && m.id != *id).then(|| (id.clone(), m.id)), ..Default::default() })
        }
        Action::Delete { id } => c.delete_message(id).map(|_| Outcome::default()),
        Action::Send { compose } | Action::SaveDraft { compose } => {
            let send = matches!(q.action, Action::Send { .. });
            let out = c.submit(compose, send, store, q.seq)?;
            Ok(Outcome { draft_id: out.draft_id, ..Default::default() })
        }
        Action::CreateEvent { calendar_id, body, .. } => Ok(Outcome { event: Some(c.create_event(calendar_id, body)?), ..Default::default() }),
        Action::UpdateEvent { id, patch } => {
            if local(id) {
                return Ok(Outcome::default());
            }
            Ok(Outcome { event: Some(c.update_event(id, patch)?), ..Default::default() })
        }
        Action::DeleteEvent { id } => {
            if local(id) {
                return Ok(Outcome::default());
            }
            match c.delete_event(id) {
                Err(e) if e.is_not_found() => Ok(Outcome::default()),
                r => r.map(|_| Outcome::default()),
            }
        }
        Action::CancelEvent { id, comment } => c.cancel_event(id, comment).map(|_| Outcome::default()),
        Action::RespondEvent { id, response, comment, send } => {
            let r = match response.as_str() {
                "accept" => EventResponse::Accept,
                "tentative" => EventResponse::Tentative,
                _ => EventResponse::Decline,
            };
            c.respond_event(id, r, comment, *send).map(|_| Outcome::default())
        }
    }
}
