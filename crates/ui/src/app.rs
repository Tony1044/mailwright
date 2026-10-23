//! The application: state, the session with a mailbox, and the frame's layout.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use egui::{Context, Ui};
use mailcraft_graph::auth::DeviceCode;
use mailcraft_graph::tokens::{MemoryTokenStore, SystemTokenStore, TokenStore};
use mailcraft_graph::{AccountConfig, GraphClient, GraphError};
use mailcraft_store::{Action, Addr, AttachmentRow, EventRow, FolderRow, ListFilter, MessageRow, SortBy, Store};
use mailcraft_sync::{Command, SyncHandle, Update};

use crate::calendar::CalState;
use crate::compose::ComposeWin;
use crate::event::EventEditor;
use crate::settings::{ReadingPane, Settings, data_dir};
use crate::theme::{self, Tokens};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Mail,
    Calendar,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backstage {
    Info,
    Options,
    About,
}

/// Messages from helper threads.
pub enum AppMsg {
    SignedIn(Result<(Box<GraphClient>, String, String), GraphError>),
    Resumed(Result<Box<GraphClient>, GraphError>),
    DeviceCode(DeviceCode),
    RemoteImage { url: String, bytes: Option<Vec<u8>> },
}

pub struct Session {
    pub account: AccountConfig,
    pub demo: bool,
    pub db_path: PathBuf,
    pub store: Store,
    pub sync: SyncHandle,
    pub updates: Receiver<Update>,
    pub name: String,
    pub address: String,
    pub connected: bool,
    pub queued: i64,
    pub needs_sign_in: Option<String>,
    pub resuming: bool,
}

/// The message in the reading pane.
pub struct Reading {
    pub row: MessageRow,
    pub doc: Option<mailcraft_html::Document>,
    pub attachments: Vec<AttachmentRow>,
    pub event: Option<EventRow>,
    pub allow_remote: bool,
    pub requested: bool,
    pub opened_at: Instant,
}

#[derive(Default)]
pub struct MailState {
    pub folders: Vec<FolderRow>,
    pub folder: Option<String>,
    pub list: Vec<MessageRow>,
    pub list_dirty: bool,
    pub selected: Vec<String>,
    pub anchor: Option<String>,
    pub filter: ListFilter,
    /// Focused Inbox: showing Other rather than Focused.
    pub other_tab: bool,
    pub sort: SortBy,
    pub ascending: bool,
    pub search: String,
    pub searching: Option<String>,
    pub search_all: bool,
    pub server_results: Option<Vec<String>>,
    pub reading: Option<Reading>,
    pub expanded: HashSet<String>,
    pub scroll_to_selected: bool,
    pub list_has_focus: bool,
    pub renaming: Option<(String, String)>,
    pub new_folder: Option<(Option<String>, String)>,
    pub confirm: Option<Confirm>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Confirm {
    DeleteFolder(String, String),
    EmptyFolder(String, String),
    DeleteForever(Vec<String>),
    SignOut,
}

#[derive(Default)]
pub struct SignInState {
    pub client_id: String,
    pub tenant: String,
    pub busy: bool,
    pub cancel: Option<Arc<AtomicBool>>,
    pub device: Option<DeviceCode>,
    pub error: Option<String>,
    pub show_help: bool,
}

pub struct MailApp {
    pub settings: Settings,
    pub settings_path: Option<PathBuf>,
    pub data_dir: PathBuf,
    pub ctx_slot: Arc<Mutex<Option<Context>>>,
    pub session: Option<Session>,
    pub tx: Sender<AppMsg>,
    rx: Receiver<AppMsg>,
    pub signin: SignInState,
    pub view: View,
    pub tab: usize,
    pub backstage: Option<Backstage>,
    pub mail: MailState,
    pub cal: CalState,
    pub composes: Vec<ComposeWin>,
    pub editors: Vec<EventEditor>,
    pub status: String,
    pub errors: Vec<String>,
    pub demo: Option<mockgraph::MockGraph>,
    pub textures: HashMap<String, egui::TextureHandle>,
    pub remote: HashMap<String, Option<egui::TextureHandle>>,
    pub people: (String, Vec<Addr>),
    /// Message windows: (window id, message id).
    pub readers: Vec<(u64, String)>,
    pub next_id: u64,
    pub quit: bool,
    style_ready: bool,
    fonts_frames: u8,
    pub options_draft: Option<Settings>,
}

fn safe_name(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '@' || c == '-' { c } else { '_' }).collect()
}

