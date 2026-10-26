//! A stand-in for Microsoft Entra ID's token endpoints and the parts of Microsoft Graph that
//! Mailwright uses, holding a sample mailbox in memory. It runs on `127.0.0.1` on a free port; point
//! Mailwright at it with `MAILWRIGHT_LOGIN_BASE` / `MAILWRIGHT_GRAPH_BASE` (or [`MockGraph::login_base`]
//! and [`MockGraph::graph_base`]). Used by the integration tests and by Mailwright's sample
//! mailbox (demo mode), so nobody needs a tenant to try it.
//!
//! It behaves like Graph where Mailwright depends on it: delta rounds with next/delta links and
//! `@removed` entries, `$select`, `$top`/`$skip` paging, `$search`, drafts, reply/forward drafts,
//! upload sessions, move/delete, calendar views, meeting responses, bearer tokens that expire, and
//! throttling (429 with Retry-After) on request.

use std::collections::{HashMap, HashSet};
use std::io::Read as _;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;

use base64::Engine as _;
use serde_json::{Map, Value, json};

mod png;
mod seed;

pub use png::banner_png;

/// The sample mailbox's owner.
pub const ME_NAME: &str = "Sam Carter";
pub const ME_ADDRESS: &str = "sam.carter@harbourview.example";

#[derive(Clone, Debug)]
pub struct Folder {
    pub id: String,
    pub name: String,
    pub parent: Option<String>,
    pub well_known: Option<&'static str>,
    pub hidden: bool,
}

