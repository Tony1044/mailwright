#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unreachable)]
//! The Graph client against mockgraph: sign-in flows, token refresh, throttling, delta sync,
//! sending with attachments, and calendar operations.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use mailcraft_graph::auth;
use mailcraft_graph::client::{EventResponse, ReplyKind};
use mailcraft_graph::tokens::{MemoryTokenStore, TokenStore};
use mailcraft_graph::{AccountConfig, GraphClient, GraphError};
use mockgraph::MockGraph;
use serde_json::json;

const CLIENT_ID: &str = "1b2c3d4e-0000-1111-2222-333344445555";

fn cfg(mock: &MockGraph) -> AccountConfig {
    AccountConfig {
        client_id: CLIENT_ID.into(),
        tenant: "organizations".into(),
        login_base: Some(mock.login_base()),
        graph_base: Some(mock.graph_base()),
        ..Default::default()
    }
}

fn signed_in(mock: &MockGraph) -> GraphClient {
    let rt = mock.grant_refresh_token();
    let store: Arc<dyn TokenStore> = Arc::new(MemoryTokenStore::default());
    let mut c = cfg(mock);
    c.username = mockgraph::ME_ADDRESS.into();
    store.save(&c.username, &rt).unwrap();
    GraphClient::from_saved(c, store).unwrap()
}

#[test]
fn browser_sign_in_with_pkce_and_loopback() {
    let mock = MockGraph::start().unwrap();
    let c = cfg(&mock);
    // A "browser" that follows the sign-in page's redirect back to the loopback address.
    let browser = |url: &str| {
        let url = url.to_string();
        std::thread::spawn(move || {
            let _ = ureq::get(&url).call();
        });
        true
    };
    let tokens = auth::sign_in_with_browser(&c, &browser, Arc::new(AtomicBool::new(false))).unwrap();
    assert!(tokens.is_fresh());
    assert!(!tokens.refresh_token.is_empty());
    let mut client = GraphClient::new(c, tokens, Arc::new(MemoryTokenStore::default()));
    let me = client.me().unwrap();
    assert_eq!(me.mail.as_deref(), Some(mockgraph::ME_ADDRESS));
    client.set_user(mockgraph::ME_ADDRESS, "Sam Carter").unwrap();
    let log = mock.state().log.join("\n");
    assert!(log.contains("code_challenge_method=S256"), "{log}");
}

#[test]
fn device_code_sign_in() {
    let mock = MockGraph::start().unwrap();
    let c = cfg(&mock);
    let dc = auth::start_device_code(&c).unwrap();
    assert_eq!(dc.user_code, "MCK-4821");
    let t = auth::finish_device_code(&c, &dc, Arc::new(AtomicBool::new(false))).unwrap();
    assert!(t.is_fresh());
}

#[test]
fn expired_tokens_refresh_and_revoked_ones_ask_to_sign_in() {
    let mock = MockGraph::start().unwrap();
    let client = signed_in(&mock);
    client.me().unwrap();
    mock.expire_access_tokens();
    client.me().unwrap(); // 401, refresh, retry
    mock.expire_access_tokens();
    mock.revoke_refresh_tokens();
    assert!(matches!(client.me(), Err(GraphError::SignInRequired(_))));
}

#[test]
fn throttling_waits_and_retries() {
    let mock = MockGraph::start().unwrap();
    let client = signed_in(&mock);
    mock.state().throttle_next = 2;
    let started = std::time::Instant::now();
    client.mailbox_settings().unwrap();
    assert!(started.elapsed().as_secs_f32() >= 1.9);
}

#[test]
fn folder_tree_and_well_known_folders() {
    let mock = MockGraph::start().unwrap();
    let client = signed_in(&mock);
    let folders = client.all_folders().unwrap();
    let names: Vec<&str> = folders.iter().map(|f| f.display_name.as_str()).collect();
    for n in ["Inbox", "Projects", "Harbour Street fit-out", "Receipts", "Sent Items", "Deleted Items"] {
        assert!(names.contains(&n), "{names:?}");
    }
    // Depth-first: Inbox's children come straight after it.
    let inbox = names.iter().position(|n| *n == "Inbox").unwrap();
    assert_eq!(names[inbox + 1], "Projects");
    assert_eq!(client.well_known_folder("sentitems").unwrap().display_name, "Sent Items");
    let f = client.create_folder(Some("AAMkInbox"), "Suppliers").unwrap();
    client.rename_folder(&f.id, "Key suppliers").unwrap();
    assert!(client.all_folders().unwrap().iter().any(|x| x.display_name == "Key suppliers"));
    client.delete_folder(&f.id).unwrap();
    assert!(matches!(client.create_folder(None, "Inbox"), Err(GraphError::Api { status: 409, .. })));
}