impl MailApp {
    /// `settings_path`: where settings live (None: don't save, as in tests).
    pub fn new(settings: Settings, settings_path: Option<PathBuf>, data: Option<PathBuf>) -> MailApp {
        let (tx, rx) = channel();
        let mut mail = MailState { list_dirty: true, ..Default::default() };
        mail.expanded = settings.expanded.iter().cloned().collect();
        let signin = SignInState { client_id: settings.last_client_id.clone(), tenant: settings.last_tenant.clone(), ..Default::default() };
        let cal = CalState::new(settings.calendar_view);
        MailApp {
            settings,
            settings_path,
            data_dir: data.unwrap_or_else(data_dir),
            ctx_slot: Arc::new(Mutex::new(None)),
            session: None,
            tx,
            rx,
            signin,
            view: View::Mail,
            tab: 1,
            backstage: None,
            mail,
            cal,
            composes: Vec::new(),
            editors: Vec::new(),
            status: String::new(),
            errors: Vec::new(),
            demo: None,
            textures: HashMap::new(),
            remote: HashMap::new(),
            people: (String::new(), Vec::new()),
            readers: Vec::new(),
            next_id: 1,
            quit: false,
            style_ready: false,
            fonts_frames: 0,
            options_draft: None,
        }
    }

    /// Pick up where we left off: the saved account, or the sample mailbox.
    pub fn resume(&mut self) {
        if let Some(acc) = self.settings.account.clone().filter(|a| !a.username.is_empty()) {
            self.start_session(acc, false, None);
        }
    }

    pub fn save_settings(&mut self) {
        self.settings.expanded = self.mail.expanded.iter().cloned().collect();
        self.settings.calendar_view = self.cal.view;
        if let Some(p) = &self.settings_path {
            self.settings.save(p);
        }
    }

    pub fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn wake(&self) -> mailcraft_sync::Wake {
        let slot = self.ctx_slot.clone();
        Arc::new(move || {
            if let Ok(g) = slot.lock() {
                if let Some(ctx) = g.as_ref() {
                    ctx.request_repaint();
                }
            }
        })
    }

    fn token_store(&self, demo: bool) -> Arc<dyn TokenStore> {
        if demo { Arc::new(MemoryTokenStore::default()) } else { Arc::new(SystemTokenStore::new(self.data_dir.clone())) }
    }

    /// Open the cache and start syncing for `account`. With `client`, already signed in;
    /// otherwise the saved sign-in is resumed in the background.
    pub fn start_session(&mut self, account: AccountConfig, demo: bool, client: Option<Box<GraphClient>>) {
        self.session = None;
        let db_path = if demo { self.data_dir.join("sample-mailbox.sqlite") } else { self.data_dir.join(format!("mail-{}.sqlite", safe_name(&account.username))) };
        let _ = std::fs::create_dir_all(&self.data_dir);
        let (sync, updates) = match mailcraft_sync::start(db_path.clone(), self.settings.offline_days, self.wake()) {
            Ok(x) => x,
            Err(e) => {
                self.errors.push(format!("Couldn't start syncing: {e}"));
                return;
            }
        };
        let store = match Store::open(&db_path) {
            Ok(s) => s,
            Err(e) => {
                self.errors.push(format!("Couldn't open the offline cache: {e}"));
                return;
            }
        };
        if self.settings.work_offline && !demo {
            sync.send(Command::WorkOffline(true));
        }
        let resuming = client.is_none();
        let name = account.display_name.clone();
        let address = account.username.clone();
        if let Some(c) = client {
            sync.send(Command::Attach(c));
        } else {
            let tx = self.tx.clone();
            let store_t = self.token_store(demo);
            let acc = account.clone();
            let wake = self.wake();
            std::thread::spawn(move || {
                let r = GraphClient::from_saved(acc, store_t).map(Box::new);
                let _ = tx.send(AppMsg::Resumed(r));
                wake();
            });
        }
        self.session = Some(Session {
            account,
            demo,
            db_path,
            store,
            sync,
            updates,
            name,
            address,
            connected: false,
            queued: 0,
            needs_sign_in: None,
            resuming,
        });
        self.mail = MailState { list_dirty: true, expanded: self.settings.expanded.iter().cloned().collect(), ..Default::default() };
        self.cal = CalState::new(self.settings.calendar_view);
        self.reload_folders();
        self.status = if resuming { "Connecting\u{2026}".into() } else { "Updating\u{2026}".into() };
    }

