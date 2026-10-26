#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unreachable)]
//! The sync engine against mockgraph, end to end through the cache.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use mailwright_graph::tokens::{MemoryTokenStore, TokenStore};
use mailwright_graph::{AccountConfig, GraphClient};
use mailwright_store::{Action, Addr, Compose, ListFilter, OutAttachment, SortBy, Store};
use mailwright_sync::{Command, SyncHandle, Update};
use mockgraph::MockGraph;

struct Rig {
    // Fields drop in order: the engine stops before the mock server goes.
    sync: SyncHandle,
    rx: Receiver<Update>,
    mock: MockGraph,
    db: PathBuf,
    dir: PathBuf,
}

impl Rig {
    fn new(name: &str) -> Rig {
        let mock = MockGraph::start().unwrap();
        let dir = std::env::temp_dir().join(format!("mailwright-sync-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("cache.sqlite");
        let (sync, rx) = mailwright_sync::start(db.clone(), 90, Arc::new(|| {})).unwrap();
        let rig = Rig { sync, rx, mock, db, dir };
        let client = rig.client();
        rig.sync.send(Command::Attach(Box::new(client)));
        rig.wait(|u| matches!(u, Update::Status(s) if s.starts_with("All folders are up to date")));
        rig
    }

    fn client(&self) -> GraphClient {
        let cfg = AccountConfig {
            client_id: "1b2c3d4e-0000-1111-2222-333344445555".into(),
            username: mockgraph::ME_ADDRESS.into(),
            login_base: Some(self.mock.login_base()),
            graph_base: Some(self.mock.graph_base()),
            ..Default::default()
        };
        let store: Arc<dyn TokenStore> = Arc::new(MemoryTokenStore::default());
        store.save(mockgraph::ME_ADDRESS, &self.mock.grant_refresh_token()).unwrap();
        GraphClient::from_saved(cfg, store).unwrap()
    }

    fn wait(&self, want: impl Fn(&Update) -> bool) -> Vec<Update> {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut seen = Vec::new();
        while Instant::now() < deadline {
            if let Ok(u) = self.rx.recv_timeout(Duration::from_millis(200)) {
                let hit = want(&u);
                seen.push(u);
                if hit {
                    return seen;
                }
            }
        }
        panic!("timed out; saw {seen:#?}");
    }

    fn store(&self) -> Store {
        Store::open(&self.db).unwrap()
    }

    fn round(&self) {
        self.sync.send(Command::SyncAll);
        self.wait(|u| matches!(u, Update::Status(s) if s.starts_with("All folders are up to date")));
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn inbox(s: &Store) -> String {
    s.folder_by_well_known("inbox").unwrap().unwrap()
}

#[test]
fn first_round_fills_the_cache() {
    let rig = Rig::new("first");
    let s = rig.store();
    let names: Vec<String> = s.folders().unwrap().into_iter().map(|f| f.name).collect();
    assert!(names.contains(&"Harbour Street fit-out".to_string()), "{names:?}");
    let list = s.messages(&inbox(&s), ListFilter::All, SortBy::Date, false, "").unwrap();
    let server = rig.mock.state().messages.iter().filter(|m| m.folder == "AAMkInbox").count();
    assert_eq!(list.len(), server);
    assert!(list.windows(2).all(|w| w[0].received >= w[1].received));
    let unread = s.folders().unwrap().into_iter().find(|f| f.well_known.as_deref() == Some("inbox")).unwrap().unread;
    assert_eq!(unread as usize, list.iter().filter(|m| !m.is_read).count());
    assert!(!s.calendars().unwrap().is_empty());
    let now = chrono::Utc::now().timestamp();
    assert!(s.events(now - 7 * 86400, now + 7 * 86400).unwrap().len() > 5);
    // Quiet-time prefetch: recent bodies arrive without opening anything.
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && !s.missing_bodies(&[inbox(&s)], now - 14 * 86400, 100).unwrap().is_empty() {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(s.missing_bodies(&[inbox(&s)], now - 14 * 86400, 100).unwrap().is_empty());
}

#[test]
fn opening_a_message_fetches_body_inline_images_and_meeting() {
    let rig = Rig::new("open");
    let s = rig.store();
    let list = s.messages(&inbox(&s), ListFilter::All, SortBy::Date, false, "").unwrap();
    let news = list.iter().find(|m| m.subject.starts_with("This week")).unwrap();
    rig.sync.send(Command::Open(news.id.clone()));
    rig.wait(|u| matches!(u, Update::MessageReady(id) if *id == news.id));
    let imgs = s.inline_images(&news.id).unwrap();
    assert_eq!(imgs.len(), 1);
    assert_eq!(imgs[0].0, "banner001");
    assert!(imgs[0].2.starts_with(&[0x89, b'P', b'N', b'G']));

    let meeting = list.iter().find(|m| m.is_meeting).unwrap();
    rig.sync.send(Command::Open(meeting.id.clone()));
    rig.wait(|u| matches!(u, Update::MessageReady(id) if *id == meeting.id));
    let m = s.message(&meeting.id).unwrap().unwrap();
    assert_eq!(m.meeting_type.as_deref(), Some("meetingRequest"));
    let ev = s.event(m.event_id.as_deref().unwrap()).unwrap().unwrap();
    assert_eq!(ev.response, "notResponded");
    rig.sync.send(Command::Do(Action::RespondEvent { id: ev.id.clone(), response: "accept".into(), comment: String::new(), send: true }));
    rig.wait(|u| matches!(u, Update::Queued(0)));
    assert_eq!(s.event(&ev.id).unwrap().unwrap().response, "accepted");
    assert!(rig.mock.state().sent.iter().any(|v| v["response"] == "accepted"));
}

#[test]
fn changes_made_offline_reach_the_server_later() {
    let rig = Rig::new("offline");
    let s = rig.store();
    let inbox_id = inbox(&s);
    let deleted = s.folder_by_well_known("deleteditems").unwrap().unwrap();
    let list = s.messages(&inbox_id, ListFilter::All, SortBy::Date, false, "").unwrap();
    let a = list.iter().find(|m| !m.is_read).unwrap().clone();
    let b = list.iter().find(|m| m.id != a.id).unwrap().clone();

    rig.sync.send(Command::WorkOffline(true));
    rig.wait(|u| matches!(u, Update::Status(s) if s == "Working offline"));
    rig.sync.send(Command::Do(Action::SetRead { id: a.id.clone(), read: true }));
    rig.sync.send(Command::Do(Action::Move { id: b.id.clone(), dest: deleted.clone() }));
    rig.sync.send(Command::Do(Action::SetFlag { id: b.id.clone(), status: "flagged".into() }));
    rig.wait(|u| matches!(u, Update::Queued(3)));
    assert!(s.message(&a.id).unwrap().unwrap().is_read);
    assert_eq!(s.message(&b.id).unwrap().unwrap().folder_id, deleted);
    assert_eq!(rig.mock.state().msg(&a.id).unwrap().props["isRead"], false);

    rig.sync.send(Command::WorkOffline(false));
    rig.wait(|u| matches!(u, Update::Status(s) if s.starts_with("All folders are up to date")));
    assert_eq!(s.queued_count().unwrap(), 0);
    let st = rig.mock.state();
    assert_eq!(st.msg(&a.id).unwrap().props["isRead"], true);
    assert!(st.msg(&b.id).is_none(), "moved, so it has a new id");
    let moved = st.messages.iter().find(|m| m.folder == "AAMkDeleted" && m.props["subject"] == b.subject.as_str()).unwrap();
    assert_eq!(moved.props["flag"]["flagStatus"], "flagged", "the flag followed the new id");
    let moved_id = moved.id.clone();
    drop(st);
    // And the cache agrees, with no duplicates after the next delta.
    rig.round();
    assert!(s.message(&b.id).unwrap().is_none());
    assert_eq!(s.message(&moved_id).unwrap().unwrap().folder_id, deleted);
    assert_eq!(s.messages(&deleted, ListFilter::All, SortBy::Date, false, "").unwrap().iter().filter(|m| m.subject == b.subject).count(), 1);
}

#[test]
fn new_mail_and_server_side_changes_arrive() {
    let rig = Rig::new("newmail");
    let s = rig.store();
    let inbox_id = inbox(&s);
    let before = s.messages(&inbox_id, ListFilter::All, SortBy::Date, false, "").unwrap();
    let victim = before[3].id.clone();
    {
        let mut st = rig.mock.state();
        let props = serde_json::Map::from_iter([
            ("subject".to_string(), serde_json::json!("Brand new")),
            ("receivedDateTime".to_string(), serde_json::json!(chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string())),
            ("isRead".to_string(), serde_json::json!(false)),
        ]);
        st.deliver("inbox", props, "<p>Fresh mail</p>", vec![]);
        st.remove_msg(&victim);
    }
    rig.round();
    let after = s.messages(&inbox_id, ListFilter::All, SortBy::Date, false, "").unwrap();
    assert_eq!(after[0].subject, "Brand new");
    assert!(!after.iter().any(|m| m.id == victim));
    assert_eq!(after.len(), before.len());
}

#[test]
fn sending_replies_and_attachments() {
    let rig = Rig::new("send");
    let s = rig.store();
    let list = s.messages(&inbox(&s), ListFilter::All, SortBy::Date, false, "").unwrap();
    let orig = list.iter().find(|m| m.subject == "Quick question on the Q3 numbers").unwrap();
    let compose = Compose {
        local_id: "local:reply-1".into(),
        reply_to: Some((orig.id.clone(), "reply".into())),
        subject: format!("RE: {}", orig.subject),
        to: vec![orig.from.clone()],
        cc: vec![Addr::new("Sam Carter", mockgraph::ME_ADDRESS)],
        html: "<p>They're final.</p><hr><p>quoted</p>".into(),
        importance: "high".into(),
        attachments: vec![OutAttachment { name: "note.txt".into(), content_type: "text/plain".into(), data: "aGVsbG8=".into(), content_id: None }],
        ..Default::default()
    };
    rig.sync.send(Command::Do(Action::Send { compose }));
    rig.wait(|u| matches!(u, Update::Sent(_)));
    let sent = rig.mock.state().sent.clone();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["subject"], "RE: Quick question on the Q3 numbers");
    assert_eq!(sent[0]["importance"], "high");
    assert_eq!(sent[0]["toRecipients"][0]["emailAddress"]["address"], "raj.patel@harbourview.example");
    assert!(sent[0]["body"]["content"].as_str().unwrap().starts_with("<p>They're final."));
    rig.round();
    let outbox = s.folder_by_well_known("outbox").unwrap().unwrap();
    assert!(s.messages(&outbox, ListFilter::All, SortBy::Date, false, "").unwrap().is_empty());
    let sentitems = s.folder_by_well_known("sentitems").unwrap().unwrap();
    assert!(s.messages(&sentitems, ListFilter::All, SortBy::Date, false, "").unwrap().iter().any(|m| m.subject.starts_with("RE: Quick question")));
    // Cc'd to me: it arrives with its attachment.
    let back = s.messages(&inbox(&s), ListFilter::All, SortBy::Date, false, "").unwrap().into_iter().find(|m| m.subject.starts_with("RE: Quick question")).unwrap();
    assert!(back.has_attachments);

    // A message with no recipients is refused and stays in the Outbox.
    let bad = Compose { local_id: "local:bad".into(), subject: "Nobody".into(), html: "<p>x</p>".into(), ..Default::default() };
    rig.sync.send(Command::Do(Action::Send { compose: bad }));
    rig.wait(|u| matches!(u, Update::Error(e) if e.contains("still in your Outbox")));
    assert_eq!(s.message("local:bad").unwrap().unwrap().folder_id, outbox);
}

#[test]
fn people_search_and_calendar_edits() {
    let rig = Rig::new("people");
    rig.sync.send(Command::People("mei".into()));
    let seen = rig.wait(|u| matches!(u, Update::People(..)));
    let Some(Update::People(_, list)) = seen.last() else { unreachable!() };
    assert_eq!(list[0].address, "mei.tanaka@harbourview.example");

    rig.sync.send(Command::SearchServer("heads of terms".into()));
    let seen = rig.wait(|u| matches!(u, Update::SearchResults(..)));
    let Some(Update::SearchResults(_, ids)) = seen.last() else { unreachable!() };
    assert!(!ids.is_empty());

    let s = rig.store();
    let cal = s.default_calendar().unwrap().unwrap();
    rig.sync.send(Command::Do(Action::CreateEvent {
        local_id: "local:ev1".into(),
        calendar_id: cal.clone(),
        body: serde_json::json!({"subject": "Planning", "start": {"dateTime": "2026-11-03T10:00:00", "timeZone": "UTC"}, "end": {"dateTime": "2026-11-03T11:00:00", "timeZone": "UTC"}}),
    }));
    rig.wait(|u| matches!(u, Update::Queued(0)));
    let t = |x: &str| mailwright_store::parse_time(x).unwrap();
    let evs = s.events(t("2026-11-03T00:00:00Z"), t("2026-11-04T00:00:00Z")).unwrap();
    let planning = evs.iter().find(|e| e.subject == "Planning").unwrap();
    assert!(!planning.local);
    rig.sync.send(Command::Do(Action::UpdateEvent { id: planning.id.clone(), patch: serde_json::json!({"subject": "Planning (moved)", "start": {"dateTime": "2026-11-03T14:00:00", "timeZone": "UTC"}, "end": {"dateTime": "2026-11-03T15:00:00", "timeZone": "UTC"}}) }));
    rig.wait(|u| matches!(u, Update::Queued(0)));
    let e = s.event(&planning.id).unwrap().unwrap();
    assert_eq!((e.subject.as_str(), e.start), ("Planning (moved)", t("2026-11-03T14:00:00Z")));
    rig.sync.send(Command::Do(Action::CancelEvent { id: planning.id.clone(), comment: "Not needed".into() }));
    rig.wait(|u| matches!(u, Update::Queued(0)));
    assert!(s.event(&planning.id).unwrap().is_none());
}

#[test]
fn a_revoked_sign_in_asks_to_sign_in_again() {
    let rig = Rig::new("revoked");
    rig.mock.expire_access_tokens();
    rig.mock.revoke_refresh_tokens();
    rig.sync.send(Command::SyncAll);
    rig.wait(|u| matches!(u, Update::SignInRequired(_)));
}

#[test]
fn a_change_the_server_keeps_refusing_never_holds_up_new_mail() {
    let rig = Rig::new("stuck");
    let s = rig.store();
    let inbox_id = inbox(&s);
    let first = s.messages(&inbox_id, ListFilter::All, SortBy::Date, false, "").unwrap()[0].id.clone();
    rig.mock.state().refuse_message_changes = true;
    rig.sync.send(Command::Do(Action::SetFlag { id: first, status: "flagged".into() }));
    {
        let mut st = rig.mock.state();
        let props = serde_json::Map::from_iter([
            ("subject".to_string(), serde_json::json!("Arrives anyway")),
            ("from".to_string(), serde_json::json!({"emailAddress": {"name": "Sam Lee", "address": "sam@example.com"}})),
            ("receivedDateTime".to_string(), serde_json::json!(chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string())),
            ("isRead".to_string(), serde_json::json!(false)),
        ]);
        st.deliver("inbox", props, "<p>Hello</p>", vec![]);
    }
    rig.sync.send(Command::SyncAll);
    let seen = rig.wait(|u| matches!(u, Update::NewMail(_)));
    let Some(Update::NewMail(list)) = seen.last() else { unreachable!() };
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].subject, "Arrives anyway");
    assert_eq!(list[0].from, "Sam Lee");
    assert!(s.messages(&inbox_id, ListFilter::All, SortBy::Date, false, "").unwrap().iter().any(|m| m.subject == "Arrives anyway"));
    assert_eq!(s.queued_count().unwrap(), 1, "the refused change waits for a later round");
    // Once the server takes changes again, it goes through.
    rig.mock.state().refuse_message_changes = false;
    rig.sync.send(Command::SyncAll);
    rig.wait(|u| matches!(u, Update::Queued(0)));
    assert_eq!(s.queued_count().unwrap(), 0);
}

#[test]
fn a_message_reported_as_removed_then_changed_stays() {
    let rig = Rig::new("readstays");
    let s = rig.store();
    let inbox_id = inbox(&s);
    let target = s.messages(&inbox_id, ListFilter::All, SortBy::Date, false, "").unwrap().into_iter().find(|m| !m.is_read).unwrap();
    rig.mock.state().updates_as_remove_then_add = true;
    rig.sync.send(Command::Do(Action::SetRead { id: target.id.clone(), read: true }));
    rig.wait(|u| matches!(u, Update::Queued(0)));
    rig.round();
    rig.round();
    let after = s.message(&target.id).unwrap().expect("a message that was only read is still there");
    assert!(after.is_read);
    assert_eq!(after.folder_id, inbox_id);
}

#[test]
fn the_inbox_check_finds_mail_delta_missed() {
    let rig = Rig::new("inboxcheck");
    let s = rig.store();
    let inbox_id = inbox(&s);
    let id = {
        let mut st = rig.mock.state();
        let props = serde_json::Map::from_iter([
            ("subject".to_string(), serde_json::json!("Delta never said")),
            ("from".to_string(), serde_json::json!({"emailAddress": {"name": "Royal Mail", "address": "no-reply@royalmail.example"}})),
            ("receivedDateTime".to_string(), serde_json::json!(chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string())),
            ("isRead".to_string(), serde_json::json!(false)),
        ]);
        let id = st.deliver("inbox", props, "<p>Parcel</p>", vec![]);
        st.hidden_from_delta.insert(id.clone());
        id
    };
    rig.sync.send(Command::SyncAll);
    let seen = rig.wait(|u| matches!(u, Update::NewMail(_)));
    let Some(Update::NewMail(list)) = seen.last() else { unreachable!() };
    assert_eq!(list[0].subject, "Delta never said");
    assert_eq!(s.message(&id).unwrap().unwrap().folder_id, inbox_id);
}