#[test]
fn delta_rounds_page_and_report_changes() {
    let mock = MockGraph::start().unwrap();
    let client = signed_in(&mock);
    // First round, two pages of 100 at most: everything in the Inbox.
    let mut all = Vec::new();
    let mut link: Option<String> = None;
    let delta_link = loop {
        let page = client.messages_delta("AAMkInbox", link.as_deref(), None).unwrap();
        all.extend(page.changed);
        if let Some(d) = page.delta_link {
            break d;
        }
        link = page.next_link;
    };
    let inbox_count = mock.state().messages.iter().filter(|m| m.folder == "AAMkInbox").count();
    assert_eq!(all.len(), inbox_count);
    assert!(all.iter().all(|m| m.body.is_none()), "list sync carries no bodies");
    assert!(all.iter().any(|m| m.is_meeting_message()));

    // Nothing changed: nothing comes back.
    let page = client.messages_delta("AAMkInbox", Some(&delta_link), None).unwrap();
    assert!(page.changed.is_empty() && page.removed.is_empty());

    // Read one, move another, a new one arrives.
    let a = all[0].id.clone();
    let b = all[1].id.clone();
    client.set_read(&a, true).unwrap();
    let moved = client.move_message(&b, "deleteditems").unwrap();
    assert_ne!(moved.id, b);
    let new_id = mock.state().deliver("inbox", serde_json::Map::from_iter([("subject".to_string(), json!("Fresh"))]), "<p>hi</p>", vec![]);
    let page = client.messages_delta("AAMkInbox", Some(&delta_link), None).unwrap();
    let changed: Vec<&str> = page.changed.iter().map(|m| m.id.as_str()).collect();
    assert!(changed.contains(&a.as_str()) && changed.contains(&new_id.as_str()), "{changed:?}");
    assert_eq!(page.removed, vec![b]);
    assert_eq!(page.changed.iter().find(|m| m.id == a).unwrap().is_read, Some(true));
}

#[test]
fn initial_sync_window_filters_old_mail() {
    let mock = MockGraph::start().unwrap();
    let client = signed_in(&mock);
    let since = (chrono::Utc::now() - chrono::Duration::days(3)).format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let page = client.messages_delta("inbox", None, Some(&since)).unwrap();
    assert!(!page.changed.is_empty());
    assert!(page.changed.iter().all(|m| m.received_date_time.as_deref().unwrap_or("") >= since.as_str()));
    // The window is remembered across the round.
    assert!(page.delta_link.unwrap().contains(&since));
}

#[test]
fn bodies_attachments_and_meeting_events() {
    let mock = MockGraph::start().unwrap();
    let client = signed_in(&mock);
    let (fixture, meeting) = {
        let st = mock.state();
        let f = st.messages.iter().find(|m| m.attachments.iter().any(|a| a.name.ends_with(".csv"))).unwrap().id.clone();
        let m = st.messages.iter().find(|m| m.meeting_type.is_some()).unwrap().id.clone();
        (f, m)
    };
    let full = client.message_full(&fixture).unwrap();
    assert!(full.body.unwrap().content.contains("<table"));
    let atts = client.attachments(&fixture).unwrap();
    assert_eq!(atts.len(), 1);
    assert!(atts[0].content_bytes.is_none());
    let bytes = client.attachment_bytes(&fixture, &atts[0].id).unwrap();
    assert!(String::from_utf8_lossy(&bytes).starts_with("Region,Q1"));
    let one = client.attachment(&fixture, &atts[0].id).unwrap();
    assert!(one.content_bytes.is_some());

    let (kind, event) = client.meeting_event(&meeting).unwrap();
    assert_eq!(kind.as_deref(), Some("meetingRequest"));
    let event = event.unwrap();
    assert_eq!(event.subject.as_deref(), Some("Q4 budget review"));
    client.respond_event(&event.id, EventResponse::Tentative, "Might be late", true).unwrap();
    let e = client.event_full(&event.id).unwrap();
    assert_eq!(e.response_status.unwrap().response.as_deref(), Some("tentativelyAccepted"));
    assert!(matches!(client.message_full(&meeting), Err(e) if e.is_not_found()), "the request moves to Deleted Items");
}