#[derive(Clone, Debug)]
pub struct Att {
    pub id: String,
    pub name: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
    pub inline: bool,
    pub content_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Msg {
    pub id: String,
    pub folder: String,
    /// Change counter: bumped on every change, for delta.
    pub seq: u64,
    /// The Graph properties other than id, body, parentFolderId and hasAttachments.
    pub props: Map<String, Value>,
    pub body: String,
    pub attachments: Vec<Att>,
    /// For meeting messages: `meetingRequest`, `meetingCancelled`, …
    pub meeting_type: Option<String>,
    pub event_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Ev {
    pub id: String,
    pub calendar: String,
    pub props: Map<String, Value>,
    pub body: String,
}

struct Upload {
    message: String,
    name: String,
    size: usize,
    content_type: String,
    inline: bool,
    content_id: Option<String>,
    data: Vec<u8>,
}

/// Everything the mock holds. Tests can look and change it through [`MockGraph::state`].
pub struct State {
    pub seq: u64,
    pub next_id: u64,
    pub folders: Vec<Folder>,
    pub messages: Vec<Msg>,
    /// (folder, message id, seq) for messages that left a folder.
    pub tombstones: Vec<(String, String, u64)>,
    pub calendars: Vec<Value>,
    pub events: Vec<Ev>,
    pub people: Vec<Value>,
    pub access_tokens: HashSet<String>,
    pub refresh_tokens: HashSet<String>,
    codes: HashSet<String>,
    device_polls: u32,
    uploads: HashMap<String, Upload>,
    /// Answer the next this-many Graph requests with 429.
    pub throttle_next: u32,
    /// Answer changes to messages (PATCH) with 503 while this is set.
    pub refuse_message_changes: bool,
    /// Every request seen, as `METHOD /path?query`.
    pub log: Vec<String>,
    /// Messages sent (their JSON at send time).
    pub sent: Vec<Value>,
    /// Access token lifetime in seconds, as reported to the client.
    pub token_lifetime: i64,
    /// The mailbox's owner.
    pub owner_name: String,
    pub owner_address: String,
}

impl State {
    fn new() -> State {
        State {
            seq: 1,
            next_id: 1000,
            folders: Vec::new(),
            messages: Vec::new(),
            tombstones: Vec::new(),
            calendars: Vec::new(),
            events: Vec::new(),
            people: Vec::new(),
            access_tokens: HashSet::new(),
            refresh_tokens: HashSet::new(),
            codes: HashSet::new(),
            device_polls: 0,
            uploads: HashMap::new(),
            throttle_next: 0,
            refuse_message_changes: false,
            log: Vec::new(),
            sent: Vec::new(),
            token_lifetime: 3600,
            owner_name: ME_NAME.into(),
            owner_address: ME_ADDRESS.into(),
        }
    }

    pub fn new_id(&mut self, prefix: &str) -> String {
        self.next_id += 1;
        format!("{prefix}{:08X}AAA=", self.next_id * 2654435761 % 0xFFFF_FFFF)
    }

    pub fn bump(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    /// A folder id from an id or a well-known name.
    pub fn folder_id(&self, name_or_id: &str) -> Option<String> {
        let lower = name_or_id.to_ascii_lowercase();
        self.folders.iter().find(|f| f.id == name_or_id || f.well_known == Some(lower.as_str())).map(|f| f.id.clone())
    }

    pub fn msg(&self, id: &str) -> Option<&Msg> {
        self.messages.iter().find(|m| m.id == id)
    }

    fn msg_mut(&mut self, id: &str) -> Option<&mut Msg> {
        self.messages.iter_mut().find(|m| m.id == id)
    }

    /// Move a message, leaving a tombstone in the folder it left. It keeps its id (Graph gives it
    /// a new one; Mailwright copes either way because it uses the id the move returns).
    pub fn move_msg(&mut self, id: &str, to: &str) -> Option<String> {
        let seq = self.bump();
        let new_id = self.new_id("AAMkMv");
        let m = self.msg_mut(id)?;
        let from = std::mem::replace(&mut m.folder, to.to_string());
        m.seq = seq;
        m.id = new_id.clone();
        self.tombstones.push((from, id.to_string(), seq));
        Some(new_id)
    }

    pub fn remove_msg(&mut self, id: &str) -> bool {
        let Some(pos) = self.messages.iter().position(|m| m.id == id) else { return false };
        let m = self.messages.remove(pos);
        let seq = self.bump();
        self.tombstones.push((m.folder, m.id, seq));
        true
    }

    /// Put a new message in a folder (as if it had just arrived).
    pub fn deliver(&mut self, folder: &str, props: Map<String, Value>, body: &str, attachments: Vec<Att>) -> String {
        let id = self.new_id("AAMkNew");
        let seq = self.bump();
        let folder = self.folder_id(folder).unwrap_or_else(|| folder.to_string());
        self.messages.push(Msg { id: id.clone(), folder, seq, props, body: body.to_string(), attachments, meeting_type: None, event_id: None });
        id
    }

    fn me(&self) -> Value {
        json!({"emailAddress": {"name": self.owner_name, "address": self.owner_address}})
    }

    fn msg_json(&self, m: &Msg, select: Option<&str>, expand_event: bool) -> Value {
        let mut o = m.props.clone();
        o.insert("id".into(), json!(m.id));
        o.insert("parentFolderId".into(), json!(m.folder));
        o.insert("hasAttachments".into(), json!(m.attachments.iter().any(|a| !a.inline)));
        o.insert("body".into(), json!({"contentType": "html", "content": m.body}));
        if let Some(t) = &m.meeting_type {
            let suffix = match t.as_str() {
                "meetingRequest" => "Request",
                "meetingCancelled" => "",
                _ => "Response",
            };
            o.insert("@odata.type".into(), json!(format!("#microsoft.graph.eventMessage{suffix}")));
        }
        let mut v = Value::Object(apply_select(o, select));
        if expand_event {
            if let Some(t) = &m.meeting_type {
                v["meetingMessageType"] = json!(t);
            }
            if let Some(eid) = &m.event_id
                && let Some(e) = self.events.iter().find(|e| &e.id == eid) {
                    v["event"] = self.event_json(e, None);
                }
        }
        v
    }

    fn event_json(&self, e: &Ev, select: Option<&str>) -> Value {
        let mut o = e.props.clone();
        o.insert("id".into(), json!(e.id));
        o.insert("body".into(), json!({"contentType": "html", "content": e.body}));
        Value::Object(apply_select(o, select))
    }

    fn unread(&self, folder: &str) -> usize {
        self.messages.iter().filter(|m| m.folder == folder && m.props.get("isRead") == Some(&json!(false))).count()
    }

    fn folder_json(&self, f: &Folder) -> Value {
        json!({
            "id": f.id,
            "displayName": f.name,
            "parentFolderId": f.parent.clone().unwrap_or_else(|| "AAMkRoot".into()),
            "childFolderCount": self.folders.iter().filter(|c| c.parent.as_deref() == Some(f.id.as_str())).count(),
            "unreadItemCount": self.unread(&f.id),
            "totalItemCount": self.messages.iter().filter(|m| m.folder == f.id).count(),
            "isHidden": f.hidden,
        })
    }

    fn issue_tokens(&mut self) -> Value {
        let at = format!("at-{}", self.new_id(""));
        let rt = format!("rt-{}", self.new_id(""));
        self.access_tokens.insert(at.clone());
        self.refresh_tokens.insert(rt.clone());
        json!({"token_type": "Bearer", "access_token": at, "refresh_token": rt, "expires_in": self.token_lifetime, "scope": "Mail.ReadWrite"})
    }
}

/// Keep only the selected properties (plus the annotations Graph always sends).
fn apply_select(mut o: Map<String, Value>, select: Option<&str>) -> Map<String, Value> {
    if let Some(sel) = select {
        let keep: HashSet<&str> = sel.split(',').map(str::trim).collect();
        o.retain(|k, _| keep.contains(k.as_str()) || k == "id" || k.starts_with('@'));
    }
    o
}

pub struct MockGraph {
    root: String,
    state: Arc<Mutex<State>>,
    server: Arc<tiny_http::Server>,
    thread: Option<JoinHandle<()>>,
}

impl MockGraph {
    /// Start with the sample mailbox.
    pub fn start() -> std::io::Result<MockGraph> {
        Self::start_on(0, true)
    }

    /// Start empty (no folders, messages or events beyond the well-known folders).
    pub fn start_empty() -> std::io::Result<MockGraph> {
        Self::start_on(0, false)
    }

    /// Start with the sample mailbox belonging to someone else (a second account in tests).
    pub fn start_as(name: &str, address: &str) -> std::io::Result<MockGraph> {
        let m = Self::start_on(0, true)?;
        {
            let mut st = m.state();
            st.owner_name = name.to_string();
            st.owner_address = address.to_string();
            let swap = |v: &mut Value| {
                let text = v.to_string().replace(ME_ADDRESS, address).replace(ME_NAME, name);
                if let Ok(n) = serde_json::from_str(&text) {
                    *v = n;
                }
            };
            for msg in &mut st.messages {
                let mut v = Value::Object(std::mem::take(&mut msg.props));
                swap(&mut v);
                if let Value::Object(o) = v {
                    msg.props = o;
                }
            }
            for e in &mut st.events {
                let mut v = Value::Object(std::mem::take(&mut e.props));
                swap(&mut v);
                if let Value::Object(o) = v {
                    e.props = o;
                }
            }
        }
        Ok(m)
    }

    pub fn start_on(port: u16, sample: bool) -> std::io::Result<MockGraph> {
        let server = tiny_http::Server::http(("127.0.0.1", port)).map_err(|e| std::io::Error::other(e.to_string()))?;
        let port = server.server_addr().to_ip().map(|a| a.port()).unwrap_or(port);
        let root = format!("http://127.0.0.1:{port}");
        let mut st = State::new();
        seed::well_known_folders(&mut st);
        if sample {
            seed::sample_mailbox(&mut st, chrono::Utc::now());
        }
        let state = Arc::new(Mutex::new(st));
        let server = Arc::new(server);
        let (s2, st2, root2) = (server.clone(), state.clone(), root.clone());
        let thread = std::thread::Builder::new().name("mockgraph".into()).spawn(move || {
            for req in s2.incoming_requests() {
                handle(&st2, &root2, req);
            }
        })?;
        Ok(MockGraph { root, state, server, thread: Some(thread) })
    }

    pub fn root(&self) -> &str {
        &self.root
    }
    pub fn login_base(&self) -> String {
        format!("{}/login", self.root)
    }
    pub fn graph_base(&self) -> String {
        format!("{}/v1.0", self.root)
    }
    /// A DNS-over-HTTPS endpoint answering MX lookups for test domains.
    pub fn doh_base(&self) -> String {
        format!("{}/dns-query", self.root)
    }

    pub fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    /// A refresh token the mock accepts (as if the user had signed in before).
    pub fn grant_refresh_token(&self) -> String {
        let mut st = self.state();
        let rt = format!("rt-{}", st.new_id(""));
        st.refresh_tokens.insert(rt.clone());
        rt
    }

    /// Make every access token handed out so far stop working (the client must refresh).
    pub fn expire_access_tokens(&self) {
        self.state().access_tokens.clear();
    }

    /// Revoke every refresh token (the user must sign in again).
    pub fn revoke_refresh_tokens(&self) {
        self.state().refresh_tokens.clear();
    }
}

impl Drop for MockGraph {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn lock(m: &Mutex<State>) -> MutexGuard<'_, State> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

struct Reply {
    status: u16,
    body: Vec<u8>,
    content_type: &'static str,
    headers: Vec<(&'static str, String)>,
}

fn ok(v: Value) -> Reply {
    Reply { status: 200, body: v.to_string().into_bytes(), content_type: "application/json", headers: vec![] }
}
fn created(v: Value) -> Reply {
    Reply { status: 201, ..ok(v) }
}
fn no_content() -> Reply {
    Reply { status: 204, body: vec![], content_type: "text/plain", headers: vec![] }
}
fn accepted() -> Reply {
    Reply { status: 202, body: vec![], content_type: "text/plain", headers: vec![] }
}
fn err(status: u16, code: &str, message: &str) -> Reply {
    Reply { status, ..ok(json!({"error": {"code": code, "message": message}})) }
}
fn not_found() -> Reply {
    err(404, "ErrorItemNotFound", "The specified object was not found in the store.")
}
fn oauth_err(error: &str, desc: &str) -> Reply {
    Reply { status: 400, ..ok(json!({"error": error, "error_description": desc})) }
}

fn handle(state: &Mutex<State>, root: &str, mut req: tiny_http::Request) {
    let method = req.method().as_str().to_ascii_uppercase();
    let raw_url = req.url().to_string();
    let header = |name: &str| {
        req.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name)).map(|h| h.value.as_str().to_string())
    };
    let auth = header("Authorization");
    let prefer = header("Prefer").unwrap_or_default();
    let range = header("Content-Range");
    let mut body = Vec::new();
    let _ = req.as_reader().take(200 * 1024 * 1024).read_to_end(&mut body);

    let reply = {
        let mut st = lock(state);
        st.log.push(format!("{method} {raw_url}"));
        route(&mut st, root, &method, &raw_url, auth.as_deref(), &prefer, range.as_deref(), &body)
    };

    let mut resp = tiny_http::Response::from_data(reply.body).with_status_code(reply.status);
    if let Ok(h) = tiny_http::Header::from_bytes("Content-Type", reply.content_type) {
        resp.add_header(h);
    }
    for (k, v) in reply.headers {
        if let Ok(h) = tiny_http::Header::from_bytes(k, v.as_bytes()) {
            resp.add_header(h);
        }
    }
    let _ = req.respond(resp);
}

struct Query(Vec<(String, String)>);

impl Query {
    fn get(&self, k: &str) -> Option<&str> {
        self.0.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str())
    }
}

fn route(st: &mut State, root: &str, method: &str, raw_url: &str, auth: Option<&str>, prefer: &str, range: Option<&str>, body: &[u8]) -> Reply {
    let Ok(u) = url::Url::parse(&format!("http://mock{raw_url}")) else { return err(400, "BadRequest", "bad url") };
    let q = Query(u.query_pairs().map(|(k, v)| (k.to_string(), v.to_string())).collect());
    let segs: Vec<String> = u.path_segments().map(|s| s.map(percent_decode).collect()).unwrap_or_default();
    let segs: Vec<&str> = segs.iter().map(String::as_str).collect();

    match segs.first().copied() {
        Some("login") if segs.get(2) == Some(&"userrealm") => {
            let email = segs.get(3).copied().unwrap_or("");
            let domain = email.rsplit_once('@').map(|(_, d)| d.to_ascii_lowercase()).unwrap_or_default();
            let ours = st.owner_address.rsplit_once('@').map(|(_, d)| d.to_ascii_lowercase()).unwrap_or_default();
            return if domain == ours || domain.ends_with(".onmicrosoft.example") {
                ok(json!({"NameSpaceType": "Managed", "DomainName": domain, "FederationBrandName": "Harbourview", "CloudInstanceName": "microsoftonline.com"}))
            } else if domain.starts_with("fed.") {
                ok(json!({"NameSpaceType": "Federated", "DomainName": domain, "FederationBrandName": "Federated Co", "AuthURL": "https://sts.example/adfs"}))
            } else {
                ok(json!({"NameSpaceType": "Unknown", "Login": email}))
            };
        }
        Some("dns-query") => {
            let name = q.get("name").unwrap_or("").to_ascii_lowercase();
            let mx = if name.starts_with("gmail") || name.ends_with("googlemail.example") {
                "10 aspmx.l.google.com."
            } else if name.starts_with("m365") {
                "0 m365-example.mail.protection.outlook.com."
            } else {
                ""
            };
            let answer: Vec<Value> = if mx.is_empty() { vec![] } else { vec![json!({"name": name, "type": 15, "data": mx})] };
            return ok(json!({"Status": 0, "Answer": answer}));
        }
        Some("login") => return login(st, root, method, &segs, &q, body),
        Some("devicelogin") => return Reply { status: 200, body: b"Mock device login".to_vec(), content_type: "text/plain", headers: vec![] },
        Some("upload") => return upload(st, method, &segs, range, body),
        Some("v1.0") => {}
        _ => return not_found(),
    }

    // Graph: bearer token, then throttling.
    let token = auth.and_then(|a| a.strip_prefix("Bearer ")).unwrap_or("");
    if !st.access_tokens.contains(token) {
        return err(401, "InvalidAuthenticationToken", "Access token has expired or is not yet valid.");
    }
    if st.throttle_next > 0 {
        st.throttle_next -= 1;
        let mut r = err(429, "ApplicationThrottled", "Application is over its MailboxConcurrency limit.");
        r.headers.push(("Retry-After", "1".into()));
        return r;
    }
    let body_json: Value = if body.is_empty() { Value::Null } else { serde_json::from_slice(body).unwrap_or(Value::Null) };
    let graph = format!("{root}/v1.0");
    let select = q.get("$select");
    let _ = prefer;

    match (method, &segs[1..]) {
        ("GET", ["me"]) => ok(json!({"id": "user-1", "displayName": st.owner_name, "mail": st.owner_address, "userPrincipalName": st.owner_address})),
        ("GET", ["me", "mailboxSettings"]) => ok(json!({"timeZone": "GMT Standard Time"})),

        // ---- folders
        ("GET", ["me", "mailFolders"]) => {
            let list: Vec<Value> = st.folders.iter().filter(|f| f.parent.is_none()).map(|f| st.folder_json(f)).collect();
            paged(list, &q, &format!("{graph}/me/mailFolders"))
        }
        ("POST", ["me", "mailFolders"]) => create_folder(st, None, &body_json),
        ("GET", ["me", "mailFolders", f]) => match st.folder_id(f).and_then(|id| st.folders.iter().find(|x| x.id == id).cloned()) {
            Some(fd) => ok(st.folder_json(&fd)),
            None => not_found(),
        },
        ("PATCH", ["me", "mailFolders", f]) => {
            let Some(id) = st.folder_id(f) else { return not_found() };
            let name = body_json["displayName"].as_str().unwrap_or("").to_string();
            if let Some(fd) = st.folders.iter_mut().find(|x| x.id == id)
                && !name.is_empty() {
                    fd.name = name;
                }
            match st.folders.iter().find(|x| x.id == id).cloned() {
                Some(fd) => ok(st.folder_json(&fd)),
                None => not_found(),
            }
        }
        ("DELETE", ["me", "mailFolders", f]) => {
            let Some(id) = st.folder_id(f) else { return not_found() };
            if st.folders.iter().any(|x| x.id == id && x.well_known.is_some()) {
                return err(403, "ErrorDeleteDistinguishedFolder", "Distinguished folders cannot be deleted.");
            }
            // The folder and everything under it go to Deleted Items' care: here, they just go.
            let mut doomed = vec![id];
            let mut i = 0;
            while i < doomed.len() {
                let kids: Vec<String> = st.folders.iter().filter(|x| x.parent.as_deref() == Some(doomed[i].as_str())).map(|x| x.id.clone()).collect();
                doomed.extend(kids);
                i += 1;
            }
            st.folders.retain(|x| !doomed.contains(&x.id));
            let gone: Vec<String> = st.messages.iter().filter(|m| doomed.contains(&m.folder)).map(|m| m.id.clone()).collect();
            for g in gone {
                st.remove_msg(&g);
            }
            no_content()
        }
        ("POST", ["me", "mailFolders", f, "move"]) => {
            let Some(id) = st.folder_id(f) else { return not_found() };
            let Some(dest) = body_json["destinationId"].as_str().and_then(|d| st.folder_id(d)) else { return err(400, "ErrorInvalidIdMalformed", "Id is malformed.") };
            if st.folders.iter().any(|x| x.id == id && x.well_known.is_some()) {
                return err(403, "ErrorMoveDistinguishedFolder", "Distinguished folders cannot be moved.");
            }
            if let Some(fd) = st.folders.iter_mut().find(|x| x.id == id) {
                fd.parent = Some(dest);
            }
            match st.folders.iter().find(|x| x.id == id).cloned() {
                Some(fd) => created(st.folder_json(&fd)),
                None => not_found(),
            }
        }
        ("GET", ["me", "mailFolders", f, "childFolders"]) => {
            let Some(id) = st.folder_id(f) else { return not_found() };
            let list: Vec<Value> = st.folders.iter().filter(|x| x.parent.as_deref() == Some(id.as_str())).map(|x| st.folder_json(x)).collect();
            paged(list, &q, &format!("{graph}/me/mailFolders/{id}/childFolders"))
        }
        ("POST", ["me", "mailFolders", f, "childFolders"]) => {
            let Some(id) = st.folder_id(f) else { return not_found() };
            create_folder(st, Some(id), &body_json)
        }

        // ---- messages
        ("GET", ["me", "mailFolders", f, "messages", "delta"]) => {
            let Some(id) = st.folder_id(f) else { return not_found() };
            delta(st, &graph, &id, &q, prefer)
        }
        ("GET", ["me", "mailFolders", f, "messages"]) => {
            let Some(id) = st.folder_id(f) else { return not_found() };
            let mut list: Vec<&Msg> = st.messages.iter().filter(|m| m.folder == id).collect();
            list.sort_by(|a, b| received(b).cmp(&received(a)));
            let vals = list.iter().map(|m| st.msg_json(m, select, false)).collect();
            paged(vals, &q, &format!("{graph}/me/mailFolders/{id}/messages"))
        }
        ("GET", ["me", "messages"]) => {
            let needle = q.get("$search").unwrap_or("").trim_matches('"').to_lowercase();
            let mut list: Vec<&Msg> = st.messages.iter().filter(|m| needle.is_empty() || matches_search(m, &needle)).collect();
            list.sort_by(|a, b| received(b).cmp(&received(a)));
            let vals = list.iter().map(|m| st.msg_json(m, select, false)).collect();
            paged(vals, &q, &format!("{graph}/me/messages"))
        }
        ("POST", ["me", "messages"]) => {
            let Some(drafts) = st.folder_id("drafts") else { return not_found() };
            let id = st.new_id("AAMkDraft");
            let seq = st.bump();
            let mut props = Map::new();
            props.insert("isDraft".into(), json!(true));
            props.insert("isRead".into(), json!(true));
            props.insert("from".into(), st.me());
            let now = now_iso();
            props.insert("createdDateTime".into(), json!(now));
            props.insert("receivedDateTime".into(), json!(now));
            props.insert("lastModifiedDateTime".into(), json!(now));
            let mut m = Msg { id, folder: drafts, seq, props, body: String::new(), attachments: vec![], meeting_type: None, event_id: None };
            patch_message(&mut m, &body_json);
            let v = st.msg_json(&m, None, false);
            st.messages.push(m);
            created(v)
        }
        ("GET", ["me", "messages", id]) => match st.msg(id) {
            Some(m) => ok(st.msg_json(m, select, q.get("$expand").is_some_and(|e| e.contains("event")))),
            None => not_found(),
        },
        ("PATCH", ["me", "messages", _]) if st.refuse_message_changes => err(503, "ServiceUnavailable", "Try again later"),
        ("PATCH", ["me", "messages", id]) => {
            let seq = st.bump();
            let Some(m) = st.msg_mut(id) else { return not_found() };
            patch_message(m, &body_json);
            m.seq = seq;
            let m = m.clone();
            ok(st.msg_json(&m, None, false))
        }
        ("DELETE", ["me", "messages", id]) => {
            if st.remove_msg(id) { no_content() } else { not_found() }
        }
        ("POST", ["me", "messages", id, "move"]) => {
            let dest = body_json["destinationId"].as_str().unwrap_or("");
            let Some(to) = st.folder_id(dest) else { return err(400, "ErrorInvalidIdMalformed", "Id is malformed.") };
            match st.move_msg(id, &to) {
                Some(new_id) => match st.msg(&new_id) {
                    Some(m) => created(st.msg_json(m, None, false)),
                    None => not_found(),
                },
                None => not_found(),
            }
        }
        ("POST", ["me", "messages", id, action @ ("createReply" | "createReplyAll" | "createForward")]) => create_reply(st, id, action),
        ("POST", ["me", "messages", id, "send"]) => send(st, id),
        ("GET", ["me", "messages", id, "attachments"]) => {
            let Some(m) = st.msg(id) else { return not_found() };
            let list = m.attachments.iter().map(|a| Value::Object(apply_select(att_json(a, false), select))).collect();
            paged(list, &q, &format!("{graph}/me/messages/{id}/attachments"))
        }
        ("POST", ["me", "messages", id, "attachments"]) => {
            let a = Att {
                id: st.new_id("AAMkAtt"),
                name: body_json["name"].as_str().unwrap_or("attachment").to_string(),
                content_type: body_json["contentType"].as_str().unwrap_or("application/octet-stream").to_string(),
                bytes: base64::engine::general_purpose::STANDARD.decode(body_json["contentBytes"].as_str().unwrap_or("")).unwrap_or_default(),
                inline: body_json["isInline"].as_bool().unwrap_or(false),
                content_id: body_json["contentId"].as_str().map(str::to_string),
            };
            let seq = st.bump();
            let Some(m) = st.msg_mut(id) else { return not_found() };
            m.seq = seq;
            let v = att_json(&a, false);
            m.attachments.push(a);
            created(Value::Object(v))
        }
        ("POST", ["me", "messages", id, "attachments", "createUploadSession"]) => {
            if st.msg(id).is_none() {
                return not_found();
            }
            let item = &body_json["AttachmentItem"];
            let sid = st.new_id("up");
            st.uploads.insert(sid.clone(), Upload {
                message: id.to_string(),
                name: item["name"].as_str().unwrap_or("attachment").to_string(),
                size: item["size"].as_u64().unwrap_or(0) as usize,
                content_type: item["contentType"].as_str().unwrap_or("application/octet-stream").to_string(),
                inline: item["isInline"].as_bool().unwrap_or(false),
                content_id: item["contentId"].as_str().map(str::to_string),
                data: Vec::new(),
            });
            created(json!({"uploadUrl": format!("{root}/upload/{sid}"), "nextExpectedRanges": ["0-"]}))
        }
        ("GET", ["me", "messages", id, "attachments", aid]) => {
            let Some(a) = st.msg(id).and_then(|m| m.attachments.iter().find(|a| a.id == *aid)) else { return not_found() };
            ok(Value::Object(att_json(a, true)))
        }
        ("GET", ["me", "messages", id, "attachments", aid, "$value"]) => {
            let Some(a) = st.msg(id).and_then(|m| m.attachments.iter().find(|a| a.id == *aid)) else { return not_found() };
            Reply { status: 200, body: a.bytes.clone(), content_type: "application/octet-stream", headers: vec![] }
        }

        // ---- calendar
        ("GET", ["me", "calendars"]) => paged(st.calendars.clone(), &q, &format!("{graph}/me/calendars")),
        ("GET", ["me", "calendars", c, "calendarView"]) => {
            let (Some(from), Some(to)) = (q.get("startDateTime").and_then(parse_time), q.get("endDateTime").and_then(parse_time)) else {
                return err(400, "ErrorInvalidParameter", "startDateTime and endDateTime are required.");
            };
            let mut list: Vec<&Ev> = st
                .events
                .iter()
                .filter(|e| e.calendar == *c)
                .filter(|e| {
                    let s = e.props["start"]["dateTime"].as_str().and_then(parse_time);
                    let en = e.props["end"]["dateTime"].as_str().and_then(parse_time);
                    matches!((s, en), (Some(s), Some(en)) if s < to && en > from)
                })
                .collect();
            list.sort_by(|a, b| a.props["start"]["dateTime"].as_str().cmp(&b.props["start"]["dateTime"].as_str()));
            let vals = list.iter().map(|e| st.event_json(e, select)).collect();
            paged(vals, &q, &format!("{graph}/me/calendars/{c}/calendarView"))
        }
        ("POST", ["me", "calendars", c, "events"]) => {
            if !st.calendars.iter().any(|x| x["id"] == json!(c)) {
                return not_found();
            }
            let id = st.new_id("AAMkEv");
            let mut props = Map::new();
            props.insert("organizer".into(), st.me());
            props.insert("isOrganizer".into(), json!(true));
            props.insert("responseStatus".into(), json!({"response": "organizer"}));
            props.insert("type".into(), json!("singleInstance"));
            props.insert("showAs".into(), json!("busy"));
            props.insert("isCancelled".into(), json!(false));
            props.insert("attendees".into(), json!([]));
            props.insert("categories".into(), json!([]));
            let mut e = Ev { id, calendar: c.to_string(), props, body: String::new() };
            patch_event(&mut e, &body_json);
            let v = st.event_json(&e, None);
            st.events.push(e);
            created(v)
        }
        ("GET", ["me", "events", id]) => match st.events.iter().find(|e| e.id == *id) {
            Some(e) => ok(st.event_json(e, select)),
            None => not_found(),
        },
        ("PATCH", ["me", "events", id]) => {
            let Some(e) = st.events.iter_mut().find(|e| e.id == *id) else { return not_found() };
            patch_event(e, &body_json);
            let e = e.clone();
            ok(st.event_json(&e, None))
        }
        ("DELETE", ["me", "events", id]) => {
            let before = st.events.len();
            st.events.retain(|e| e.id != *id);
            if st.events.len() < before { no_content() } else { not_found() }
        }
        ("POST", ["me", "events", id, action @ ("accept" | "tentativelyAccept" | "decline")]) => {
            let Some(e) = st.events.iter_mut().find(|e| e.id == *id) else { return not_found() };
            if e.props.get("isOrganizer") == Some(&json!(true)) {
                return err(400, "ErrorInvalidRequest", "Your request can't be completed. You can't respond to a meeting you organized.");
            }
            let response = match *action {
                "accept" => "accepted",
                "tentativelyAccept" => "tentativelyAccepted",
                _ => "declined",
            };
            e.props.insert("responseStatus".into(), json!({"response": response, "time": now_iso()}));
            e.props.insert("showAs".into(), json!(if response == "tentativelyAccepted" { "tentative" } else { "busy" }));
            if response == "declined" {
                st.events.retain(|e| e.id != *id);
            }
            // The meeting request has done its job: it goes to Deleted Items.
            let requests: Vec<String> = st.messages.iter().filter(|m| m.event_id.as_deref() == Some(*id) && m.meeting_type.is_some()).map(|m| m.id.clone()).collect();
            if let Some(deleted) = st.folder_id("deleteditems") {
                for r in requests {
                    st.move_msg(&r, &deleted);
                }
            }
            if body_json["sendResponse"].as_bool().unwrap_or(true) {
                st.sent.push(json!({"response": response, "event": id, "comment": body_json["comment"]}));
            }
            accepted()
        }
        ("POST", ["me", "events", id, "cancel"]) => {
            let Some(e) = st.events.iter().find(|e| e.id == *id) else { return not_found() };
            if e.props.get("isOrganizer") != Some(&json!(true)) {
                return err(400, "ErrorAccessDenied", "Only the organizer can cancel the meeting.");
            }
            st.sent.push(json!({"cancelled": id, "comment": body_json["comment"]}));
            st.events.retain(|e| e.id != *id);
            accepted()
        }

        // ---- people
        ("GET", ["me", "people"]) => {
            let needle = q.get("$search").unwrap_or("").trim_matches('"').to_lowercase();
            let list: Vec<Value> = st
                .people
                .iter()
                .filter(|p| needle.is_empty() || p.to_string().to_lowercase().contains(&needle))
                .take(q.get("$top").and_then(|t| t.parse().ok()).unwrap_or(10))
                .cloned()
                .collect();
            ok(json!({"value": list}))
        }
        _ => err(400, "BadRequest", &format!("mockgraph doesn't know {method} {}", u.path())),
    }
}

fn percent_decode(s: &str) -> String {
    url::form_urlencoded::parse(format!("x={}", s.replace('+', "%2B")).as_bytes()).next().map(|(_, v)| v.to_string()).unwrap_or_else(|| s.to_string())
}

fn now_iso() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn received(m: &Msg) -> String {
    m.props.get("receivedDateTime").and_then(Value::as_str).unwrap_or("").to_string()
}

fn parse_time(s: &str) -> Option<chrono::NaiveDateTime> {
    let s = s.trim().trim_end_matches('Z');
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f").or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M")).ok()
}

