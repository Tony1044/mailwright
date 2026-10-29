//! The application: state, the session with a mailbox, and the frame's layout.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use egui::{Context, Ui};
use mailwright_graph::auth::DeviceCode;
use mailwright_graph::tokens::{MemoryTokenStore, SystemTokenStore, TokenStore};
use mailwright_graph::discovery::{Discovery, DiscoveryConfig, discover};
use mailwright_graph::{AccountConfig, GraphClient, GraphError};
use mailwright_store::{Action, Addr, AttachmentRow, EventRow, FolderRow, ListFilter, MessageRow, SortBy, Store};
use mailwright_sync::{Command, Mailbox, SyncHandle, Update};
use mailwright_sync::net_mailbox::NetMailbox;

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
    SignedIn(Result<(Box<dyn Mailbox>, String, String), GraphError>),
    /// A saved sign-in resumed (or not), for the account with this address.
    Resumed(String, Result<Box<dyn Mailbox>, GraphError>),
    DeviceCode(DeviceCode),
    Discovered(String, Result<Discovery, GraphError>),
    RemoteImage { url: String, bytes: Option<Vec<u8>> },
    /// A manual IMAP/POP3 account was tested: the account and its password, or why it failed.
    ManualTested(Result<(AccountConfig, String, String), GraphError>),
}

/// Opens Mailwright's own sign-in window on a URL (a separate process with a web view).
pub type SignInWindow = Arc<dyn Fn(&str) -> std::io::Result<std::process::Child> + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignInMethod {
    /// In a Mailwright window.
    InApp,
    /// In the default browser.
    Browser,
    /// A code entered on another device.
    DeviceCode,
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
    pub folders: Vec<FolderRow>,
    pub status: String,
}

impl Session {
    pub fn label(&self) -> &str {
        if self.address.is_empty() { &self.name } else { &self.address }
    }
}

/// The message in the reading pane.
pub struct Reading {
    pub row: MessageRow,
    pub doc: Option<mailwright_html::Document>,
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
    /// Bumped whenever `list` is reloaded (the list's layout is kept until then).
    pub list_gen: u64,
    /// Focused Inbox: unread mail on the tab that isn't showing.
    pub other_half_unread: i64,
    pub layout: crate::list::ListLayout,
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
    /// Remove the account with this address.
    RemoveAccount(String),
}

#[derive(Default)]
pub struct SignInState {
    pub email: String,
    pub discovering: bool,
    pub discovery: Option<Discovery>,
    /// An organisation's own app registration (Advanced).
    pub client_id: String,
    pub show_advanced: bool,
    pub busy: bool,
    pub method: Option<SignInMethod>,
    pub cancel: Option<Arc<AtomicBool>>,
    pub device: Option<DeviceCode>,
    pub error: Option<String>,
    /// Adding another account (the sign-in page over the mailbox).
    pub adding: bool,
    /// The manual IMAP/POP3 setup form, while it's open.
    pub manual: Option<ManualForm>,
}

/// The editable fields of the IMAP/POP3 + SMTP setup form. Ports are text so they can be typed.
#[derive(Default)]
pub struct ManualForm {
    pub email: String,
    pub name: String,
    pub password: String,
    pub protocol: mailwright_graph::MailProtocol,
    pub in_host: String,
    pub in_port: String,
    pub in_security: mailwright_graph::Security,
    pub smtp_host: String,
    pub smtp_port: String,
    pub smtp_security: mailwright_graph::Security,
    pub note: String,
    pub busy: bool,
    pub error: Option<String>,
}

pub struct MailApp {
    pub settings: Settings,
    pub settings_path: Option<PathBuf>,
    pub data_dir: PathBuf,
    pub ctx_slot: Arc<Mutex<Option<Context>>>,
    /// The signed-in mailboxes.
    pub sessions: Vec<Session>,
    /// The one whose folder is showing.
    pub active: usize,
    pub sign_in_window: Option<SignInWindow>,
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
    /// New mail that hasn't been read yet, as (account, message id): while there is any, the
    /// taskbar button carries an envelope.
    pub waiting: Vec<(String, String)>,
    pub new_mail_waiting: bool,
    last_wait_check: Instant,
    /// When the Inbox came into view in the focused window (for clearing the envelope).
    seen_inbox_since: Option<Instant>,
    /// The Desktop Alert showing (Windows).
    pub desktop_alert: Option<crate::alert::DesktopAlert>,
    /// The main window's native handle (Windows), for the taskbar overlay.
    pub native_window: isize,
    icon_badged: Option<bool>,
    window_title: String,
    badge: crate::notify::Badge,
    clicked_tx: Sender<crate::notify::Clicked>,
    clicked_rx: Receiver<crate::notify::Clicked>,
    /// Where sign-ins are kept (asking the keyring is slow, so it's asked once).
    pub keyring_location: Option<String>,
    /// The Automatic Replies dialog, while it's open.
    pub auto_reply: Option<crate::autoreply::AutoReplyUi>,
}

