//! Building an outgoing message (for SMTP) and reading a fetched one (for IMAP/POP3), mapping to
//! and from the shared [`mailwright_graph::types::Message`] model.

use base64::Engine as _;
use mailwright_graph::types::{EmailAddress, ItemBody, Message, Recipient};
use mailwright_store::Addr;

/// One attachment to send.
pub struct OutAttach<'a> {
    pub name: &'a str,
    pub content_type: &'a str,
    pub bytes: &'a [u8],
    /// Set for an inline image referenced by `cid:`.
    pub content_id: Option<&'a str>,
}

/// A message to render as RFC 822 for SMTP.
pub struct OutMail<'a> {
    pub from: &'a Addr,
    pub to: &'a [Addr],
    pub cc: &'a [Addr],
    pub subject: &'a str,
    pub html: &'a str,
    pub text: &'a str,
    pub attachments: &'a [OutAttach<'a>],
    pub message_id: &'a str,
    pub in_reply_to: Option<&'a str>,
    /// `low`, `normal` or `high`.
    pub importance: &'a str,
    pub request_read_receipt: bool,
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Fold base64 into 76-character lines, as MIME wants.
fn b64_wrapped(bytes: &[u8]) -> String {
    let s = b64(bytes);
    let mut out = String::with_capacity(s.len() + s.len() / 76 + 2);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && i % 76 == 0 {
            out.push_str("\r\n");
        }
        out.push(c);
    }
    out
}

/// A header value, RFC 2047-encoded when it isn't plain ASCII.
fn encode_word(s: &str) -> String {
    if s.is_ascii() {
        s.to_string()
    } else {
        format!("=?UTF-8?B?{}?=", b64(s.as_bytes()))
    }
}

/// `Display Name <addr>` for a header.
fn format_addr(a: &Addr) -> String {
    if a.name.trim().is_empty() {
        a.address.clone()
    } else {
        format!("{} <{}>", encode_word(&a.name), a.address)
    }
}

fn addr_list(addrs: &[Addr]) -> String {
    addrs.iter().map(format_addr).collect::<Vec<_>>().join(", ")
}

fn boundary(tag: &str) -> String {
    let n = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
    format!("=_mailwright_{tag}_{n:x}")
}