fn matches_search(m: &Msg, needle: &str) -> bool {
    let hay = format!(
        "{} {} {} {}",
        m.props.get("subject").and_then(Value::as_str).unwrap_or(""),
        m.props.get("from").map(Value::to_string).unwrap_or_default(),
        m.props.get("toRecipients").map(Value::to_string).unwrap_or_default(),
        m.body
    )
    .to_lowercase();
    needle.split_whitespace().all(|w| hay.contains(w))
}

fn att_json(a: &Att, with_content: bool) -> Map<String, Value> {
    let mut o = Map::new();
    o.insert("@odata.type".into(), json!("#microsoft.graph.fileAttachment"));
    o.insert("id".into(), json!(a.id));
    o.insert("name".into(), json!(a.name));
    o.insert("contentType".into(), json!(a.content_type));
    o.insert("size".into(), json!(a.bytes.len()));
    o.insert("isInline".into(), json!(a.inline));
    if with_content {
        o.insert("contentId".into(), json!(a.content_id));
        o.insert("contentBytes".into(), json!(base64::engine::general_purpose::STANDARD.encode(&a.bytes)));
    }
    o
}

/// `$top`/`$skip` paging with a nextLink.
fn paged(list: Vec<Value>, q: &Query, base: &str) -> Reply {
    let top = q.get("$top").and_then(|t| t.parse::<usize>().ok()).unwrap_or(10).max(1);
    let skip = q.get("$skip").and_then(|t| t.parse::<usize>().ok()).unwrap_or(0);
    let page: Vec<Value> = list.iter().skip(skip).take(top).cloned().collect();
    let mut v = json!({"value": page});
    if skip + top < list.len() {
        let mut u = match url::Url::parse(base) {
            Ok(u) => u,
            Err(_) => return ok(v),
        };
        {
            let mut qp = u.query_pairs_mut();
            for (k, val) in &q.0 {
                if k != "$skip" {
                    qp.append_pair(k, val);
                }
            }
            qp.append_pair("$skip", &(skip + top).to_string());
        }
        v["@odata.nextLink"] = json!(u.to_string());
    }
    ok(v)
}