/// Mailwright draws its own Desktop Alert (Windows; elsewhere with MAILWRIGHT_OWN_ALERT set).
fn own_alert() -> bool {
    cfg!(windows) || std::env::var_os("MAILWRIGHT_OWN_ALERT").is_some()
}

/// Open the right backend for a saved account: a plain IMAP/POP3 mailbox when it has manual
/// settings, otherwise the Microsoft 365 (Graph) client.
fn open_saved_backend(account: AccountConfig, store: std::sync::Arc<dyn TokenStore>) -> Result<Box<dyn Mailbox>, GraphError> {
    if account.is_manual() {
        let pw = store.load(&account.username)?.ok_or_else(|| GraphError::SignInRequired("Enter your password again for this mailbox.".into()))?;
        Ok(Box::new(NetMailbox::new(account, pw)?))
    } else {
        Ok(Box::new(GraphClient::from_saved(account, store)?))
    }
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
        let signin = SignInState { client_id: settings.client_id_override.clone(), ..Default::default() };
        let cal = CalState::new(settings.calendar_view);
        let (clicked_tx, clicked_rx) = channel();
        MailApp {
            settings,
            settings_path,
            data_dir: data.unwrap_or_else(data_dir),
            ctx_slot: Arc::new(Mutex::new(None)),
            sessions: Vec::new(),
            active: 0,
            sign_in_window: None,
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
            waiting: Vec::new(),
            new_mail_waiting: false,
            last_wait_check: Instant::now(),
            seen_inbox_since: None,
            desktop_alert: None,
            native_window: 0,
            icon_badged: None,
            window_title: String::new(),
            badge: crate::notify::Badge::new(),
            clicked_tx,
            clicked_rx,
            keyring_location: None,
            auto_reply: None,
        }
    }

    /// The account whose folder is showing.
    pub fn session(&self) -> Option<&Session> {
        self.sessions.get(self.active)
    }

    pub fn session_mut(&mut self) -> Option<&mut Session> {
        self.sessions.get_mut(self.active)
    }

    pub fn session_by_address(&self, address: &str) -> Option<usize> {
        self.sessions.iter().position(|s| s.address.eq_ignore_ascii_case(address) || s.account.username.eq_ignore_ascii_case(address))
    }

    pub fn session_for_folder(&self, folder: &str) -> Option<usize> {
        self.sessions.iter().position(|s| s.folders.iter().any(|f| f.id == folder))
    }

    pub fn session_for_event(&self, event: &str) -> Option<usize> {
        self.sessions.iter().position(|s| s.store.event(event).ok().flatten().is_some())
    }

    pub fn session_for_calendar(&self, calendar: &str) -> Option<usize> {
        self.sessions.iter().position(|s| s.store.calendars().unwrap_or_default().iter().any(|c| c.id == calendar))
    }

    /// Pick up where we left off: every saved account.
    pub fn resume(&mut self) {
        let accounts: Vec<AccountConfig> = self.settings.accounts.iter().filter(|a| !a.username.is_empty()).cloned().collect();
        for acc in accounts {
            self.start_session(acc, false, None);
        }
        self.active = 0;
        self.activate(0);
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

    fn wake(&self) -> mailwright_sync::Wake {
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

    /// Open the cache and start syncing for `account`, as one more mailbox (or in place of the
    /// same one). With `client`, already signed in; otherwise the saved sign-in is resumed in the
    /// background. Returns its index.
    pub fn start_session(&mut self, account: AccountConfig, demo: bool, client: Option<Box<dyn Mailbox>>) -> Option<usize> {
        if let Some(i) = self.session_by_address(&account.username) {
            self.sessions.remove(i);
            if self.active >= self.sessions.len() {
                self.active = 0;
            }
        }
        let db_path = if demo { self.data_dir.join("sample-mailbox.sqlite") } else { self.data_dir.join(format!("mail-{}.sqlite", safe_name(&account.username))) };
        let _ = std::fs::create_dir_all(&self.data_dir);
        let (sync, updates) = match mailwright_sync::start(db_path.clone(), self.settings.offline_days, self.wake()) {
            Ok(x) => x,
            Err(e) => {
                self.errors.push(format!("Couldn't start syncing {}: {e}", account.username));
                return None;
            }
        };
        let store = match Store::open(&db_path) {
            Ok(s) => s,
            Err(e) => {
                self.errors.push(format!("Couldn't open the offline cache for {}: {e}", account.username));
                return None;
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
            let key = address.clone();
            std::thread::spawn(move || {
                let r = open_saved_backend(acc, store_t);
                let _ = tx.send(AppMsg::Resumed(key, r));
                wake();
            });
        }
        let folders = store.folders().unwrap_or_default();
        self.sessions.push(Session {
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
            folders,
            status: if resuming { "Connecting\u{2026}".into() } else { "Updating\u{2026}".into() },
        });
        self.cal.dirty = true;
        self.cal.forget_window();
        Some(self.sessions.len() - 1)
    }

    /// Show account `i`'s mail.
    pub fn activate(&mut self, i: usize) {
        if i >= self.sessions.len() {
            return;
        }
        let changed = self.active != i || self.mail.folder.is_none();
        self.active = i;
        if changed {
            self.mail = MailState { list_dirty: true, expanded: self.mail.expanded.clone(), sort: self.mail.sort, ascending: self.mail.ascending, ..Default::default() };
        }
        self.reload_folders();
        self.status = self.sessions[i].status.clone();
    }

    /// The sample mailbox: a stand-in server in this process.
    pub fn start_demo(&mut self) {
        if let Some(i) = self.sessions.iter().position(|s| s.demo) {
            self.activate(i);
            self.signin.adding = false;
            return;
        }
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
            manual: None,
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
                if let Some(i) = self.start_session(account, true, Some(Box::new(c) as Box<dyn Mailbox>)) {
                    self.activate(i);
                }
                self.signin.adding = false;
            }
            Err(e) => self.signin.error = Some(e.to_string()),
        }
    }

    /// Work out what kind of account the typed address is.
    pub fn discover(&mut self) {
        let email = self.signin.email.trim().to_string();
        if !mailwright_graph::discovery::valid_email(&email) {
            self.signin.error = Some("Enter your email address, like name@example.com.".into());
            return;
        }
        if self.session_by_address(&email).is_some() {
            self.signin.error = Some(format!("{email} is already in Mailwright."));
            return;
        }
        self.signin.error = None;
        self.signin.discovery = None;
        self.signin.discovering = true;
        let (tx, wake) = (self.tx.clone(), self.wake());
        std::thread::spawn(move || {
            let r = discover(&DiscoveryConfig::default(), &email);
            let _ = tx.send(AppMsg::Discovered(email, r));
            wake();
        });
    }

    /// Open the manual IMAP/POP3 + SMTP setup form, pre-filled from the typed address.
    pub fn begin_manual(&mut self) {
        let email = self.signin.email.trim().to_string();
        let (cfg, note) = mailwright_net::providers::suggest(&email);
        self.signin.error = None;
        self.signin.manual = Some(ManualForm {
            email: email.clone(),
            name: String::new(),
            password: String::new(),
            protocol: cfg.protocol,
            in_host: cfg.incoming.host,
            in_port: cfg.incoming.port.to_string(),
            in_security: cfg.incoming.security,
            smtp_host: cfg.smtp.host,
            smtp_port: cfg.smtp.port.to_string(),
            smtp_security: cfg.smtp.security,
            note,
            busy: false,
            error: None,
        });
    }

    /// Build the account from the manual form and test it in the background.
    pub fn test_and_add_manual(&mut self) {
        use mailwright_graph::{ManualConfig, ServerConfig};
        let Some(form) = self.signin.manual.as_ref() else { return };
        // Copy out what we need, so we can validate against the rest of the app without a borrow clash.
        let (email, name, password) = (form.email.trim().to_string(), form.name.trim().to_string(), form.password.clone());
        let (in_host, in_port_s, in_security) = (form.in_host.trim().to_string(), form.in_port.trim().to_string(), form.in_security);
        let (smtp_host, smtp_port_s, smtp_security) = (form.smtp_host.trim().to_string(), form.smtp_port.trim().to_string(), form.smtp_security);
        let protocol = form.protocol;

        let fail = |app: &mut MailApp, msg: &str| {
            if let Some(f) = app.signin.manual.as_mut() {
                f.error = Some(msg.to_string());
            }
        };
        if !mailwright_graph::discovery::valid_email(&email) {
            return fail(self, "Enter your email address, like name@example.com.");
        }
        if self.session_by_address(&email).is_some() {
            return fail(self, &format!("{email} is already in Mailwright."));
        }
        if password.is_empty() || in_host.is_empty() || smtp_host.is_empty() {
            return fail(self, "Fill in your password and the server names.");
        }
        let (in_port, smtp_port): (u16, u16) = (in_port_s.parse().unwrap_or(0), smtp_port_s.parse().unwrap_or(0));
        if in_port == 0 || smtp_port == 0 {
            return fail(self, "Check the port numbers.");
        }
        let manual = ManualConfig {
            protocol,
            incoming: ServerConfig { host: in_host, port: in_port, security: in_security },
            smtp: ServerConfig { host: smtp_host, port: smtp_port, security: smtp_security },
            login: email.clone(),
        };
        let account = AccountConfig { username: email, display_name: name, manual: Some(manual), ..Default::default() };
        if let Some(f) = self.signin.manual.as_mut() {
            f.error = None;
            f.busy = true;
        }
        let (tx, wake) = (self.tx.clone(), self.wake());
        std::thread::spawn(move || {
            let r = NetMailbox::check(&account, &password).map(|_| (account, password, String::new()));
            let _ = tx.send(AppMsg::ManualTested(r));
            wake();
        });
    }

    /// A manual account tested successfully: save its password, remember it, and start syncing.
    fn manual_added(&mut self, account: AccountConfig, password: String) {
        let store = self.token_store(false);
        if let Err(e) = store.save(&account.username, &password) {
            self.signin.manual = Some(ManualForm { error: Some(format!("Couldn't save the password: {e}")), ..self.signin.manual.take().unwrap_or_default() });
            return;
        }
        match self.settings.accounts.iter_mut().find(|a| a.username.eq_ignore_ascii_case(&account.username)) {
            Some(a) => *a = account.clone(),
            None => self.settings.accounts.push(account.clone()),
        }
        self.save_settings();
        let client: Box<dyn Mailbox> = match NetMailbox::new(account.clone(), password) {
            Ok(c) => Box::new(c),
            Err(e) => {
                if let Some(f) = self.signin.manual.as_mut() {
                    f.busy = false;
                    f.error = Some(e.to_string());
                }
                return;
            }
        };
        if let Some(i) = self.start_session(account, false, Some(client)) {
            self.activate(i);
            self.view = View::Mail;
        }
        self.signin.manual = None;
        self.signin.adding = false;
        self.signin.email.clear();
        self.signin.discovery = None;
        self.backstage = None;
    }

    /// Sign in to the discovered account.
    pub fn begin_sign_in(&mut self, method: SignInMethod) {
        let Some(d) = self.signin.discovery.clone() else { return };
        let Some(tenant) = d.tenant() else {
            self.signin.error = Some(d.describe());
            return;
        };
        let Some(client_id) = mailwright_graph::config::client_id(&self.signin.client_id) else {
            self.signin.show_advanced = true;
            self.signin.error = Some("This copy of Mailwright has no built-in app registration yet. Enter your organisation's Application (client) ID under Advanced.".into());
            return;
        };
        if self.settings.client_id_override != self.signin.client_id.trim() {
            self.settings.client_id_override = self.signin.client_id.trim().to_string();
            self.save_settings();
        }
        let cfg = AccountConfig { client_id, tenant, username: d.email.clone(), ..Default::default() };
        let method = if method == SignInMethod::InApp && self.sign_in_window.is_none() { SignInMethod::Browser } else { method };
        self.signin.error = None;
        self.signin.busy = true;
        self.signin.method = Some(method);
        self.signin.device = None;
        let cancel = Arc::new(AtomicBool::new(false));
        self.signin.cancel = Some(cancel.clone());
        let tx = self.tx.clone();
        let wake = self.wake();
        let store = self.token_store(false);
        let window = self.sign_in_window.clone();
        let done = Arc::new(AtomicBool::new(false));
        std::thread::spawn(move || {
            let tokens = match method {
                SignInMethod::DeviceCode => match mailwright_graph::auth::start_device_code(&cfg) {
                    Ok(dc) => {
                        let _ = tx.send(AppMsg::DeviceCode(dc.clone()));
                        wake();
                        mailwright_graph::auth::finish_device_code(&cfg, &dc, cancel)
                    }
                    Err(e) => Err(e),
                },
                SignInMethod::InApp => {
                    let watch = cancel.clone();
                    let finished = done.clone();
                    let open = move |url: &str| -> bool {
                        let Some(w) = &window else { return false };
                        match w(url) {
                            Ok(mut child) => {
                                // Closing the window without finishing cancels the sign-in;
                                // finishing closes the window.
                                let watch = watch.clone();
                                let finished = finished.clone();
                                let url = url.to_string();
                                std::thread::spawn(move || loop {
                                    if finished.load(std::sync::atomic::Ordering::Relaxed) {
                                        std::thread::sleep(std::time::Duration::from_millis(900));
                                        let _ = child.kill();
                                        let _ = child.wait();
                                        return;
                                    }
                                    match child.try_wait() {
                                        // No web view on this computer: carry on in the browser.
                                        Ok(Some(st)) if !st.success() => {
                                            log::warn!("sign-in window unavailable ({st}); using the browser");
                                            if open::that_detached(&url).is_err() {
                                                watch.store(true, std::sync::atomic::Ordering::Relaxed);
                                            }
                                            return;
                                        }
                                        Ok(Some(_)) | Err(_) => {
                                            std::thread::sleep(std::time::Duration::from_secs(3));
                                            watch.store(true, std::sync::atomic::Ordering::Relaxed);
                                            return;
                                        }
                                        Ok(None) => std::thread::sleep(std::time::Duration::from_millis(200)),
                                    }
                                });
                                true
                            }
                            Err(e) => {
                                log::warn!("sign-in window: {e}");
                                open::that_detached(url).is_ok()
                            }
                        }
                    };
                    mailwright_graph::auth::sign_in_with_browser(&cfg, &open, cancel)
                }
                SignInMethod::Browser => mailwright_graph::auth::sign_in_with_browser(&cfg, &|url| open::that_detached(url).is_ok(), cancel),
            };
            done.store(true, std::sync::atomic::Ordering::Relaxed);
            let result = tokens.and_then(|t| {
                let mut client = GraphClient::new(cfg, t, store);
                let me = client.me()?;
                let address = me.mail.clone().filter(|m| !m.is_empty()).or(me.user_principal_name.clone()).unwrap_or_default();
                let name = me.display_name.clone().unwrap_or_default();
                client.set_user(&address, &name)?;
                Ok((Box::new(client) as Box<dyn Mailbox>, name, address))
            });
            let _ = tx.send(AppMsg::SignedIn(result));
            wake();
        });
    }

    /// Sign an account in again (its sign-in expired).
    pub fn reauthenticate(&mut self, i: usize) {
        let Some(s) = self.sessions.get(i) else { return };
        self.signin.email = s.account.username.clone();
        self.signin.discovery = Some(Discovery {
            email: s.account.username.clone(),
            domain: s.account.username.rsplit_once('@').map(|(_, d)| d.to_string()).unwrap_or_default(),
            kind: if s.account.tenant == "consumers" {
                mailwright_graph::discovery::AccountKind::Personal
            } else {
                mailwright_graph::discovery::AccountKind::Work { tenant: s.account.tenant.clone(), organisation: String::new(), federated: false }
            },
            mx: Vec::new(),
        });
        if s.account.client_id != mailwright_graph::config::client_id("").unwrap_or_default() {
            self.signin.client_id = s.account.client_id.clone();
        }
        self.signin.adding = true;
        self.begin_sign_in(SignInMethod::InApp);
    }

    pub fn cancel_sign_in(&mut self) {
        if let Some(c) = self.signin.cancel.take() {
            c.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.signin.busy = false;
        self.signin.device = None;
        self.signin.method = None;
    }

    /// Take an account out of Mailwright (its local copy and saved sign-in go; nothing changes on
    /// the server).
    pub fn remove_account(&mut self, address: &str) {
        if let Some(i) = self.session_by_address(address) {
            let s = self.sessions.remove(i);
            s.sync.send(Command::SignOut);
            if s.demo {
                self.demo = None;
            }
            self.composes.retain(|c| !c.account.eq_ignore_ascii_case(address));
            self.editors.clear();
        }
        self.settings.accounts.retain(|a| !a.username.eq_ignore_ascii_case(address));
        self.save_settings();
        self.active = 0;
        self.mail = MailState { list_dirty: true, expanded: self.mail.expanded.clone(), ..Default::default() };
        self.cal = CalState::new(self.settings.calendar_view);
        if self.sessions.is_empty() {
            self.backstage = None;
        } else {
            self.activate(0);
        }
    }

    pub fn send(&self, c: Command) {
        if let Some(s) = self.session() {
            s.sync.send(c);
        }
    }

    pub fn send_to(&self, i: usize, c: Command) {
        if let Some(s) = self.sessions.get(i) {
            s.sync.send(c);
        }
    }

    pub fn act(&mut self, a: Action) {
        self.send(Command::Do(a));
    }

    /// A change to a calendar item, sent to the account it belongs to.
    pub fn act_for_event(&mut self, event_id: &str, a: Action) {
        match self.session_for_event(event_id) {
            Some(i) => self.send_to(i, Command::Do(a)),
            None => self.act(a),
        }
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
        let mut updates: Vec<(usize, Update)> = Vec::new();
        for (i, s) in self.sessions.iter().enumerate() {
            updates.extend(s.updates.try_iter().map(|u| (i, u)));
        }
        for (i, u) in updates {
            self.on_update(ctx, i, u);
        }
        if self.mail.list_dirty {
            self.reload_list();
        }
        if self.cal.dirty {
            let stores: Vec<&Store> = self.sessions.iter().map(|s| &s.store).collect();
            self.cal.reload(&stores);
        }
        while let Ok(c) = self.clicked_rx.try_recv() {
            self.open_alert(ctx, &c.account, Some(&c.id));
        }
        self.mark_read_timer(ctx);
        crate::keys::shortcuts(self, ctx);
        self.window_state(ctx);
    }

    /// Unread mail in every account's Inbox.
    pub fn inbox_unread(&self) -> i64 {
        self.sessions.iter().flat_map(|s| s.folders.iter()).filter(|f| f.well_known.as_deref() == Some("inbox")).map(|f| f.unread).sum()
    }

    /// The window title, the taskbar envelope, and the dock's unread count.
    fn window_state(&mut self, ctx: &Context) {
        // The envelope stays until the new mail has been read (or deleted), as in Outlook.
        if !self.waiting.is_empty() && self.last_wait_check.elapsed() >= std::time::Duration::from_secs(1) {
            self.last_wait_check = Instant::now();
            let sessions = &self.sessions;
            self.waiting.retain(|(acct, id)| {
                sessions.iter().find(|s| s.label() == acct).is_some_and(|s| s.store.message(id).ok().flatten().is_some_and(|m| !m.is_read))
            });
        }
        // And, as in Outlook, once you've come to Mailwright and looked at your Inbox.
        let focused = ctx.input(|i| i.viewport().focused).unwrap_or(false);
        let on_inbox = self.view == View::Mail && self.backstage.is_none() && self.current_folder().is_some_and(|f| f.well_known.as_deref() == Some("inbox"));
        if !self.waiting.is_empty() && focused && on_inbox {
            let since = *self.seen_inbox_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= std::time::Duration::from_secs(2) {
                log::info!("new mail seen: envelope off");
                self.waiting.clear();
            }
        } else {
            self.seen_inbox_since = None;
        }
        if !self.waiting.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
        self.new_mail_waiting = !self.waiting.is_empty() && self.settings.new_mail_envelope;
        let unread = self.inbox_unread();
        let badged = self.new_mail_waiting;
        if self.icon_badged != Some(badged) && (!cfg!(windows) || self.native_window != 0) {
            self.icon_badged = Some(badged);
            log::info!("taskbar envelope {}", if badged { "on" } else { "off" });
            if cfg!(windows) {
                // An envelope over the taskbar button, as Outlook does (shown even when pinned).
                let png = badged.then(|| crate::app_icon::new_mail_png(32));
                mailwright_winshell::set_overlay(self.native_window, png.as_deref(), if badged { "New mail" } else { "" });
            } else {
                let rgba = if badged { crate::app_icon::rgba_badged(256) } else { crate::app_icon::rgba(256) };
                ctx.send_viewport_cmd(egui::ViewportCommand::Icon(Some(Arc::new(egui::IconData { rgba, width: 256, height: 256 }))));
            }
        }
        self.badge.set(if self.settings.new_mail_envelope { unread } else { 0 });
        let title = match (self.view, self.session()) {
            (View::Calendar, Some(s)) => format!("Calendar \u{2013} {} \u{2013} {}", s.label(), crate::notify::APP_NAME),
            (View::Mail, Some(s)) => {
                let folder = self.current_folder().map(|f| f.name.clone()).unwrap_or_else(|| "Mail".into());
                let count = self.current_folder().filter(|f| f.unread > 0).map(|f| format!(" ({})", f.unread)).unwrap_or_default();
                format!("{folder}{count} \u{2013} {} \u{2013} {}", s.label(), crate::notify::APP_NAME)
            }
            _ => crate::notify::APP_NAME.to_string(),
        };
        if title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }

    /// New mail arrived in account `i`.
    fn mail_arrived(&mut self, ctx: &Context, i: usize, items: Vec<mailwright_sync::NewMail>) {
        log::info!("{} new message(s) for account {i}", items.len());
        let account = self.sessions[i].label().to_string();
        for m in &items {
            self.waiting.push((account.clone(), m.id.clone()));
        }
        // Windows: Mailwright's own Desktop Alert. Linux: the desktop's notifications.
        let own_alert = own_alert();
        if own_alert && self.settings.new_mail_alert {
            self.desktop_alert = crate::alert::DesktopAlert::new(&account, &items);
        }
        let alert = crate::notify::Alert { account, items, popup: self.settings.new_mail_alert && !own_alert, sound: self.settings.new_mail_sound };
        let wake = self.wake();
        crate::notify::announce(alert, Some(self.clicked_tx.clone()), move || wake());
        ctx.request_repaint();
    }

    /// File › Options › Test: what new mail looks and sounds like, with the settings as they are.
    pub fn test_alert(&mut self, popup: bool, sound: bool) {
        let items = vec![mailwright_sync::NewMail {
            id: String::new(),
            from: "Mailwright".into(),
            subject: "This is what new mail looks like".into(),
            preview: "New messages show here for a few seconds, with a sound and an envelope on the taskbar.".into(),
            focused: true,
        }];
        let account = self.session().map(|s| s.label().to_string()).unwrap_or_default();
        if own_alert() && popup {
            self.desktop_alert = crate::alert::DesktopAlert::new(&account, &items).map(|mut a| {
                a.id = None;
                a
            });
        }
        crate::notify::announce(crate::notify::Alert { account, items, popup: popup && !own_alert(), sound }, None, || {});
    }

    /// A new-mail alert was clicked: bring the window forward on that message (or the Inbox).
    pub fn open_alert(&mut self, ctx: &Context, account: &str, id: Option<&str>) {
        ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
        let Some(i) = self.sessions.iter().position(|s| s.label() == account) else { return };
        let Some(id) = id else {
            if let Some(inbox) = self.sessions[i].folders.iter().find(|f| f.well_known.as_deref() == Some("inbox")).map(|f| f.id.clone()) {
                self.backstage = None;
                self.select_folder_in(i, &inbox);
            }
            return;
        };
        let Ok(Some(row)) = self.sessions[i].store.message(id) else { return };
        self.backstage = None;
        self.mail.filter = ListFilter::All;
        self.select_folder_in(i, &row.folder_id);
        if self.focused_applies() {
            self.mail.other_tab = !row.focused;
        }
        self.reload_list();
        self.select_message(id, false, false);
        self.mail.scroll_to_selected = true;
    }

    fn on_msg(&mut self, ctx: &Context, m: AppMsg) {
        match m {
            AppMsg::SignedIn(Ok((client, name, address))) => {
                self.signin.busy = false;
                self.signin.device = None;
                self.signin.method = None;
                let mut account = client.config().clone();
                account.username = address.clone();
                account.display_name = name;
                match self.settings.accounts.iter_mut().find(|a| a.username.eq_ignore_ascii_case(&address)) {
                    Some(a) => *a = account.clone(),
                    None => self.settings.accounts.push(account.clone()),
                }
                self.save_settings();
                if let Some(i) = self.start_session(account, false, Some(client)) {
                    self.activate(i);
                    self.view = View::Mail;
                }
                self.signin.adding = false;
                self.signin.email.clear();
                self.signin.discovery = None;
                self.backstage = None;
            }
            AppMsg::SignedIn(Err(e)) => {
                self.signin.busy = false;
                self.signin.device = None;
                self.signin.method = None;
                if !matches!(e, GraphError::SignInCancelled(ref m) if m == "cancelled") {
                    self.signin.error = Some(crate::signin::explain(&e));
                }
            }
            AppMsg::Resumed(key, Ok(client)) => {
                if let Some(i) = self.session_by_address(&key) {
                    let s = &mut self.sessions[i];
                    s.resuming = false;
                    s.needs_sign_in = None;
                    s.sync.send(Command::Attach(client));
                }
            }
            AppMsg::Resumed(key, Err(e)) => {
                if let Some(i) = self.session_by_address(&key) {
                    let s = &mut self.sessions[i];
                    s.resuming = false;
                    match e {
                        GraphError::Network { .. } => s.status = "Working offline: can't reach Microsoft 365.".into(),
                        other => s.needs_sign_in = Some(crate::signin::explain(&other)),
                    }
                    if i == self.active {
                        self.status = self.sessions[i].status.clone();
                    }
                }
            }
            AppMsg::Discovered(email, r) => {
                if self.signin.email.trim().eq_ignore_ascii_case(&email) {
                    self.signin.discovering = false;
                    match r {
                        Ok(d) => {
                            if d.tenant().is_none() {
                                self.signin.error = Some(format!("{}. Mailwright works with Microsoft 365 and Outlook.com accounts.", d.describe()));
                            }
                            self.signin.discovery = Some(d);
                        }
                        Err(e) => self.signin.error = Some(crate::signin::explain(&e)),
                    }
                }
            }
            AppMsg::DeviceCode(dc) => self.signin.device = Some(dc),
            AppMsg::RemoteImage { url, bytes } => {
                let tex = bytes.and_then(|b| crate::render::decode_texture(ctx, &url, &b));
                self.remote.insert(url, tex);
            }
            AppMsg::ManualTested(Ok((account, password, _))) => self.manual_added(account, password),
            AppMsg::ManualTested(Err(e)) => {
                if let Some(f) = self.signin.manual.as_mut() {
                    f.busy = false;
                    f.error = Some(crate::signin::explain(&e));
                }
            }
        }
    }

    fn on_update(&mut self, ctx: &Context, i: usize, u: Update) {
        let active = i == self.active;
        match u {
            Update::Status(s) => {
                self.sessions[i].status = s.clone();
                if active {
                    self.status = s;
                }
            }
            Update::Connected(c) => self.sessions[i].connected = c,
            Update::Queued(n) => self.sessions[i].queued = n,
            Update::Account { name, address } => {
                let s = &mut self.sessions[i];
                s.name = name.clone();
                s.address = address.clone();
                s.needs_sign_in = None;
                if let Some(acc) = self.settings.accounts.iter_mut().find(|a| a.username.eq_ignore_ascii_case(&address)) {
                    if acc.display_name != name {
                        acc.display_name = name;
                        self.save_settings();
                    }
                }
            }
            Update::FoldersChanged => {
                self.sessions[i].folders = self.sessions[i].store.folders().unwrap_or_default();
                if active {
                    self.reload_folders();
                }
            }
            Update::MessagesChanged => {
                if active {
                    self.mail.list_dirty = true;
                    self.refresh_reading();
                }
            }
            Update::MessageReady(id) => {
                if active && self.mail.reading.as_ref().is_some_and(|r| r.row.id == id) {
                    self.load_reading(&id, false);
                }
            }
            Update::CalendarChanged => {
                self.cal.dirty = true;
                if let Some(r) = &self.mail.reading {
                    if active && r.row.is_meeting {
                        let id = r.row.id.clone();
                        self.load_reading(&id, false);
                    }
                }
            }
            Update::People(q, list) => self.people = (q, list),
            Update::SearchResults(q, ids) => {
                if active && self.mail.searching.as_deref() == Some(q.as_str()) {
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
            Update::NewMail(items) => self.mail_arrived(ctx, i, items),
            Update::DraftSaved { local_id, draft_id } => self.draft_saved(&local_id, &draft_id),
            Update::AutoReply(setting) => {
                if let Some(a) = self.auto_reply.as_mut().filter(|a| a.account == i) {
                    if a.saving {
                        // A save just came back: confirm and close.
                        self.status = "Automatic replies updated".into();
                        self.auto_reply = None;
                    } else {
                        a.apply(&setting);
                    }
                }
            }
            Update::AutoReplyError(m) => {
                if let Some(a) = self.auto_reply.as_mut().filter(|a| a.account == i) {
                    a.loading = false;
                    a.saving = false;
                    a.error = Some(m);
                }
            }
            Update::Error(e) => {
                let e = if self.sessions.len() > 1 { format!("{}: {e}", self.sessions[i].label()) } else { e };
                if !self.errors.contains(&e) {
                    self.errors.push(e);
                }
            }
            Update::SignInRequired(m) => {
                let s = &mut self.sessions[i];
                if !s.demo {
                    s.needs_sign_in = Some(if m.is_empty() { "Your sign-in has expired.".into() } else { m });
                }
            }
            Update::SignedOut => {}
        }
        ctx.request_repaint();
    }

    // ---- mail data ---------------------------------------------------------------------------

    pub fn reload_folders(&mut self) {
        let Some(s) = self.session() else { return };
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
        let Some(s) = self.session() else {
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
        self.mail.other_half_unread = match (&self.mail.folder, self.focused_applies()) {
            (Some(f), true) => s.store.unread_split(f, self.mail.other_tab).unwrap_or(0),
            _ => 0,
        };
        self.mail.list = list;
        self.mail.list_gen += 1;
        // Keep the selection to what's still there.
        let ids: HashSet<&str> = self.mail.list.iter().map(|m| m.id.as_str()).collect();
        self.mail.selected.retain(|id| ids.contains(id.as_str()));
    }

    pub fn select_folder(&mut self, id: &str) {
        let acct = if self.session().is_some_and(|s| s.folders.iter().any(|f| f.id == id)) { self.active } else { self.session_for_folder(id).unwrap_or(self.active) };
        self.select_folder_in(acct, id);
    }

    /// Open folder `id` of account `acct`.
    pub fn select_folder_in(&mut self, acct: usize, id: &str) {
        if acct != self.active {
            self.activate(acct);
        }
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
        let Some(s) = self.session() else { return };
        let Some(row) = s.store.message(id).ok().flatten() else {
            self.mail.reading = None;
            return;
        };
        let attachments = s.store.attachments(id).unwrap_or_default();
        let event = row.event_id.as_deref().and_then(|e| s.store.event(e).ok().flatten());
        let doc = row.body.as_deref().map(mailwright_html::parse);
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
            if self.session().and_then(|s| s.store.message(&id).ok().flatten()).is_some() {
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
            if let mailwright_html::ImageSource::Remote(url) = &img.source {
                if self.remote.contains_key(url) {
                    continue;
                }
                self.remote.insert(url.clone(), None);
                let (tx, url, wake) = (self.tx.clone(), url.clone(), self.wake());
                std::thread::spawn(move || {
                    let bytes = mailwright_graph::http::agent()
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
        if !self.session().is_some_and(|s| s.folders.iter().any(|f| f.id == dest)) {
            self.errors.push("Messages can't be moved between accounts yet. Forward them instead.".into());
            return;
        }
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
        let Some(i) = self.session_for_folder(folder) else { return };
        let unread: Vec<String> = self.sessions[i].store.messages(folder, ListFilter::Unread, SortBy::Date, false, "").unwrap_or_default().into_iter().map(|m| m.id).collect();
        for id in unread {
            self.send_to(i, Command::Do(Action::SetRead { id, read: true }));
        }
    }

    pub fn empty_folder(&mut self, folder: &str) {
        let Some(i) = self.session_for_folder(folder) else { return };
        let all: Vec<String> = self.sessions[i].store.messages(folder, ListFilter::All, SortBy::Date, false, "").unwrap_or_default().into_iter().map(|m| m.id).collect();
        if i == self.active {
            self.mail.reading = None;
            self.mail.selected.clear();
        }
        for id in all {
            self.send_to(i, Command::Do(Action::Delete { id }));
        }
    }

    /// Send/Receive: every account.
    pub fn sync_all(&mut self) {
        for s in &self.sessions {
            s.sync.send(Command::SyncAll);
        }
    }

    pub fn set_work_offline(&mut self, on: bool) {
        self.settings.work_offline = on;
        self.save_settings();
        for s in &self.sessions {
            if !s.demo {
                s.sync.send(Command::WorkOffline(on));
            }
        }
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
            let dir = std::env::temp_dir().join("Mailwright").join(safe_name(&att.message_id).chars().rev().take(16).collect::<String>());
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
        if self.sessions.is_empty() || self.signin.adding {
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

    /// Open the Automatic Replies dialog for account `i` and ask the server for its current state.
    pub fn open_auto_reply(&mut self, i: usize) {
        self.auto_reply = Some(crate::autoreply::AutoReplyUi::loading(i));
        self.send_to(i, mailwright_sync::Command::GetAutoReply);
    }

    /// Compose and appointment windows, and dialogs.
    fn windows(&mut self, ctx: &Context) {
        crate::alert::show(self, ctx);
        crate::autoreply::dialog(self, ctx);
        crate::compose::windows(self, ctx);
        crate::reading::reader_windows(self, ctx);
        crate::event::windows(self, ctx);
        crate::dialogs::show(self, ctx);
    }
}
