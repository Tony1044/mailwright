use super::*;
use mailcraft_graph::types::{EmailAddress, FollowupFlag, ItemBody, Recipient};

fn folder(id: &str, name: &str, parent: Option<&str>) -> MailFolder {
    MailFolder { id: id.into(), display_name: name.into(), parent_folder_id: parent.map(str::to_string), total_item_count: 0, ..Default::default() }
}

fn msg(id: &str, subject: &str, from: &str, received: &str, read: bool) -> Message {
    Message {
        id: id.into(),
        subject: Some(subject.into()),
        body_preview: Some(format!("preview of {subject}")),
        from: Some(Recipient { email_address: EmailAddress { name: Some(from.into()), address: Some(format!("{}@x.example", from.to_lowercase())) } }),
        to_recipients: vec![Recipient::new("Sam Carter", "sam@x.example")],
        received_date_time: Some(received.into()),
        is_read: Some(read),
        flag: Some(FollowupFlag { flag_status: "notFlagged".into() }),
        ..Default::default()
    }
}

fn store() -> Store {
    let mut s = Store::open_in_memory().unwrap();
    s.save_folders(&[folder("in", "Inbox", Some("root")), folder("sub", "Projects", Some("in")), folder("del", "Deleted Items", None)], &[("inbox".into(), "in".into()), ("deleteditems".into(), "del".into())]).unwrap();
    s.set_delta_link("in", Some("d1")).unwrap();
    s.set_delta_link("del", Some("d2")).unwrap();
    s
}

#[test]
fn folders_keep_order_parents_and_delta_links() {
    let mut s = store();
    let f = s.folders().unwrap();
    assert_eq!(f.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["Inbox", "Projects", "Deleted Items"]);
    assert_eq!(f[0].parent_id, None, "a parent outside the list (the root) is dropped");
    assert_eq!(f[1].parent_id.as_deref(), Some("in"));
    assert_eq!(f[0].well_known.as_deref(), Some("inbox"));
    assert!(f[0].synced && !f[1].synced);
    // Saving again keeps delta links; a folder that went away takes its mail with it.
    s.upsert_messages("sub", &[msg("m1", "Hello", "Ada", "2026-10-20T10:00:00Z", true)]).unwrap();
    s.save_folders(&[folder("in", "Inbox", None), folder("del", "Deleted Items", None)], &[("inbox".into(), "in".into())]).unwrap();
    assert_eq!(s.delta_link("in").unwrap().as_deref(), Some("d1"));
    assert!(s.message("m1").unwrap().is_none());
}

#[test]
fn messages_list_sort_filter_and_search() {
    let mut s = store();
    let mut a = msg("a", "Lease renewal – heads of terms", "Owen", "2026-10-22T09:00:00Z", false);
    a.flag = Some(FollowupFlag { flag_status: "flagged".into() });
    a.has_attachments = Some(true);
    let mut b = msg("b", "Lunch on Thursday?", "Priya", "2026-10-21T09:00:00Z", true);
    b.inference_classification = Some("other".into());
    let c = msg("c", "Board pack", "Raj", "2026-10-23T09:00:00Z", true);
    s.upsert_messages("in", &[a, b, c]).unwrap();
    let ids = |v: Vec<MessageRow>| v.into_iter().map(|m| m.id).collect::<Vec<_>>();
    assert_eq!(ids(s.messages("in", ListFilter::All, SortBy::Date, false, "").unwrap()), ["c", "a", "b"]);
    assert_eq!(ids(s.messages("in", ListFilter::All, SortBy::From, true, "").unwrap()), ["a", "b", "c"]);
    assert_eq!(ids(s.messages("in", ListFilter::Unread, SortBy::Date, false, "").unwrap()), ["a"]);
    assert_eq!(ids(s.messages("in", ListFilter::Flagged, SortBy::Date, false, "").unwrap()), ["a"]);
    assert_eq!(ids(s.messages("in", ListFilter::Other, SortBy::Date, false, "").unwrap()), ["b"]);
    assert_eq!(ids(s.messages("in", ListFilter::ToMe, SortBy::Date, false, "sam@x.example").unwrap()).len(), 3);
    assert_eq!(ids(s.search("heads", None, 10).unwrap()), ["a"]);
    assert_eq!(ids(s.search("priya", Some("in"), 10).unwrap()), ["b"]);
    assert!(s.search("priya", Some("del"), 10).unwrap().is_empty());
    assert!(s.search("\"); DROP TABLE messages; --", None, 10).unwrap().is_empty());

    // The body, once fetched, is searchable too.
    let mut full = msg("c", "Board pack", "Raj", "2026-10-23T09:00:00Z", true);
    full.body = Some(ItemBody { content_type: "html".into(), content: "<style>.zqstyle{}</style><p>Quarterly r\u{e9}sum\u{e9} &amp; numbers</p>".into() });
    s.save_body(&full).unwrap();
    assert_eq!(ids(s.search("resume", None, 10).unwrap()), ["c"], "diacritics folded");
    assert!(s.search("zqstyle", None, 10).unwrap().is_empty(), "styles aren't indexed");
    assert!(s.message("c").unwrap().unwrap().body.unwrap().contains("Quarterly"));
    // A delta update with the same preview keeps the body; a changed one drops it.
    s.upsert_messages("in", &[msg("c", "Board pack", "Raj", "2026-10-23T09:00:00Z", false)]).unwrap();
    assert!(s.message("c").unwrap().unwrap().body.is_some());
    let mut changed = msg("c", "Board pack", "Raj", "2026-10-23T09:00:00Z", false);
    changed.body_preview = Some("new".into());
    s.upsert_messages("in", &[changed]).unwrap();
    assert!(s.message("c").unwrap().unwrap().body.is_none());
}