/// A delta round over one folder. Tokens are change counters: `$deltatoken=N` returns what changed
/// after N; `$skiptoken=N_offset_snapshot` continues a round.
fn delta(st: &State, graph: &str, folder: &str, q: &Query, prefer: &str) -> Reply {
    let page_size = prefer
        .split(',')
        .find_map(|p| p.trim().strip_prefix("odata.maxpagesize="))
        .and_then(|n| n.trim().parse::<usize>().ok())
        .unwrap_or(10)
        .clamp(1, 1000);
    let (since, offset, snapshot, filter) = if let Some(skip) = q.get("$skiptoken") {
        let parts: Vec<&str> = skip.split('_').collect();
        let n = |i: usize| parts.get(i).and_then(|p| p.parse::<u64>().ok()).unwrap_or(0);
        (n(0), n(1) as usize, n(2), parts.get(3).map(|s| s.to_string()).filter(|s| !s.is_empty()))
    } else if let Some(tok) = q.get("$deltatoken") {
        let parts: Vec<&str> = tok.split('_').collect();
        (parts.first().and_then(|p| p.parse().ok()).unwrap_or(0), 0, st.seq, parts.get(1).map(|s| s.to_string()).filter(|s| !s.is_empty()))
    } else {
        let filter = q.get("$filter").and_then(|f| f.split_whitespace().last()).map(str::to_string);
        (0, 0, st.seq, filter)
    };
    let select = q.get("$select");
    let mut items: Vec<Value> = Vec::new();
    let mut changed: Vec<&Msg> =
        st.messages.iter().filter(|m| m.folder == folder && m.seq > since && m.seq <= snapshot).filter(|m| filter.as_deref().is_none_or(|f| received(m).as_str() >= f)).collect();
    changed.sort_by(|a, b| received(b).cmp(&received(a)));
    items.extend(changed.iter().map(|m| st.msg_json(m, select, false)));
    if since > 0 {
        for (f, id, seq) in &st.tombstones {
            if f == folder && *seq > since && *seq <= snapshot && !st.messages.iter().any(|m| &m.id == id && m.folder == folder) {
                items.push(json!({"id": id, "@removed": {"reason": "deleted"}}));
            }
        }
    }
    let page: Vec<Value> = items.iter().skip(offset).take(page_size).cloned().collect();
    let mut v = json!({"value": page});
    let f = filter.unwrap_or_default();
    if offset + page_size < items.len() {
        v["@odata.nextLink"] = json!(format!("{graph}/me/mailFolders/{folder}/messages/delta?$skiptoken={since}_{}_{snapshot}_{f}", offset + page_size));
    } else {
        v["@odata.deltaLink"] = json!(format!("{graph}/me/mailFolders/{folder}/messages/delta?$deltatoken={snapshot}_{f}"));
    }
    ok(v)
}