#[test]
fn compose_reply_and_send_with_a_large_attachment() {
    let mock = MockGraph::start().unwrap();
    let client = signed_in(&mock);
    let draft = client
        .create_draft(&json!({
            "subject": "Test",
            "body": {"contentType": "html", "content": "<p>Hello</p>"},
            "toRecipients": [{"emailAddress": {"address": mockgraph::ME_ADDRESS}}],
        }))
        .unwrap();
    assert_eq!(draft.is_draft, Some(true));
    client.add_attachment(&draft.id, "small.txt", "text/plain", b"tiny", None).unwrap();
    let big: Vec<u8> = (0..4_500_000u32).map(|i| (i % 251) as u8).collect();
    client.add_attachment(&draft.id, "big.bin", "application/octet-stream", &big, None).unwrap();
    let atts = client.attachments(&draft.id).unwrap();
    assert_eq!(atts.iter().map(|a| a.size).sum::<i64>(), 4 + big.len() as i64);
    assert!(mock.state().log.iter().filter(|l| l.starts_with("PUT /upload/")).count() >= 2);
    client.send_draft(&draft.id).unwrap();
    assert_eq!(mock.state().sent.len(), 1);
    // Sent to self: a copy arrives in the Inbox.
    let st = mock.state();
    assert!(st.messages.iter().any(|m| m.folder == "AAMkInbox" && m.props["subject"] == json!("Test") && m.attachments.len() == 2));
    assert!(st.messages.iter().any(|m| m.folder == "AAMkSent" && m.props["subject"] == json!("Test")));
    drop(st);

    // A reply quotes the original and goes back to its sender.
    let orig = mock.state().messages.iter().find(|m| m.props["subject"] == json!("Quick question on the Q3 numbers")).unwrap().id.clone();
    let reply = client.create_reply(&orig, ReplyKind::Reply).unwrap();
    assert_eq!(reply.subject.as_deref(), Some("RE: Quick question on the Q3 numbers"));
    assert_eq!(reply.to_recipients[0].address(), "raj.patel@harbourview.example");
    assert!(reply.body.as_ref().unwrap().content.contains("Raj"));
    let html = format!("<p>Final, yes.</p>{}", reply.body.unwrap().content);
    client.update_draft(&reply.id, &json!({"body": {"contentType": "html", "content": html}})).unwrap();
    client.send_draft(&reply.id).unwrap();
    assert_eq!(mock.state().sent.len(), 2);
    let fwd = client.create_reply(&orig, ReplyKind::Forward).unwrap();
    assert!(fwd.to_recipients.is_empty());
    assert!(matches!(client.send_draft(&fwd.id), Err(GraphError::Api { status: 400, .. })));
}

#[test]
fn calendar_view_and_event_changes() {
    let mock = MockGraph::start().unwrap();
    let client = signed_in(&mock);
    let cals = client.calendars().unwrap();
    assert_eq!(cals.len(), 1);
    let cal = &cals[0].id;
    let now = chrono::Utc::now();
    let from = (now - chrono::Duration::days(8)).format("%Y-%m-%dT00:00:00Z").to_string();
    let to = (now + chrono::Duration::days(8)).format("%Y-%m-%dT00:00:00Z").to_string();
    let events = client.calendar_view(cal, &from, &to).unwrap();
    assert!(events.len() > 10, "{}", events.len());
    assert!(events.iter().all(|e| e.calendar_id == *cal));
    assert!(events.windows(2).all(|w| w[0].start.as_ref().unwrap().date_time <= w[1].start.as_ref().unwrap().date_time));

    let created = client
        .create_event(cal, &json!({
            "subject": "Dentist",
            "start": {"dateTime": "2026-10-28T08:00:00", "timeZone": "UTC"},
            "end": {"dateTime": "2026-10-28T09:00:00", "timeZone": "UTC"},
            "isOnlineMeeting": true,
        }))
        .unwrap();
    assert!(created.online_meeting.unwrap().join_url.is_some());
    let moved = client.update_event(&created.id, &json!({"subject": "Dentist (moved)", "start": {"dateTime": "2026-10-28T10:00:00", "timeZone": "UTC"}, "end": {"dateTime": "2026-10-28T11:00:00", "timeZone": "UTC"}})).unwrap();
    assert_eq!(moved.start.unwrap().date_time, "2026-10-28T10:00:00.0000000");
    assert!(matches!(client.respond_event(&created.id, EventResponse::Accept, "", true), Err(GraphError::Api { status: 400, .. })));
    client.cancel_event(&created.id, "Sorry").unwrap();
    assert!(matches!(client.event_full(&created.id), Err(e) if e.is_not_found()));
}

#[test]
fn search_and_people() {
    let mock = MockGraph::start().unwrap();
    let client = signed_in(&mock);
    let hits = client.search_messages("heads of terms", 25).unwrap();
    assert!(hits.iter().any(|m| m.subject.as_deref().unwrap_or("").contains("Lease renewal")));
    let people = client.people("ada").unwrap();
    assert_eq!(people[0].display_name.as_deref(), Some("Ada Lindqvist"));
}