#[test]
fn local_changes_queue_and_win_over_sync_until_done() {
    let mut s = store();
    s.upsert_messages("in", &[msg("a", "One", "Ada", "2026-10-22T09:00:00Z", false), msg("b", "Two", "Raj", "2026-10-22T08:00:00Z", false)]).unwrap();
    s.recount("in").unwrap();
    assert_eq!(s.folders().unwrap()[0].unread, 2);
    s.apply(Action::SetRead { id: "a".into(), read: true }).unwrap();
    assert_eq!(s.folders().unwrap()[0].unread, 1);
    // Sync still says unread: the local change stands while it's queued.
    s.upsert_messages("in", &[msg("a", "One", "Ada", "2026-10-22T09:00:00Z", false)]).unwrap();
    let a = s.message("a").unwrap().unwrap();
    assert!(a.is_read && a.pending);

    s.apply(Action::Move { id: "b".into(), dest: "del".into() }).unwrap();
    assert_eq!(s.message("b").unwrap().unwrap().folder_id, "del");
    s.apply(Action::SetFlag { id: "b".into(), status: "flagged".into() }).unwrap();
    let q = s.queued().unwrap();
    assert_eq!(q.len(), 3);
    s.done(&q[0]).unwrap();
    assert!(!s.message("a").unwrap().unwrap().pending);
    // The move gave b a new id: the queued flag follows it.
    s.done(&q[1]).unwrap();
    s.rename_message("b", "b2").unwrap();
    assert!(s.message("b").unwrap().is_none());
    assert_eq!(s.queued().unwrap()[0].action, Action::SetFlag { id: "b2".into(), status: "flagged".into() });
    s.remove_messages("in", &["b".into()]).unwrap();
    assert!(s.message("b2").unwrap().is_some(), "a removal from the old folder doesn't touch the moved message");

    s.apply(Action::Delete { id: "a".into() }).unwrap();
    assert!(s.message("a").unwrap().is_none());
}

#[test]
fn sending_waits_in_the_outbox() {
    let mut s = Store::open_in_memory().unwrap();
    s.save_folders(&[folder("out", "Outbox", None)], &[("outbox".into(), "out".into())]).unwrap();
    let compose = Compose { local_id: "local:1".into(), subject: "Hi".into(), to: vec![Addr::new("", "a@x.example")], html: "<p>Hello <b>there</b></p>".into(), ..Default::default() };
    s.apply(Action::Send { compose }).unwrap();
    let m = s.messages("out", ListFilter::All, SortBy::Date, false, "").unwrap();
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].preview, "Hello there");
    let q = s.queued().unwrap();
    s.done(&q[0]).unwrap();
    assert!(s.messages("out", ListFilter::All, SortBy::Date, false, "").unwrap().is_empty());
}

#[test]
fn calendar_windows_replace_what_they_cover() {
    let mut s = Store::open_in_memory().unwrap();
    s.save_calendars(&[Calendar { id: "cal".into(), name: "Calendar".into(), is_default_calendar: Some(true), ..Default::default() }]).unwrap();
    let ev = |id: &str, start: &str, end: &str| Event {
        id: id.into(),
        subject: Some(id.into()),
        start: Some(mailcraft_graph::types::DateTimeTimeZone { date_time: start.into(), time_zone: "UTC".into() }),
        end: Some(mailcraft_graph::types::DateTimeTimeZone { date_time: end.into(), time_zone: "UTC".into() }),
        ..Default::default()
    };
    let t = |s: &str| parse_time(s).unwrap();
    s.save_calendar_window("cal", t("2026-10-19T00:00:00Z"), t("2026-10-26T00:00:00Z"), &[ev("x", "2026-10-20T09:00:00.0000000", "2026-10-20T10:00:00.0000000"), ev("y", "2026-10-22T09:00:00.0000000", "2026-10-22T10:00:00.0000000")]).unwrap();
    assert_eq!(s.events(t("2026-10-19T00:00:00Z"), t("2026-10-26T00:00:00Z")).unwrap().len(), 2);
    // y was deleted on the server.
    s.save_calendar_window("cal", t("2026-10-19T00:00:00Z"), t("2026-10-26T00:00:00Z"), &[ev("x", "2026-10-20T09:00:00.0000000", "2026-10-20T10:00:00.0000000")]).unwrap();
    let left = s.events(t("2026-10-19T00:00:00Z"), t("2026-10-26T00:00:00Z")).unwrap();
    assert_eq!(left.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["x"]);
    assert_eq!(left[0].start, t("2026-10-20T09:00:00Z"));
    s.apply(Action::RespondEvent { id: "x".into(), response: "decline".into(), comment: String::new(), send: true }).unwrap();
    assert!(s.events(t("2026-10-19T00:00:00Z"), t("2026-10-26T00:00:00Z")).unwrap().is_empty());
}

#[test]
fn a_new_schema_rebuilds_the_cache_but_keeps_queued_changes() {
    let dir = std::env::temp_dir().join(format!("mailcraft-store-test-{}", std::process::id()));
    let path = dir.join("cache.sqlite");
    let _ = std::fs::remove_file(&path);
    {
        let mut s = Store::open(&path).unwrap();
        s.apply(Action::SetRead { id: "zz".into(), read: true }).unwrap();
        s.db.pragma_update(None, "user_version", 1).unwrap();
    }
    let s = Store::open(&path).unwrap();
    assert_eq!(s.queued_count().unwrap(), 1);
    let _ = std::fs::remove_dir_all(dir);
}