fn create_folder(st: &mut State, parent: Option<String>, body: &Value) -> Reply {
    let name = body["displayName"].as_str().unwrap_or("").trim().to_string();
    if name.is_empty() {
        return err(400, "ErrorInvalidParameter", "displayName is required.");
    }
    if st.folders.iter().any(|f| f.parent == parent && f.name.eq_ignore_ascii_case(&name)) {
        return err(409, "ErrorFolderExists", "A folder with the specified name already exists.");
    }
    let f = Folder { id: st.new_id("AAMkFld"), name, parent, well_known: None, hidden: false };
    let v = st.folder_json(&f);
    st.folders.push(f);
    created(v)
}

fn patch_message(m: &mut Msg, body: &Value) {
    let Some(o) = body.as_object() else { return };
    for (k, v) in o {
        match k.as_str() {
            "body" => m.body = v["content"].as_str().unwrap_or("").to_string(),
            "id" | "parentFolderId" | "hasAttachments" => {}
            "flag" => {
                m.props.insert("flag".into(), json!({"flagStatus": v["flagStatus"].as_str().unwrap_or("notFlagged")}));
            }
            _ => {
                m.props.insert(k.clone(), v.clone());
            }
        }
    }
    if o.contains_key("body") {
        let text = strip_tags(&m.body);
        m.props.insert("bodyPreview".into(), json!(text.chars().take(255).collect::<String>()));
    }
    m.props.insert("lastModifiedDateTime".into(), json!(now_iso()));
}