/// Render the message as RFC 822 bytes (CRLF line endings), ready for SMTP DATA.
pub fn build(mail: &OutMail) -> Vec<u8> {
    let mut h = String::new();
    h.push_str(&format!("From: {}\r\n", format_addr(mail.from)));
    if !mail.to.is_empty() {
        h.push_str(&format!("To: {}\r\n", addr_list(mail.to)));
    }
    if !mail.cc.is_empty() {
        h.push_str(&format!("Cc: {}\r\n", addr_list(mail.cc)));
    }
    h.push_str(&format!("Subject: {}\r\n", encode_word(mail.subject)));
    h.push_str(&format!("Date: {}\r\n", chrono::Local::now().to_rfc2822()));
    h.push_str(&format!("Message-ID: <{}>\r\n", mail.message_id));
    if let Some(irt) = mail.in_reply_to {
        h.push_str(&format!("In-Reply-To: <{irt}>\r\nReferences: <{irt}>\r\n"));
    }
    match mail.importance {
        "high" => h.push_str("Importance: high\r\nX-Priority: 1\r\n"),
        "low" => h.push_str("Importance: low\r\nX-Priority: 5\r\n"),
        _ => {}
    }
    if mail.request_read_receipt && !mail.from.address.is_empty() {
        h.push_str(&format!("Disposition-Notification-To: {}\r\n", mail.from.address));
    }
    h.push_str("MIME-Version: 1.0\r\n");

    // The body: text + html as alternatives, inline images as related, files as mixed.
    let (inline, files): (Vec<&OutAttach>, Vec<&OutAttach>) = mail.attachments.iter().partition(|a| a.content_id.is_some());

    let mut alt = String::new();
    let alt_b = boundary("alt");
    alt.push_str(&format!("Content-Type: multipart/alternative; boundary=\"{alt_b}\"\r\n\r\n"));
    for (ctype, body) in [("text/plain", mail.text), ("text/html", mail.html)] {
        alt.push_str(&format!("--{alt_b}\r\n"));
        alt.push_str(&format!("Content-Type: {ctype}; charset=UTF-8\r\n"));
        alt.push_str("Content-Transfer-Encoding: base64\r\n\r\n");
        alt.push_str(&b64_wrapped(body.as_bytes()));
        alt.push_str("\r\n");
    }
    alt.push_str(&format!("--{alt_b}--\r\n"));

    // Wrap the alternative in multipart/related if there are inline images.
    let body_section = if inline.is_empty() {
        alt
    } else {
        let rel_b = boundary("rel");
        let mut rel = format!("Content-Type: multipart/related; boundary=\"{rel_b}\"\r\n\r\n--{rel_b}\r\n{alt}");
        for a in &inline {
            rel.push_str(&format!("--{rel_b}\r\n"));
            rel.push_str(&part_headers(a, true));
            rel.push_str(&b64_wrapped(a.bytes));
            rel.push_str("\r\n");
        }
        rel.push_str(&format!("--{rel_b}--\r\n"));
        rel
    };

    let mut out = Vec::new();
    if files.is_empty() {
        // The headers belong to the single body section.
        out.extend_from_slice(h.as_bytes());
        out.extend_from_slice(body_section.as_bytes());
    } else {
        let mix_b = boundary("mix");
        h.push_str(&format!("Content-Type: multipart/mixed; boundary=\"{mix_b}\"\r\n\r\n"));
        out.extend_from_slice(h.as_bytes());
        out.extend_from_slice(format!("--{mix_b}\r\n").as_bytes());
        out.extend_from_slice(body_section.as_bytes());
        for a in &files {
            out.extend_from_slice(format!("--{mix_b}\r\n").as_bytes());
            out.extend_from_slice(part_headers(a, false).as_bytes());
            out.extend_from_slice(b64_wrapped(a.bytes).as_bytes());
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(format!("--{mix_b}--\r\n").as_bytes());
    }
    out
}

fn part_headers(a: &OutAttach, inline: bool) -> String {
    let mut s = format!("Content-Type: {}; name=\"{}\"\r\n", a.content_type, encode_word(a.name));
    s.push_str("Content-Transfer-Encoding: base64\r\n");
    let disp = if inline { "inline" } else { "attachment" };
    s.push_str(&format!("Content-Disposition: {disp}; filename=\"{}\"\r\n", encode_word(a.name)));
    if let Some(cid) = a.content_id {
        s.push_str(&format!("Content-ID: <{}>\r\n", cid.trim_matches(['<', '>'])));
    }
    s.push_str("\r\n");
    s
}

/// An attachment read out of a fetched message.
pub struct ParsedAttachment {
    pub name: String,
    pub content_type: String,
    pub is_inline: bool,
    pub content_id: Option<String>,
    pub bytes: Vec<u8>,
}

/// What a fetched message parsed into: the model message (with body) and its attachments.
pub struct Parsed {
    pub message: Message,
    pub attachments: Vec<ParsedAttachment>,
}

fn to_recipients(list: Option<&mail_parser::Address>) -> Vec<Recipient> {
    let Some(list) = list else { return Vec::new() };
    list.iter()
        .filter_map(|a| {
            a.address.as_deref().map(|email| Recipient {
                email_address: EmailAddress { name: a.name.as_deref().map(|n| n.to_string()), address: Some(email.to_string()) },
            })
        })
        .collect()
}

fn first_recipient(list: Option<&mail_parser::Address>) -> Option<Recipient> {
    to_recipients(list).into_iter().next()
}

/// Parse RFC 822 bytes into the shared model. `id` and `folder` are what the store keys on.
pub fn parse(raw: &[u8], id: &str, folder: &str, is_read: bool, flagged: bool) -> Parsed {
    use mail_parser::MimeHeaders;
    let parser = mail_parser::MessageParser::default();
    let Some(msg) = parser.parse(raw) else {
        // Unreadable: a stub so the row at least exists.
        let mut m = Message { id: id.to_string(), parent_folder_id: Some(folder.to_string()), ..Default::default() };
        m.subject = Some("(unreadable message)".into());
        m.is_read = Some(is_read);
        return Parsed { message: m, attachments: Vec::new() };
    };

    let html = msg.body_html(0).map(|c| c.into_owned());
    let text = msg.body_text(0).map(|c| c.into_owned());
    let body = match (&html, &text) {
        (Some(h), _) => Some(ItemBody { content_type: "html".into(), content: h.clone() }),
        (None, Some(t)) => Some(ItemBody { content_type: "text".into(), content: t.clone() }),
        _ => None,
    };
    let preview: String = text
        .as_deref()
        .map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "))
        .or_else(|| html.as_deref().map(strip_tags))
        .unwrap_or_default()
        .chars()
        .take(255)
        .collect();

    let received = msg.date().map(|d| d.to_rfc3339());

    let mut attachments = Vec::new();
    for part in msg.attachments() {
        let bytes = part.contents().to_vec();
        let ctype = part
            .content_type()
            .map(|c| match c.subtype() {
                Some(st) => format!("{}/{}", c.ctype(), st),
                None => c.ctype().to_string(),
            })
            .unwrap_or_else(|| "application/octet-stream".into());
        attachments.push(ParsedAttachment {
            name: part.attachment_name().unwrap_or("attachment").to_string(),
            content_type: ctype,
            is_inline: part.content_id().is_some(),
            content_id: part.content_id().map(|c| c.to_string()),
            bytes,
        });
    }

    let flag = flagged.then(|| mailwright_graph::types::FollowupFlag { flag_status: "flagged".into() });
    let message = Message {
        id: id.to_string(),
        subject: msg.subject().map(|s| s.to_string()).or(Some(String::new())),
        body_preview: Some(preview),
        body,
        from: first_recipient(msg.from()),
        sender: first_recipient(msg.from()),
        to_recipients: to_recipients(msg.to()),
        cc_recipients: to_recipients(msg.cc()),
        received_date_time: received.clone(),
        sent_date_time: received,
        is_read: Some(is_read),
        is_draft: Some(false),
        has_attachments: Some(!attachments.is_empty()),
        importance: Some("normal".into()),
        flag,
        parent_folder_id: Some(folder.to_string()),
        inference_classification: Some("focused".into()),
        ..Default::default()
    };
    Parsed { message, attachments }
}

/// A readable plain-text version of HTML, for the text/plain alternative of an outgoing message.
pub fn html_to_text(html: &str) -> String {
    let with_breaks = html
        .replace("</p>", "

")
        .replace("<br>", "
")
        .replace("<br/>", "
")
        .replace("<br />", "
")
        .replace("</div>", "
")
        .replace("</li>", "
");
    let mut out = String::new();
    let mut in_tag = false;
    for c in with_breaks.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    // Collapse runs of blank lines.
    let mut text = String::new();
    for line in out.lines() {
        text.push_str(line.trim_end());
        text.push('\n');
    }
    while text.contains("


") {
        text = text.replace("


", "

");
    }
    text.trim().to_string()
}

/// A rough plain-text rendering of HTML, for the preview line when there's no text part.
fn strip_tags(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mailwright_store::Addr;

    #[test]
    fn build_then_parse_round_trips() {
        let from = Addr::new("Tony Johncock", "tony@example.com");
        let to = vec![Addr::new("Amy", "amy@example.org")];
        let cc = vec![Addr::new("", "cc@example.net")];
        let png = b"\x89PNG\r\n\x1a\n-not-really".to_vec();
        let atts = vec![OutAttach { name: "note.txt", content_type: "text/plain", bytes: b"hello file", content_id: None }, OutAttach { name: "pic.png", content_type: "image/png", bytes: &png, content_id: Some("img1") }];
        let mail = OutMail {
            from: &from,
            to: &to,
            cc: &cc,
            subject: "Lunch on Thursday? \u{2013} caf\u{e9}",
            html: "<p>Hi <b>Amy</b>, see the <img src=\"cid:img1\"> attached.</p>",
            text: "Hi Amy, see attached.",
            attachments: &atts,
            message_id: "mw-test@example.com",
            in_reply_to: None,
            importance: "high",
            request_read_receipt: false,
        };
        let raw = build(&mail);

        let parsed = parse(&raw, "imap\u{1}INBOX\u{1}1\u{1}42", "INBOX", false, false);
        let m = parsed.message;
        assert_eq!(m.subject.as_deref(), Some("Lunch on Thursday? \u{2013} caf\u{e9}"));
        assert_eq!(m.from.as_ref().and_then(|r| r.email_address.address.as_deref()), Some("tony@example.com"));
        assert_eq!(m.from.as_ref().and_then(|r| r.email_address.name.as_deref()), Some("Tony Johncock"));
        assert_eq!(m.to_recipients.first().and_then(|r| r.email_address.address.as_deref()), Some("amy@example.org"));
        assert_eq!(m.cc_recipients.first().and_then(|r| r.email_address.address.as_deref()), Some("cc@example.net"));
        assert!(m.body.as_ref().is_some_and(|b| b.content.contains("Amy")));
        assert_eq!(m.has_attachments, Some(true));

        // Both attachments survive, with names and the inline image's content-id.
        assert_eq!(parsed.attachments.len(), 2);
        let pic = parsed.attachments.iter().find(|a| a.name == "pic.png").expect("png attachment");
        assert_eq!(pic.bytes, png);
        assert_eq!(pic.content_id.as_deref(), Some("img1"));
        assert!(pic.is_inline);
        let note = parsed.attachments.iter().find(|a| a.name == "note.txt").expect("text attachment");
        assert_eq!(note.bytes, b"hello file");
    }

    #[test]
    fn html_to_text_keeps_breaks() {
        let t = html_to_text("<p>Line one</p><p>Line two<br>still two</p>");
        assert!(t.contains("Line one"));
        assert!(t.contains("Line two"));
        assert!(t.contains("still two"));
    }
}
