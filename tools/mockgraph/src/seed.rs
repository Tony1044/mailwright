//! The sample mailbox: Sam Carter at Harbourview, a fictional firm. Dates are relative to "now" so
//! the mailbox always looks lived-in: mail from the last few weeks, a calendar around this week.

use chrono::{DateTime, Datelike, Duration, NaiveTime, Utc};
use serde_json::{Map, Value, json};

use crate::{Att, Ev, Folder, ME_ADDRESS, ME_NAME, Msg, State, png};

pub fn well_known_folders(st: &mut State) {
    let list: [(&str, &str, &'static str); 8] = [
        ("AAMkInbox", "Inbox", "inbox"),
        ("AAMkDrafts", "Drafts", "drafts"),
        ("AAMkSent", "Sent Items", "sentitems"),
        ("AAMkDeleted", "Deleted Items", "deleteditems"),
        ("AAMkJunk", "Junk Email", "junkemail"),
        ("AAMkArchive", "Archive", "archive"),
        ("AAMkOutbox", "Outbox", "outbox"),
        ("AAMkConvHist", "Conversation History", "conversationhistory"),
    ];
    for (id, name, wk) in list {
        st.folders.push(Folder { id: id.into(), name: name.into(), parent: None, well_known: Some(wk), hidden: false });
    }
    st.calendars.push(json!({"id": "AAMkCalendar", "name": "Calendar", "hexColor": "#0f6cbd", "canEdit": true, "isDefaultCalendar": true}));
}

fn person(name: &str, address: &str) -> Value {
    json!({"emailAddress": {"name": name, "address": address}})
}

fn me() -> Value {
    person(ME_NAME, ME_ADDRESS)
}

struct P(&'static str, &'static str);

const ADA: P = P("Ada Lindqvist", "ada.lindqvist@harbourview.example");
const RAJ: P = P("Raj Patel", "raj.patel@harbourview.example");
const MEI: P = P("Mei Tanaka", "mei.tanaka@harbourview.example");
const OWEN: P = P("Owen Hughes", "owen.hughes@harbourview.example");
const PRIYA: P = P("Priya Nair", "priya.nair@harbourview.example");
const LUCA: P = P("Luca Romano", "luca.romano@northgate-partners.example");
const FIN: P = P("Harbourview Finance", "finance@harbourview.example");
const IT: P = P("IT Service Desk", "servicedesk@harbourview.example");
const NEWS: P = P("The Ledger Weekly", "newsletter@ledgerweekly.example");
const TRAVEL: P = P("Skyline Travel", "bookings@skylinetravel.example");
const HANNAH: P = P("Hannah Brooks", "hannah.brooks@harbourview.example");

fn pv(p: &P) -> Value {
    person(p.0, p.1)
}

#[derive(Default)]
struct Opts {
    read: bool,
    flag: Option<&'static str>,
    high: bool,
    other: bool,
    cc: Vec<Value>,
    to: Vec<Value>,
    atts: Vec<Att>,
    categories: Vec<&'static str>,
}

fn iso(t: DateTime<Utc>) -> String {
    t.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn add(st: &mut State, folder: &str, at: DateTime<Utc>, from: Value, subject: &str, html: &str, o: Opts) -> String {
    let id = st.new_id("AAMkMsg");
    let seq = st.bump();
    let mut props = Map::new();
    props.insert("subject".into(), json!(subject));
    props.insert("from".into(), from.clone());
    props.insert("sender".into(), from);
    props.insert("toRecipients".into(), json!(if o.to.is_empty() { vec![me()] } else { o.to }));
    props.insert("ccRecipients".into(), json!(o.cc));
    props.insert("bccRecipients".into(), json!([]));
    props.insert("replyTo".into(), json!([]));
    props.insert("receivedDateTime".into(), json!(iso(at)));
    props.insert("sentDateTime".into(), json!(iso(at - Duration::seconds(20))));
    props.insert("isRead".into(), json!(o.read));
    props.insert("isDraft".into(), json!(false));
    props.insert("importance".into(), json!(if o.high { "high" } else { "normal" }));
    props.insert("flag".into(), json!({"flagStatus": o.flag.unwrap_or("notFlagged")}));
    props.insert("conversationId".into(), json!(format!("conv-{}", subject.trim_start_matches("RE: ").trim_start_matches("FW: ").len())));
    props.insert("inferenceClassification".into(), json!(if o.other { "other" } else { "focused" }));
    props.insert("categories".into(), json!(o.categories));
    props.insert("webLink".into(), json!(format!("https://outlook.office365.com/owa/?ItemID={id}")));
    let preview: String = crate::strip_tags(html).chars().take(255).collect();
    props.insert("bodyPreview".into(), json!(preview));
    let body = format!("<html><head><meta charset=\"utf-8\"></head><body>{html}</body></html>");
    st.messages.push(Msg { id: id.clone(), folder: folder.into(), seq, props, body, attachments: o.atts, meeting_type: None, event_id: None });
    id
}

fn att(st: &mut State, name: &str, ct: &str, bytes: Vec<u8>) -> Att {
    Att { id: st.new_id("AAMkAtt"), name: name.into(), content_type: ct.into(), bytes, inline: false, content_id: None }
}

fn csv_report() -> Vec<u8> {
    let mut s = String::from("Region,Q1,Q2,Q3\n");
    for (r, a, b, c) in [("North", 412, 455, 498), ("South", 380, 371, 402), ("East", 290, 344, 361), ("West", 505, 512, 547)] {
        s.push_str(&format!("{r},{a},{b},{c}\n"));
    }
    s.into_bytes()
}

fn signature(name: &str, role: &str) -> String {
    format!("<p style=\"margin-top:18px;color:#555\">{name}<br><span style=\"font-size:9pt\">{role} | Harbourview</span></p>")
}

#[allow(clippy::too_many_arguments)]
fn ev(st: &mut State, start: String, end: String, subject: &str, location: &str, attendees: Vec<(&P, &str)>, extra: Value, body: &str) -> String {
        let id = st.new_id("AAMkEv");
        let mut props = Map::new();
        props.insert("subject".into(), json!(subject));
        props.insert("start".into(), json!({"dateTime": start, "timeZone": "UTC"}));
        props.insert("end".into(), json!({"dateTime": end, "timeZone": "UTC"}));
        props.insert("isAllDay".into(), json!(false));
        props.insert("location".into(), json!({"displayName": location}));
        props.insert("organizer".into(), me());
        props.insert("isOrganizer".into(), json!(true));
        props.insert("responseStatus".into(), json!({"response": "organizer"}));
        props.insert("showAs".into(), json!("busy"));
        props.insert("isCancelled".into(), json!(false));
        props.insert("isOnlineMeeting".into(), json!(false));
        props.insert("type".into(), json!("singleInstance"));
        props.insert("categories".into(), json!([]));
        props.insert("reminderMinutesBeforeStart".into(), json!(15));
        props.insert("isReminderOn".into(), json!(true));
        props.insert(
            "attendees".into(),
            json!(attendees.iter().map(|(p, r)| json!({"emailAddress": {"name": p.0, "address": p.1}, "type": "required", "status": {"response": r}})).collect::<Vec<_>>()),
        );
        props.insert("bodyPreview".into(), json!(crate::strip_tags(body)));
        if let Some(o) = extra.as_object() {
            for (k, v) in o {
                props.insert(k.clone(), v.clone());
            }
        }
        st.events.push(Ev { id: id.clone(), calendar: "AAMkCalendar".into(), props, body: body.to_string() });
        id
    }

pub fn sample_mailbox(st: &mut State, now: DateTime<Utc>) {
    // Sub-folders.
    for (id, name, parent) in [("AAMkProjects", "Projects", "AAMkInbox"), ("AAMkReceipts", "Receipts", "AAMkInbox"), ("AAMkHarbour", "Harbour Street fit-out", "AAMkProjects")] {
        st.folders.push(Folder { id: id.into(), name: name.into(), parent: Some(parent.into()), well_known: None, hidden: false });
    }

    let ago = |mins: i64| now - Duration::minutes(mins);
    let day = |days: i64, h: u32, m: u32| {
        let d = (now - Duration::days(days)).date_naive();
        DateTime::<Utc>::from_naive_utc_and_offset(d.and_time(NaiveTime::from_hms_opt(h, m, 0).unwrap_or_default()), Utc)
    };

    // ---- Inbox: today
    let logo = png::banner_png(560, 90, [15, 108, 189], [0, 183, 195]);
    let logo_att = Att { id: st.new_id("AAMkAtt"), name: "banner.png".into(), content_type: "image/png".into(), bytes: logo, inline: true, content_id: Some("banner001".into()) };
    let programme = att(st, "Harbour Street programme.csv", "text/csv", csv_report());
    let terms = att(st, "Heads of terms (draft).txt", "text/plain", b"HEADS OF TERMS (DRAFT)\n\nTerm: 5 years\nBreak: year 3\nRent review: upwards only\n".to_vec());
    add(
        st,
        "AAMkInbox",
        ago(12),
        pv(&ADA),
        "Harbour Street fit-out – revised schedule",
        &format!(
            "<p>Hi Sam,</p><p>The contractor has come back with a revised schedule for the Harbour Street fit-out. The headline is that <b>first fix electrics move back a week</b>, but they think they can recover most of it in decoration.</p>\
             <table border=\"1\" cellpadding=\"4\" style=\"border-collapse:collapse\"><tr style=\"background:#f3f2f1\"><th>Stage</th><th>Was</th><th>Now</th></tr>\
             <tr><td>Strip-out</td><td>w/c 2 Nov</td><td>w/c 2 Nov</td></tr><tr><td>First fix</td><td>w/c 9 Nov</td><td><span style=\"color:#c50f1f\">w/c 16 Nov</span></td></tr>\
             <tr><td>Decoration</td><td>w/c 30 Nov</td><td>w/c 30 Nov</td></tr><tr><td>Handover</td><td>11 Dec</td><td>14 Dec</td></tr></table>\
             <p>Could you check it against the lease dates before I confirm with them on Friday? The full programme is attached.</p>{}",
            signature("Ada", "Projects Lead")
        ),
        Opts { flag: Some("flagged"), cc: vec![pv(&OWEN)], atts: vec![programme], ..Default::default() },
    );
    add(
        st,
        "AAMkInbox",
        ago(47),
        pv(&RAJ),
        "Quick question on the Q3 numbers",
        "<p>Sam – are the Q3 numbers in the board pack the final ones, or is the West region still being reconciled? Happy to wait if so.</p><p>Thanks,<br>Raj</p>",
        Opts::default(),
    );
    add(
        st,
        "AAMkInbox",
        ago(95),
        pv(&IT),
        "Planned maintenance: VPN, Saturday 06:00–08:00",
        "<p>Hello,</p><p>The remote access VPN will be unavailable on <b>Saturday between 06:00 and 08:00</b> while we apply security updates. Webmail and Teams aren't affected.</p><ul><li>Save your work before 06:00</li><li>Reconnect after 08:00; you may need to approve a new sign-in</li></ul><p>IT Service Desk</p>",
        Opts { read: true, ..Default::default() },
    );
    add(
        st,
        "AAMkInbox",
        ago(160),
        pv(&NEWS),
        "This week: rates, rents and the return of the office",
        "<div style=\"font-family:Georgia,serif\"><img src=\"cid:banner001\" alt=\"The Ledger Weekly\" width=\"560\" height=\"90\"><h2>Rates, rents and the return of the office</h2>\
             <p>Commercial rents rose for the third quarter running, led by smaller regional cities. <a href=\"https://ledgerweekly.example/rents\">Read the analysis</a>.</p>\
             <h3>Also this week</h3><ol><li>Why fit-out costs are finally levelling off</li><li>Five questions to ask before you sign a break clause</li><li>Hybrid working: what the data says now</li></ol>\
             <p style=\"font-size:8pt;color:#777\">You're receiving this because you subscribed. <a href=\"https://ledgerweekly.example/unsub\">Unsubscribe</a>. Remote images: <img src=\"https://tracker.ledgerweekly.example/p.gif\" width=\"1\" height=\"1\"></p></div>",
        Opts { other: true, atts: vec![logo_att], ..Default::default() },
    );
    add(
        st,
        "AAMkInbox",
        ago(210),
        pv(&MEI),
        "RE: Supplier onboarding checklist",
        &format!(
            "<p>Thanks Sam, that's really helpful. I've added the two items you suggested:</p><ul><li>Proof of professional indemnity cover</li><li>A named contact for data protection queries</li></ul><p>I'll circulate the final version tomorrow.</p>{}\
             <div style=\"border-left:2px solid #ccc;padding-left:8px;color:#555\"><p><b>From:</b> Sam Carter<br><b>Sent:</b> yesterday<br><b>Subject:</b> Supplier onboarding checklist</p><p>Hi Mei, two small additions from me…</p></div>",
            signature("Mei", "Procurement")
        ),
        Opts { read: true, ..Default::default() },
    );

    // ---- Inbox: yesterday and this week
    add(
        st,
        "AAMkInbox",
        day(1, 16, 40),
        pv(&FIN),
        "Expense claim approved – October",
        "<p>Your expense claim <b>EXP-20417</b> for £186.40 has been approved and will be paid on the next payroll run.</p>",
        Opts { read: true, other: true, ..Default::default() },
    );
    add(
        st,
        "AAMkInbox",
        day(1, 11, 5),
        pv(&OWEN),
        "Lease renewal – heads of terms attached",
        &format!(
            "<p>Sam,</p><p>Attached are the draft heads of terms from the landlord's agent. Three things stand out:</p><ol><li>They want a five-year term with a break at year three (we asked for two).</li><li>The rent review is upwards-only.</li><li>Service charge cap has gone.</li></ol><p>Can we talk it through before the team meeting?</p>{}",
            signature("Owen", "Legal Counsel")
        ),
        Opts { high: true, flag: Some("flagged"), atts: vec![terms], ..Default::default() },
    );
    add(st, "AAMkInbox", day(2, 9, 12), pv(&PRIYA), "Lunch on Thursday?", "<p>Are you around for lunch on Thursday? The new place on Quay Street does a decent set menu. 12:30?</p><p>P</p>", Opts { read: true, ..Default::default() });
    add(
        st,
        "AAMkInbox",
        day(2, 8, 2),
        pv(&TRAVEL),
        "Your booking is confirmed: Manchester, 12 November",
        "<table width=\"100%\" style=\"background:#f6f8fa\"><tr><td style=\"padding:12px\"><h3 style=\"margin:0\">Booking reference SKY-77Q2K</h3><p>Return rail, London Euston → Manchester Piccadilly<br>Outbound 07:00, return 18:35</p></td></tr></table><p>Have a good trip.</p>",
        Opts { read: true, other: true, categories: vec!["Travel"], ..Default::default() },
    );
    add(
        st,
        "AAMkInbox",
        day(3, 15, 30),
        pv(&HANNAH),
        "Welcome pack for new starters",
        "<p>Hi all,</p><p>I've refreshed the welcome pack for new starters. Main changes: the IT set-up steps are now a checklist, and there's a page on who to ask for what. Comments welcome by Friday.</p><p>Hannah</p>",
        Opts { read: true, to: vec![pv(&ADA), pv(&RAJ), me()], ..Default::default() },
    );
    add(st, "AAMkInbox", day(4, 10, 0), pv(&LUCA), "Introductions – Northgate Partners", "<p>Dear Sam,</p><p>Ada suggested I get in touch. We advise on workplace strategy for growing firms, and I'd welcome twenty minutes to hear about your plans for Harbour Street.</p><p>Kind regards,<br>Luca Romano<br>Northgate Partners</p>", Opts { read: true, flag: Some("complete"), ..Default::default() });

    // ---- Inbox: earlier
    let older = [
        (6, &RAJ, "Board pack – first draft", "<p>First draft of the board pack is on the shared drive. Sections 3 and 4 still need numbers.</p>"),
        (8, &ADA, "Site visit photos", "<p>Photos from this morning's site visit are in the project folder. The north stairwell is in better shape than we thought.</p>"),
        (9, &FIN, "Purchase order PO-5521 raised", "<p>Purchase order PO-5521 has been raised for Brightline Electrical, value £24,800.00.</p>"),
        (11, &MEI, "Supplier onboarding checklist", "<p>Hi Sam, here's the draft supplier onboarding checklist. Anything missing?</p>"),
        (13, &IT, "Your password expires in 7 days", "<p>Your Harbourview password expires in 7 days. Change it at any time from your account page.</p>"),
        (15, &OWEN, "Contract template updated", "<p>The standard services contract template now includes the new liability clause. Please use version 4.2 from today.</p>"),
        (18, &PRIYA, "Team social – save the date", "<p>Save the date: team social on the last Friday of the month. Details to follow!</p>"),
        (21, &HANNAH, "Annual leave reminder", "<p>A reminder that leave requests for December should be in by the end of the month.</p>"),
    ];
    for (d, p, subject, html) in older {
        add(st, "AAMkInbox", day(d, 10 + (d % 6) as u32, (d * 7 % 60) as u32), pv(p), subject, html, Opts { read: true, ..Default::default() });
    }
    // ---- Sub-folders
    add(st, "AAMkProjects", day(5, 14, 20), pv(&ADA), "Harbour Street – contractor shortlist", "<p>Shortlist is down to three. Scores attached in the tracker.</p>", Opts { read: true, ..Default::default() });
    add(st, "AAMkHarbour", day(7, 9, 45), pv(&ADA), "Floor plan v3", "<p>Floor plan v3 incorporates the extra meeting room on level 2.</p>", Opts { read: false, ..Default::default() });
    add(st, "AAMkReceipts", day(10, 19, 2), pv(&TRAVEL), "Receipt for SKY-61B0P", "<p>Thank you for your payment of £84.20.</p>", Opts { read: true, ..Default::default() });

    // ---- Sent, Drafts, Junk, Deleted
    add(st, "AAMkSent", day(1, 12, 15), me(), "RE: Lease renewal – heads of terms attached", "<p>Owen – yes, let's talk at 2. I agree the break clause is the big one.</p><p>Sam</p>", Opts { read: true, to: vec![pv(&OWEN)], ..Default::default() });
    add(st, "AAMkSent", day(2, 9, 30), me(), "RE: Lunch on Thursday?", "<p>12:30 works. See you there.</p>", Opts { read: true, to: vec![pv(&PRIYA)], ..Default::default() });
    let draft = add(st, "AAMkDrafts", day(1, 17, 50), me(), "Notes for the board", "<p>Points to cover:</p><ul><li>Harbour Street timing</li><li>Lease renewal position</li></ul>", Opts { read: true, to: vec![pv(&RAJ)], ..Default::default() });
    if let Some(m) = st.messages.iter_mut().find(|m| m.id == draft) {
        m.props.insert("isDraft".into(), json!(true));
    }
    add(st, "AAMkJunk", day(2, 3, 14), person("Prize Desk", "winner@prizes.example"), "You have been selected!!!", "<p>Claim your reward now.</p>", Opts { other: true, ..Default::default() });
    add(st, "AAMkDeleted", day(6, 8, 0), pv(&NEWS), "Last week: the big rate debate", "<p>Last week's issue.</p>", Opts { read: true, other: true, ..Default::default() });

    // ---- Calendar
    let monday = now.date_naive() - Duration::days(now.weekday().num_days_from_monday() as i64);
    let at = |date: chrono::NaiveDate, h: u32, m: u32| date.and_time(NaiveTime::from_hms_opt(h, m, 0).unwrap_or_default()).format("%Y-%m-%dT%H:%M:%S.0000000").to_string();
    // Daily stand-up, a recurring series (occurrences over six weeks).
    for w in -2i64..4 {
        for d in 0..5 {
            let date = monday + Duration::days(w * 7 + d);
            ev(
                st,
                at(date, 9, 30),
                at(date, 9, 45),
                "Projects stand-up",
                "Teams",
                vec![(&ADA, "accepted"), (&RAJ, "accepted"), (&MEI, "tentativelyAccepted")],
                json!({"type": "occurrence", "seriesMasterId": "AAMkSeriesStandup", "isOnlineMeeting": true, "onlineMeeting": {"joinUrl": "https://meet.harbourview.example/j/standup"}, "categories": ["Projects"], "showAs": "busy"}),
                "<p>Fifteen minutes: yesterday, today, blockers.</p>",
            );
        }
    }
    let wk = |d: i64| monday + Duration::days(d);
    ev(st, at(wk(0), 11, 0), at(wk(0), 12, 0), "Lease renewal – prep", "Room 2.04", vec![(&OWEN, "accepted")], json!({}), "<p>Go through the heads of terms before we reply.</p>");
    ev(st, at(wk(1), 14, 0), at(wk(1), 14, 30), "1:1 Ada / Sam", "Ada's office", vec![(&ADA, "accepted")], json!({"categories": ["Projects"]}), "<p>Regular catch-up.</p>");
    ev(
        st,
        at(wk(2), 10, 0),
        at(wk(2), 11, 30),
        "Harbour Street – programme review",
        "Board room / Teams",
        vec![(&ADA, "accepted"), (&OWEN, "accepted"), (&LUCA, "none")],
        json!({"isOnlineMeeting": true, "onlineMeeting": {"joinUrl": "https://meet.harbourview.example/j/hs-review"}, "categories": ["Projects"]}),
        "<p>Agenda</p><ol><li>Revised schedule</li><li>Lease dates</li><li>Risks</li></ol>",
    );
    ev(st, at(wk(3), 12, 30), at(wk(3), 13, 30), "Lunch with Priya", "Quay Street", vec![], json!({"showAs": "oof", "reminderMinutesBeforeStart": 30}), "");
    ev(st, at(wk(3), 15, 0), at(wk(3), 16, 0), "Focus time", "", vec![], json!({"showAs": "workingElsewhere"}), "");
    ev(st, at(wk(4), 16, 0), at(wk(4), 17, 0), "Board pack sign-off", "Room 3.01", vec![(&RAJ, "accepted"), (&HANNAH, "tentativelyAccepted")], json!({"categories": ["Finance"]}), "");
    ev(
        st,
        at(wk(7), 0, 0),
        at(wk(8), 0, 0),
        "Office closed – electrical testing",
        "Harbourview HQ",
        vec![],
        json!({"isAllDay": true, "showAs": "free", "isOrganizer": false, "organizer": person("Facilities", "facilities@harbourview.example"), "responseStatus": {"response": "accepted"}}),
        "<p>The building is closed for electrical testing.</p>",
    );
    ev(st, at(wk(9), 7, 0), at(wk(9), 18, 35), "Manchester – client visit", "Manchester", vec![], json!({"showAs": "oof", "categories": ["Travel"]}), "<p>Booking ref SKY-77Q2K</p>");
    ev(st, at(wk(-3), 10, 0), at(wk(-3), 11, 0), "Quarterly planning", "Board room", vec![(&ADA, "accepted"), (&RAJ, "accepted")], json!({}), "");

    // A meeting request waiting for an answer, with its message in the Inbox.
    let budget = ev(
        st,
        at(wk(4), 14, 0),
        at(wk(4), 15, 0),
        "Q4 budget review",
        "Finance meeting room",
        vec![(&RAJ, "organizer")],
        json!({"isOrganizer": false, "organizer": person(RAJ.0, RAJ.1), "responseStatus": {"response": "notResponded"}, "showAs": "tentative", "categories": ["Finance"]}),
        "<p>Let's review the Q4 budget before it goes to the board.</p>",
    );
    let req = add(
        st,
        "AAMkInbox",
        ago(30),
        pv(&RAJ),
        "Q4 budget review",
        "<p>Let's review the Q4 budget before it goes to the board.</p>",
        Opts { ..Default::default() },
    );
    if let Some(m) = st.messages.iter_mut().find(|m| m.id == req) {
        m.meeting_type = Some("meetingRequest".into());
        m.event_id = Some(budget);
    }

    // ---- People
    for p in [&ADA, &RAJ, &MEI, &OWEN, &PRIYA, &LUCA, &HANNAH, &FIN, &IT] {
        st.people.push(json!({"displayName": p.0, "scoredEmailAddresses": [{"address": p.1, "relevanceScore": 8.0}]}));
    }
}