fn patch_event(e: &mut Ev, body: &Value) {
    let Some(o) = body.as_object() else { return };
    for (k, v) in o {
        match k.as_str() {
            "body" => e.body = v["content"].as_str().unwrap_or("").to_string(),
            "id" => {}
            "start" | "end" => {
                // Stored in UTC, as `Prefer: outlook.timezone="UTC"` asks for.
                let dt = v["dateTime"].as_str().and_then(parse_time).map(|t| t.format("%Y-%m-%dT%H:%M:%S.0000000").to_string()).unwrap_or_default();
                e.props.insert(k.clone(), json!({"dateTime": dt, "timeZone": "UTC"}));
            }
            "isOnlineMeeting" => {
                e.props.insert(k.clone(), v.clone());
                if v.as_bool() == Some(true) {
                    e.props.insert("onlineMeeting".into(), json!({"joinUrl": format!("https://meet.harbourview.example/j/{}", e.id.len() * 7919)}));
                } else {
                    e.props.remove("onlineMeeting");
                }
            }
            _ => {
                e.props.insert(k.clone(), v.clone());
            }
        }
    }
    if o.contains_key("body") {
        e.props.insert("bodyPreview".into(), json!(strip_tags(&e.body).chars().take(255).collect::<String>()));
    }
}