    /// The sample mailbox: a stand-in server in this process.
    pub fn start_demo(&mut self) {
        let mock = match mockgraph::MockGraph::start() {
            Ok(m) => m,
            Err(e) => {
                self.signin.error = Some(format!("Couldn't start the sample mailbox: {e}"));
                return;
            }
        };
        let account = AccountConfig {
            client_id: "00000000-0000-0000-0000-00000000c0de".into(),
            tenant: "organizations".into(),
            username: mockgraph::ME_ADDRESS.into(),
            display_name: mockgraph::ME_NAME.into(),
            login_base: Some(mock.login_base()),
            graph_base: Some(mock.graph_base()),
        };
        let store = self.token_store(true);
        let _ = store.save(&account.username, &mock.grant_refresh_token());
        // A fresh sample each time.
        let db = self.data_dir.join("sample-mailbox.sqlite");
        for ext in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", db.display()));
        }
        match GraphClient::from_saved(account.clone(), store) {
            Ok(c) => {
                self.demo = Some(mock);
                self.start_session(account, true, Some(Box::new(c)));
            }
            Err(e) => self.signin.error = Some(e.to_string()),
        }
    }

    /// Sign in with the system browser (or the device code flow).
    pub fn begin_sign_in(&mut self, device_code: bool) {
        let mut cfg = AccountConfig { client_id: self.signin.client_id.trim().to_string(), tenant: self.signin.tenant.trim().to_string(), ..Default::default() };
        if let Some(old) = self.session.as_ref().map(|s| s.account.clone()).or(self.settings.account.clone()) {
            if old.client_id == cfg.client_id {
                cfg.username = old.username;
            }
        }
        if let Err(e) = cfg.validate() {
            self.signin.error = Some(e);
            return;
        }
        self.settings.last_client_id = cfg.client_id.clone();
        self.settings.last_tenant = cfg.tenant.clone();
        self.save_settings();
        self.signin.error = None;
        self.signin.busy = true;
        self.signin.device = None;
        let cancel = Arc::new(AtomicBool::new(false));
        self.signin.cancel = Some(cancel.clone());
        let tx = self.tx.clone();
        let wake = self.wake();
        let store = self.token_store(false);
        std::thread::spawn(move || {
            let tokens = if device_code {
                match mailcraft_graph::auth::start_device_code(&cfg) {
                    Ok(dc) => {
                        let _ = tx.send(AppMsg::DeviceCode(dc.clone()));
                        wake();
                        mailcraft_graph::auth::finish_device_code(&cfg, &dc, cancel)
                    }
                    Err(e) => Err(e),
                }
            } else {
                mailcraft_graph::auth::sign_in_with_browser(&cfg, &|url| open::that_detached(url).is_ok(), cancel)
            };
            let result = tokens.and_then(|t| {
                let mut client = GraphClient::new(cfg, t, store);
                let me = client.me()?;
                let address = me.mail.clone().filter(|m| !m.is_empty()).or(me.user_principal_name.clone()).unwrap_or_default();
                let name = me.display_name.clone().unwrap_or_default();
                client.set_user(&address, &name)?;
                Ok((Box::new(client), name, address))
            });
            let _ = tx.send(AppMsg::SignedIn(result));
            wake();
        });
    }

    pub fn cancel_sign_in(&mut self) {
        if let Some(c) = self.signin.cancel.take() {
            c.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.signin.busy = false;
        self.signin.device = None;
    }

    pub fn sign_out(&mut self) {
        if let Some(s) = &self.session {
            s.sync.send(Command::SignOut);
        }
        self.session = None;
        self.demo = None;
        self.settings.account = None;
        self.mail = MailState::default();
        self.composes.clear();
        self.editors.clear();
        self.backstage = None;
        self.save_settings();
    }

    pub fn send(&self, c: Command) {
        if let Some(s) = &self.session {
            s.sync.send(c);
        }
    }

    pub fn act(&mut self, a: Action) {
        self.send(Command::Do(a));
    }

    // ---- per-frame work ----------------------------------------------------------------------

    /// Everything that isn't drawing: helper threads, sync updates, timers, shortcuts.
    pub fn logic(&mut self, ctx: &Context) {
        if let Ok(mut g) = self.ctx_slot.lock() {
            if g.is_none() {
                *g = Some(ctx.clone());
            }
        }
        if !self.style_ready {
            theme::apply(ctx, self.settings.appearance);
            self.style_ready = true;
        }
        if self.fonts_frames < 2 {
            // Fonts set this frame are bound from the next one.
            self.fonts_frames += 1;
            ctx.request_repaint();
        }
        while let Ok(m) = self.rx.try_recv() {
            self.on_msg(ctx, m);
        }
        let updates: Vec<Update> = self.session.as_ref().map(|s| s.updates.try_iter().collect()).unwrap_or_default();
        for u in updates {
            self.on_update(ctx, u);
        }
        if self.mail.list_dirty {
            self.reload_list();
        }
        if self.cal.dirty {
            self.cal.reload(self.session.as_ref().map(|s| &s.store));
        }
        self.mark_read_timer(ctx);
        crate::keys::shortcuts(self, ctx);
    }

    fn on_msg(&mut self, ctx: &Context, m: AppMsg) {
        match m {
            AppMsg::SignedIn(Ok((client, name, address))) => {
                self.signin.busy = false;
                self.signin.device = None;
                let mut account = client.config().clone();
                account.username = address;
                account.display_name = name;
                self.settings.account = Some(account.clone());
                self.save_settings();
                self.demo = None;
                self.start_session(account, false, Some(client));
            }
            AppMsg::SignedIn(Err(e)) => {
                self.signin.busy = false;
                self.signin.device = None;
                if !matches!(e, GraphError::SignInCancelled(ref m) if m == "cancelled") {
                    self.signin.error = Some(crate::signin::explain(&e));
                }
            }
            AppMsg::Resumed(Ok(client)) => {
                if let Some(s) = &mut self.session {
                    s.resuming = false;
                    s.needs_sign_in = None;
                    s.sync.send(Command::Attach(client));
                }
            }
            AppMsg::Resumed(Err(e)) => {
                if let Some(s) = &mut self.session {
                    s.resuming = false;
                    match e {
                        GraphError::Network { .. } => {
                            self.status = "Working offline: can't reach Microsoft 365.".into();
                        }
                        other => s.needs_sign_in = Some(crate::signin::explain(&other)),
                    }
                }
            }
            AppMsg::DeviceCode(dc) => self.signin.device = Some(dc),
            AppMsg::RemoteImage { url, bytes } => {
                let tex = bytes.and_then(|b| crate::render::decode_texture(ctx, &url, &b));
                self.remote.insert(url, tex);
            }
        }
    }

    fn on_update(&mut self, ctx: &Context, u: Update) {
        match u {
            Update::Status(s) => self.status = s,
            Update::Connected(c) => {
                if let Some(s) = &mut self.session {
                    s.connected = c;
                }
            }
            Update::Queued(n) => {
                if let Some(s) = &mut self.session {
                    s.queued = n;
                }
            }
            Update::Account { name, address } => {
                if let Some(s) = &mut self.session {
                    s.name = name.clone();
                    s.address = address.clone();
                    s.needs_sign_in = None;
                }
                if let Some(acc) = &mut self.settings.account {
                    if acc.display_name != name {
                        acc.display_name = name;
                        self.save_settings();
                    }
                }
            }
            Update::FoldersChanged => self.reload_folders(),
            Update::MessagesChanged => {
                self.mail.list_dirty = true;
                self.refresh_reading();
            }
            Update::MessageReady(id) => {
                if self.mail.reading.as_ref().is_some_and(|r| r.row.id == id) {
                    self.load_reading(&id, false);
                }
            }
            Update::CalendarChanged => {
                self.cal.dirty = true;
                if let Some(r) = &self.mail.reading {
                    if r.row.is_meeting {
                        let id = r.row.id.clone();
                        self.load_reading(&id, false);
                    }
                }
            }
            Update::People(q, list) => self.people = (q, list),
            Update::SearchResults(q, ids) => {
                if self.mail.searching.as_deref() == Some(q.as_str()) {
                    self.mail.server_results = Some(ids);
                    self.mail.list_dirty = true;
                }
            }
            Update::AttachmentSaved { path, open } => {
                if open {
                    if let Err(e) = open::that_detached(&path) {
                        self.errors.push(format!("Couldn't open {}: {e}", path.display()));
                    }
                }
            }
            Update::Sent(subject) => self.status = format!("Sent \u{201c}{subject}\u{201d}"),
            Update::DraftSaved { local_id, draft_id } => self.draft_saved(&local_id, &draft_id),
            Update::Error(e) => {
                if !self.errors.contains(&e) {
                    self.errors.push(e);
                }
            }
            Update::SignInRequired(m) => {
                if let Some(s) = &mut self.session {
                    if !s.demo {
                        s.needs_sign_in = Some(if m.is_empty() { "Your sign-in has expired.".into() } else { m });
                    }
                }
            }
            Update::SignedOut => {}
        }
        ctx.request_repaint();
    }

    // ---- mail data ---------------------------------------------------------------------------

    pub fn reload_folders(&mut self) {
        let Some(s) = &self.session else { return };
        self.mail.folders = s.store.folders().unwrap_or_default();
        if self.mail.folder.as_ref().is_none_or(|f| !self.mail.folders.iter().any(|x| &x.id == f)) {
            let inbox = self.mail.folders.iter().find(|f| f.well_known.as_deref() == Some("inbox")).map(|f| f.id.clone());
            if inbox.is_some() || self.mail.folder.is_some() {
                self.mail.folder = inbox;
                self.mail.list_dirty = true;
            }
        }
    }

    pub fn current_folder(&self) -> Option<&FolderRow> {
        let id = self.mail.folder.as_ref()?;
        self.mail.folders.iter().find(|f| &f.id == id)
    }

    pub fn folder_wk(&self, wk: &str) -> Option<String> {
        self.mail.folders.iter().find(|f| f.well_known.as_deref() == Some(wk)).map(|f| f.id.clone())
    }

    /// Focused Inbox applies to this folder.
    pub fn focused_applies(&self) -> bool {
        self.settings.focused_inbox && self.current_folder().is_some_and(|f| f.well_known.as_deref() == Some("inbox")) && self.mail.searching.is_none()
    }

    pub fn reload_list(&mut self) {
        self.mail.list_dirty = false;
        let Some(s) = &self.session else {
            self.mail.list.clear();
            return;
        };
        let me = s.address.clone();
        let list = if let Some(q) = &self.mail.searching {
            let scope = if self.mail.search_all { None } else { self.mail.folder.as_deref() };
            let mut rows = s.store.search(q, scope, 500).unwrap_or_default();
            if let Some(ids) = &self.mail.server_results {
                for id in ids {
                    if !rows.iter().any(|r| &r.id == id) {
                        if let Ok(Some(m)) = s.store.message(id) {
                            if scope.is_none_or(|f| f == m.folder_id) {
                                rows.push(m);
                            }
                        }
                    }
                }
                rows.sort_by(|a, b| b.received.cmp(&a.received));
            }
            for r in &mut rows {
                r.body = None;
            }
            rows
        } else if let Some(f) = self.mail.folder.clone() {
            let filter = if self.focused_applies() && self.mail.filter == ListFilter::All {
                if self.mail.other_tab { ListFilter::Other } else { ListFilter::Focused }
            } else {
                self.mail.filter
            };
            s.store.messages(&f, filter, self.mail.sort, self.mail.ascending, &me).unwrap_or_default()
        } else {
            Vec::new()
        };
        self.mail.list = list;
        // Keep the selection to what's still there.
        let ids: HashSet<&str> = self.mail.list.iter().map(|m| m.id.as_str()).collect();
        self.mail.selected.retain(|id| ids.contains(id.as_str()));
    }

    pub fn select_folder(&mut self, id: &str) {
        self.view = View::Mail;
        self.mail.folder = Some(id.to_string());
        self.mail.selected.clear();
        self.mail.reading = None;
        self.mail.searching = None;
        self.mail.server_results = None;
        self.mail.search.clear();
        self.mail.other_tab = false;
        self.mail.list_dirty = true;
        self.send(Command::SyncFolder(id.to_string()));
    }

    /// Click on a message (with Ctrl or Shift for more than one).
    pub fn select_message(&mut self, id: &str, ctrl: bool, shift: bool) {
        if shift {
            let anchor = self.mail.anchor.clone().unwrap_or_else(|| id.to_string());
            let a = self.mail.list.iter().position(|m| m.id == anchor);
            let b = self.mail.list.iter().position(|m| m.id == id);
            if let (Some(a), Some(b)) = (a, b) {
                let (lo, hi) = (a.min(b), a.max(b));
                self.mail.selected = self.mail.list[lo..=hi].iter().map(|m| m.id.clone()).collect();
            }
        } else if ctrl {
            if let Some(p) = self.mail.selected.iter().position(|s| s == id) {
                self.mail.selected.remove(p);
            } else {
                self.mail.selected.push(id.to_string());
            }
            self.mail.anchor = Some(id.to_string());
        } else {
            self.mail.selected = vec![id.to_string()];
            self.mail.anchor = Some(id.to_string());
        }
        if self.mail.selected.len() == 1 {
            let one = self.mail.selected[0].clone();
            self.load_reading(&one, true);
        } else {
            self.mail.reading = None;
        }
    }

    /// Move the selection up or down the list.
    pub fn step_selection(&mut self, delta: i64, extend: bool) {
        if self.mail.list.is_empty() {
            return;
        }
        let cur = self.mail.selected.last().and_then(|id| self.mail.list.iter().position(|m| &m.id == id));
        let next = match cur {
            Some(i) => (i as i64 + delta).clamp(0, self.mail.list.len() as i64 - 1) as usize,
            None => 0,
        };
        let id = self.mail.list[next].id.clone();
        self.select_message(&id, false, extend);
        self.mail.scroll_to_selected = true;
    }

    /// Show a message in the reading pane (`fresh`: just selected, so ask for anything missing).
    pub fn load_reading(&mut self, id: &str, fresh: bool) {
        let Some(s) = &self.session else { return };
        let Some(row) = s.store.message(id).ok().flatten() else {
            self.mail.reading = None;
            return;
        };
        let attachments = s.store.attachments(id).unwrap_or_default();
        let event = row.event_id.as_deref().and_then(|e| s.store.event(e).ok().flatten());
        let doc = row.body.as_deref().map(mailcraft_html::parse);
        let sender = row.from.address.to_ascii_lowercase();
        let allow_remote = self.settings.remote_images || self.settings.safe_senders.iter().any(|x| x.eq_ignore_ascii_case(&sender));
        let keep = self.mail.reading.as_ref().filter(|r| r.row.id == id);
        let allow_remote = allow_remote || keep.is_some_and(|r| r.allow_remote);
        let opened_at = keep.map(|r| r.opened_at).unwrap_or_else(Instant::now);
        let requested = keep.is_some_and(|r| r.requested);
        let need = row.body.is_none() || (row.has_attachments && attachments.is_empty()) || (row.is_meeting && row.event_id.is_none());
        let ask = fresh || (need && !requested);
        // Inline images for this message.
        for (cid, _ct, bytes) in s.store.inline_images(id).unwrap_or_default() {
            let key = format!("{id}/cid:{cid}");
            if !self.textures.contains_key(&key) {
                if let Some(ctx) = self.ctx_slot.lock().ok().and_then(|g| g.clone()) {
                    if let Some(t) = crate::render::decode_texture(&ctx, &key, &bytes) {
                        self.textures.insert(key, t);
                    }
                }
            }
        }
        self.mail.reading = Some(Reading { row, doc, attachments, event, allow_remote, requested: requested || ask, opened_at });
        if ask {
            self.send(Command::Open(id.to_string()));
        }
        if allow_remote {
            self.fetch_remote_images();
        }
    }

    fn refresh_reading(&mut self) {
        if let Some(id) = self.mail.reading.as_ref().map(|r| r.row.id.clone()) {
            if self.session.as_ref().and_then(|s| s.store.message(&id).ok().flatten()).is_some() {
                self.load_reading(&id, false);
            } else {
                self.mail.reading = None;
            }
        }
    }

    pub fn fetch_remote_images(&mut self) {
        let Some(r) = &self.mail.reading else { return };
        let Some(doc) = &r.doc else { return };
        for img in doc.images() {
            if let mailcraft_html::ImageSource::Remote(url) = &img.source {
                if self.remote.contains_key(url) {
                    continue;
                }
                self.remote.insert(url.clone(), None);
                let (tx, url, wake) = (self.tx.clone(), url.clone(), self.wake());
                std::thread::spawn(move || {
                    let bytes = mailcraft_graph::http::agent()
                        .get(&url)
                        .call()
                        .ok()
                        .filter(|r| r.status().is_success())
                        .and_then(|mut r| r.body_mut().with_config().limit(10 * 1024 * 1024).read_to_vec().ok());
                    let _ = tx.send(AppMsg::RemoteImage { url, bytes });
                    wake();
                });
            }
        }
    }

    fn mark_read_timer(&mut self, ctx: &Context) {
        let Some(after) = self.settings.mark_read_after else { return };
        let Some(r) = &self.mail.reading else { return };
        if r.row.is_read || r.row.id.starts_with("local:") {
            return;
        }
        let elapsed = r.opened_at.elapsed().as_secs_f32();
        if elapsed >= after as f32 {
            let id = r.row.id.clone();
            if let Some(rr) = &mut self.mail.reading {
                rr.row.is_read = true;
            }
            self.act(Action::SetRead { id, read: true });
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs_f32(after as f32 - elapsed + 0.05));
        }
    }

    /// The messages a command applies to: the selection.
    pub fn targets(&self) -> Vec<MessageRow> {
        self.mail.selected.iter().filter_map(|id| self.mail.list.iter().find(|m| &m.id == id).cloned()).collect()
    }

    // ---- commands ----------------------------------------------------------------------------

    pub fn delete_selected(&mut self) {
        let targets = self.targets();
        if targets.is_empty() {
            return;
        }
        let deleted = self.folder_wk("deleteditems");
        let in_deleted = targets.iter().all(|m| Some(&m.folder_id) == deleted.as_ref());
        if in_deleted || deleted.is_none() {
            self.mail.confirm = Some(Confirm::DeleteForever(targets.iter().map(|m| m.id.clone()).collect()));
            return;
        }
        self.after_removing(&targets);
        for m in targets {
            if m.id.starts_with("local:") {
                self.act(Action::Delete { id: m.id });
            } else if let Some(d) = &deleted {
                self.act(Action::Move { id: m.id, dest: d.clone() });
            }
        }
    }

    /// Select the next message after these go away, as Outlook does.
    fn after_removing(&mut self, gone: &[MessageRow]) {
        let ids: HashSet<&str> = gone.iter().map(|m| m.id.as_str()).collect();
        let last = gone.iter().filter_map(|g| self.mail.list.iter().position(|m| m.id == g.id)).max();
        let next = last.and_then(|i| self.mail.list.iter().skip(i + 1).find(|m| !ids.contains(m.id.as_str())).or_else(|| self.mail.list[..i].iter().rev().find(|m| !ids.contains(m.id.as_str()))));
        let next = next.map(|m| m.id.clone());
        self.mail.list.retain(|m| !ids.contains(m.id.as_str()));
        self.mail.selected.clear();
        self.mail.reading = None;
        if let Some(n) = next {
            self.select_message(&n, false, false);
        }
    }

    pub fn delete_forever(&mut self, ids: Vec<String>) {
        let gone: Vec<MessageRow> = self.mail.list.iter().filter(|m| ids.contains(&m.id)).cloned().collect();
        self.after_removing(&gone);
        for id in ids {
            self.act(Action::Delete { id });
        }
    }

    pub fn move_selected(&mut self, dest: &str) {
        let targets = self.targets();
        if targets.is_empty() || targets.iter().all(|m| m.folder_id == dest) {
            return;
        }
        self.after_removing(&targets);
        for m in targets {
            self.act(Action::Move { id: m.id, dest: dest.to_string() });
        }
        let name = self.mail.folders.iter().find(|f| f.id == dest).map(|f| f.name.clone()).unwrap_or_default();
        self.status = format!("Moved to {name}");
    }

    pub fn archive_selected(&mut self) {
        match self.folder_wk("archive") {
            Some(a) => self.move_selected(&a),
            None => self.errors.push("This mailbox has no Archive folder.".into()),
        }
    }

    pub fn set_read_selected(&mut self, read: bool) {
        for m in self.targets() {
            if m.is_read != read {
                self.act(Action::SetRead { id: m.id, read });
            }
        }
        if let Some(r) = &mut self.mail.reading {
            if self.mail.selected.contains(&r.row.id) {
                r.row.is_read = read;
                // Stop the timer marking it read straight back.
                r.opened_at = Instant::now() + std::time::Duration::from_secs(86_400);
            }
        }
    }

    pub fn toggle_read_selected(&mut self) {
        let any_unread = self.targets().iter().any(|m| !m.is_read);
        self.set_read_selected(any_unread);
    }

    pub fn flag_selected(&mut self, status: &str) {
        for m in self.targets() {
            self.act(Action::SetFlag { id: m.id, status: status.to_string() });
        }
    }

    pub fn toggle_flag_selected(&mut self) {
        let all_flagged = self.targets().iter().all(|m| m.flagged());
        self.flag_selected(if all_flagged { "complete" } else { "flagged" });
    }

    pub fn categorize_selected(&mut self, cat: &str) {
        for m in self.targets() {
            let mut cats = m.categories.clone();
            if cat.is_empty() {
                cats.clear();
            } else if let Some(p) = cats.iter().position(|c| c == cat) {
                cats.remove(p);
            } else {
                cats.push(cat.to_string());
            }
            self.act(Action::SetCategories { id: m.id, categories: cats });
        }
    }

    pub fn mark_all_read(&mut self, folder: &str) {
        let Some(s) = &self.session else { return };
        let unread: Vec<String> = s.store.messages(folder, ListFilter::Unread, SortBy::Date, false, "").unwrap_or_default().into_iter().map(|m| m.id).collect();
        for id in unread {
            self.act(Action::SetRead { id, read: true });
        }
    }

    pub fn empty_folder(&mut self, folder: &str) {
        let Some(s) = &self.session else { return };
        let all: Vec<String> = s.store.messages(folder, ListFilter::All, SortBy::Date, false, "").unwrap_or_default().into_iter().map(|m| m.id).collect();
        self.mail.reading = None;
        self.mail.selected.clear();
        for id in all {
            self.act(Action::Delete { id });
        }
    }

    pub fn sync_all(&mut self) {
        self.send(Command::SyncAll);
    }

    pub fn set_work_offline(&mut self, on: bool) {
        self.settings.work_offline = on;
        self.save_settings();
        self.send(Command::WorkOffline(on));
    }

    pub fn start_search(&mut self) {
        let q = self.mail.search.trim().to_string();
        if q.is_empty() {
            self.clear_search();
            return;
        }
        self.view = View::Mail;
        self.mail.searching = Some(q);
        self.mail.server_results = None;
        self.mail.selected.clear();
        self.mail.reading = None;
        self.mail.list_dirty = true;
    }

    pub fn search_server(&mut self) {
        if let Some(q) = self.mail.searching.clone() {
            self.send(Command::SearchServer(q));
        }
    }

    pub fn clear_search(&mut self) {
        self.mail.search.clear();
        self.mail.searching = None;
        self.mail.server_results = None;
        self.mail.list_dirty = true;
    }

    pub fn save_attachment(&mut self, att: &AttachmentRow, open_it: bool) {
        let path = if open_it {
            let dir = std::env::temp_dir().join("MailCraft").join(safe_name(&att.message_id).chars().rev().take(16).collect::<String>());
            dir.join(crate::compose::safe_file_name(&att.name))
        } else {
            match rfd::FileDialog::new().set_file_name(crate::compose::safe_file_name(&att.name)).save_file() {
                Some(p) => p,
                None => return,
            }
        };
        self.send(Command::SaveAttachment { message_id: att.message_id.clone(), attachment_id: att.id.clone(), path, open: open_it });
    }

    // ---- drawing -----------------------------------------------------------------------------

    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        if self.fonts_frames < 2 {
            return;
        }
        let t = Tokens::get(&ctx);
        if self.session.is_none() {
            egui::CentralPanel::default().frame(egui::Frame::new().fill(t.canvas)).show(ui, |ui| crate::signin::screen(self, ui));
            return;
        }
        if let Some(b) = self.backstage {
            egui::CentralPanel::default().frame(egui::Frame::new().fill(t.panel)).show(ui, |ui| crate::backstage::show(self, ui, b));
            self.windows(&ctx);
            return;
        }
        egui::Panel::top("title").exact_size(38.0).frame(egui::Frame::new().fill(t.title_bar)).show(ui, |ui| crate::chrome::title_bar(self, ui));
        egui::Panel::top("ribbon")
            .frame(egui::Frame::new().fill(t.ribbon).inner_margin(egui::Margin { left: 4, right: 4, top: 0, bottom: 2 }).stroke(egui::Stroke::new(1.0, t.border)))
            .show(ui, |ui| crate::ribbon::ribbon(self, ui));
        egui::Panel::bottom("status").exact_size(24.0).frame(egui::Frame::new().fill(t.status_bar)).show(ui, |ui| crate::chrome::status_bar(self, ui));
        crate::chrome::banners(self, ui);
        match self.view {
            View::Mail => self.mail_view(ui, &t),
            View::Calendar => crate::calendar::view(self, ui),
        }
        self.windows(&ctx);
    }

    fn mail_view(&mut self, ui: &mut Ui, t: &Tokens) {
        if self.settings.folder_pane {
            let r = egui::Panel::left("folders")
                .resizable(true)
                .default_size(self.settings.folder_pane_width)
                .size_range(160.0..=420.0)
                .frame(egui::Frame::new().fill(t.panel))
                .show(ui, |ui| crate::folders::pane(self, ui));
            self.settings.folder_pane_width = r.response.rect.width();
        } else {
            egui::Panel::left("folders_min").exact_size(44.0).frame(egui::Frame::new().fill(t.panel)).show(ui, |ui| crate::folders::minimized(self, ui));
        }
        match self.settings.reading_pane {
            ReadingPane::Right => {
                let r = egui::Panel::left("list")
                    .resizable(true)
                    .default_size(self.settings.list_width)
                    .size_range(280.0..=760.0)
                    .frame(egui::Frame::new().fill(t.list).stroke(egui::Stroke::new(1.0, t.border)))
                    .show(ui, |ui| crate::list::list(self, ui));
                self.settings.list_width = r.response.rect.width();
                egui::CentralPanel::default().frame(egui::Frame::new().fill(t.reading)).show(ui, |ui| crate::reading::pane(self, ui));
            }
            ReadingPane::Bottom => {
                egui::Panel::top("list_top")
                    .resizable(true)
                    .default_size(300.0)
                    .size_range(140.0..=900.0)
                    .frame(egui::Frame::new().fill(t.list).stroke(egui::Stroke::new(1.0, t.border)))
                    .show(ui, |ui| crate::list::list(self, ui));
                egui::CentralPanel::default().frame(egui::Frame::new().fill(t.reading)).show(ui, |ui| crate::reading::pane(self, ui));
            }
            ReadingPane::Off => {
                egui::CentralPanel::default().frame(egui::Frame::new().fill(t.list)).show(ui, |ui| crate::list::list(self, ui));
            }
        }
    }

    /// Compose and appointment windows, and dialogs.
    fn windows(&mut self, ctx: &Context) {
        crate::compose::windows(self, ctx);
        crate::reading::reader_windows(self, ctx);
        crate::event::windows(self, ctx);
        crate::dialogs::show(self, ctx);
    }
}