fn strip_tags(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

fn recipients_line(v: &Value) -> String {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|r| {
                    let e = &r["emailAddress"];
                    e["name"].as_str().filter(|n| !n.is_empty()).or(e["address"].as_str()).unwrap_or("").to_string()
                })
                .collect::<Vec<_>>()
                .join("; ")
        })
        .unwrap_or_default()
}

fn create_reply(st: &mut State, id: &str, action: &str) -> Reply {
    let Some(orig) = st.msg(id).cloned() else { return not_found() };
    let Some(drafts) = st.folder_id("drafts") else { return not_found() };
    let subject = orig.props.get("subject").and_then(Value::as_str).unwrap_or("").to_string();
    let prefix = if action == "createForward" { "FW: " } else { "RE: " };
    let from = orig.props.get("from").cloned().unwrap_or(Value::Null);
    let owner = st.owner_address.clone();
    let me_addr = |r: &Value| r["emailAddress"]["address"].as_str().is_some_and(|a| a.eq_ignore_ascii_case(&owner));
    let (to, cc) = match action {
        "createReply" => (json!([from.clone()]), json!([])),
        "createReplyAll" => {
            let mut to = vec![from.clone()];
            if let Some(a) = orig.props.get("toRecipients").and_then(Value::as_array) {
                to.extend(a.iter().filter(|r| !me_addr(r)).cloned());
            }
            let cc: Vec<Value> = orig.props.get("ccRecipients").and_then(Value::as_array).map(|a| a.iter().filter(|r| !me_addr(r)).cloned().collect()).unwrap_or_default();
            (json!(to), json!(cc))
        }
        _ => (json!([]), json!([])),
    };
    let header = format!(
        "<div id=\"divRplyFwdMsg\"><hr style=\"display:inline-block;width:98%\"><b>From:</b> {}<br><b>Sent:</b> {}<br><b>To:</b> {}<br><b>Subject:</b> {}</div><br>",
        recipients_line(&json!([from])),
        orig.props.get("receivedDateTime").and_then(Value::as_str).unwrap_or(""),
        recipients_line(orig.props.get("toRecipients").unwrap_or(&Value::Null)),
        subject
    );
    let body = format!("<html><body><div><br></div>{header}{}</body></html>", orig.body);
    let mut props = Map::new();
    props.insert("subject".into(), json!(format!("{prefix}{subject}")));
    props.insert("toRecipients".into(), to);
    props.insert("ccRecipients".into(), cc);
    props.insert("isDraft".into(), json!(true));
    props.insert("isRead".into(), json!(true));
    props.insert("from".into(), st.me());
    props.insert("conversationId".into(), orig.props.get("conversationId").cloned().unwrap_or(Value::Null));
    props.insert("receivedDateTime".into(), json!(now_iso()));
    let new_id = st.new_id("AAMkDraft");
    let seq = st.bump();
    let attachments = if action == "createForward" {
        orig.attachments.iter().map(|a| Att { id: format!("{}f", a.id), ..a.clone() }).collect()
    } else {
        orig.attachments.iter().filter(|a| a.inline).cloned().collect()
    };
    let m = Msg { id: new_id, folder: drafts, seq, props, body, attachments, meeting_type: None, event_id: None };
    let v = st.msg_json(&m, None, false);
    st.messages.push(m);
    created(v)
}

fn send(st: &mut State, id: &str) -> Reply {
    let Some(m) = st.msg(id).cloned() else { return not_found() };
    if m.props.get("isDraft") != Some(&json!(true)) {
        return err(400, "ErrorInvalidOperation", "Only drafts can be sent.");
    }
    let no_recipients = ["toRecipients", "ccRecipients", "bccRecipients"].iter().all(|k| m.props.get(*k).and_then(Value::as_array).is_none_or(|a| a.is_empty()));
    if no_recipients {
        return err(400, "ErrorInvalidRecipients", "At least one recipient isn't valid., A message can't be sent because it contains no recipients.");
    }
    let Some(sent) = st.folder_id("sentitems") else { return not_found() };
    let now = now_iso();
    let to_me = ["toRecipients", "ccRecipients", "bccRecipients"]
        .iter()
        .any(|k| m.props.get(*k).and_then(Value::as_array).is_some_and(|a| a.iter().any(|r| r["emailAddress"]["address"].as_str().is_some_and(|x| x.eq_ignore_ascii_case(&st.owner_address)))));
    let snapshot = st.msg_json(&m, None, false);
    st.sent.push(snapshot);
    if let Some(new_id) = st.move_msg(id, &sent) {
        let seq = st.bump();
        if let Some(x) = st.msg_mut(&new_id) {
            x.props.insert("isDraft".into(), json!(false));
            x.props.insert("sentDateTime".into(), json!(now));
            x.props.insert("receivedDateTime".into(), json!(now));
            x.seq = seq;
        }
    }
    if to_me {
        let mut props = m.props.clone();
        props.insert("isDraft".into(), json!(false));
        props.insert("isRead".into(), json!(false));
        props.insert("receivedDateTime".into(), json!(now));
        props.insert("inferenceClassification".into(), json!("focused"));
        let body = m.body.clone();
        st.deliver("inbox", props, &body, m.attachments.clone());
    }
    accepted()
}

fn upload(st: &mut State, method: &str, segs: &[&str], range: Option<&str>, body: &[u8]) -> Reply {
    let (Some(sid), "PUT") = (segs.get(1), method) else { return not_found() };
    let Some(up) = st.uploads.get_mut(*sid) else { return not_found() };
    // Content-Range: bytes start-end/total
    let start = range.and_then(|r| r.trim_start_matches("bytes ").split('-').next().and_then(|s| s.parse::<usize>().ok())).unwrap_or(usize::MAX);
    if start != up.data.len() {
        return err(416, "InvalidRange", "The uploaded fragment overlaps or skips data.");
    }
    up.data.extend_from_slice(body);
    if up.data.len() < up.size {
        return Reply { status: 200, ..ok(json!({"nextExpectedRanges": [format!("{}-", up.data.len())]})) };
    }
    let Some(up) = st.uploads.remove(*sid) else { return not_found() };
    let a = Att { id: st.new_id("AAMkAtt"), name: up.name, content_type: up.content_type, bytes: up.data, inline: up.inline, content_id: up.content_id };
    let seq = st.bump();
    let Some(m) = st.msg_mut(&up.message) else { return not_found() };
    m.attachments.push(a);
    m.seq = seq;
    created(json!({}))
}

fn login(st: &mut State, root: &str, method: &str, segs: &[&str], q: &Query, body: &[u8]) -> Reply {
    let form: HashMap<String, String> = url::form_urlencoded::parse(body).map(|(k, v)| (k.to_string(), v.to_string())).collect();
    match (method, segs.get(4).copied()) {
        ("GET", Some("authorize")) => {
            // A browser that signs straight in: back to the redirect URI with a code.
            let redirect = q.get("redirect_uri").unwrap_or("");
            let state = q.get("state").unwrap_or("");
            if q.get("code_challenge_method") != Some("S256") || q.get("code_challenge").is_none_or(str::is_empty) {
                return oauth_err("invalid_request", "PKCE is required.");
            }
            let code = format!("code-{}", st.new_id(""));
            st.codes.insert(code.clone());
            let Ok(mut u) = url::Url::parse(redirect) else { return oauth_err("invalid_request", "bad redirect_uri") };
            u.query_pairs_mut().append_pair("code", &code).append_pair("state", state);
            Reply { status: 302, body: vec![], content_type: "text/plain", headers: vec![("Location", u.to_string())] }
        }
        ("POST", Some("devicecode")) => {
            st.device_polls = 0;
            ok(json!({
                "user_code": "MCK-4821",
                "device_code": "dc-mock",
                "verification_uri": format!("{root}/devicelogin"),
                "expires_in": 900,
                "interval": 1,
                "message": format!("To sign in, open {root}/devicelogin and enter the code MCK-4821."),
            }))
        }
        ("POST", Some("token")) => match form.get("grant_type").map(String::as_str) {
            Some("authorization_code") => {
                let code = form.get("code").cloned().unwrap_or_default();
                if !st.codes.remove(&code) || form.get("code_verifier").is_none_or(String::is_empty) {
                    return oauth_err("invalid_grant", "AADSTS70008: The provided authorization code or refresh token has expired.");
                }
                ok(st.issue_tokens())
            }
            Some("refresh_token") => {
                let rt = form.get("refresh_token").cloned().unwrap_or_default();
                if !st.refresh_tokens.remove(&rt) {
                    return oauth_err("invalid_grant", "AADSTS700082: The refresh token has expired due to inactivity.");
                }
                ok(st.issue_tokens())
            }
            Some("urn:ietf:params:oauth:grant-type:device_code") => {
                st.device_polls += 1;
                if st.device_polls < 2 {
                    return oauth_err("authorization_pending", "AADSTS70016: OAuth 2.0 device flow error. Authorization is pending.");
                }
                ok(st.issue_tokens())
            }
            _ => oauth_err("unsupported_grant_type", "unsupported grant type"),
        },
        _ => not_found(),
    }
}
