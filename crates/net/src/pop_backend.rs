//! POP3: a download-only mailbox with a single inbox, no folders, and no server-side flags.
//! Read and flag changes are kept on this computer only; deleting removes the message from the
//! server. Sending is over SMTP, like IMAP.

use std::collections::HashSet;
use std::time::Duration;

use mailwright_graph::types::{MailFolder, Message};
use mailwright_graph::{ManualConfig, Security};

use crate::tls::{Line, Stream};
use crate::{mime, NetError};

const TIMEOUT: Duration = Duration::from_secs(30);
/// The one folder a POP3 account has.
pub const INBOX_ID: &str = "pop-inbox";

pub struct PopBackend {
    cfg: ManualConfig,
    password: String,
}

impl PopBackend {
    pub fn new(cfg: ManualConfig, password: String) -> PopBackend {
        PopBackend { cfg, password }
    }

    pub fn inbox_folder() -> MailFolder {
        MailFolder { id: INBOX_ID.into(), display_name: "Inbox".into(), total_item_count: 0, ..Default::default() }
    }

    /// Check the connection and sign-in work, for the setup page's "Test" button.
    pub fn check(&self) -> Result<(), NetError> {
        let _ = self.session()?;
        Ok(())
    }

    /// The newest messages not already known, parsed, up to `max`. Each id is `pop:<uidl>`.
    pub fn fetch_new(&self, known: &HashSet<String>, max: usize) -> Result<Vec<Message>, NetError> {
        let mut s = self.session()?;
        let uidls = s.uidl()?; // (msg_number, uid)
        let mut out = Vec::new();
        // Newest are usually highest-numbered; take from the end.
        for (num, uid) in uidls.iter().rev() {
            let id = format!("pop:{uid}");
            if known.contains(&id) {
                continue;
            }
            if out.len() >= max {
                break;
            }
            let raw = s.retr(*num)?;
            out.push(mime::parse(&raw, &id, INBOX_ID, false, false).message);
        }
        let _ = s.quit();
        Ok(out)
    }

    /// One message with its body and attachments.
    pub fn fetch_full(&self, id: &str) -> Result<mime::Parsed, NetError> {
        let uid = id.strip_prefix("pop:").unwrap_or(id);
        let mut s = self.session()?;
        let num = s.uidl()?.into_iter().find(|(_, u)| u == uid).map(|(n, _)| n).ok_or_else(|| NetError::Protocol("that message is no longer on the server".into()))?;
        let raw = s.retr(num)?;
        let _ = s.quit();
        Ok(mime::parse(&raw, id, INBOX_ID, true, false))
    }

    /// Delete messages from the server (applied when the session ends).
    pub fn delete(&self, ids: &[String]) -> Result<(), NetError> {
        let want: HashSet<&str> = ids.iter().map(|i| i.strip_prefix("pop:").unwrap_or(i)).collect();
        let mut s = self.session()?;
        for (num, uid) in s.uidl()? {
            if want.contains(uid.as_str()) {
                s.dele(num)?;
            }
        }
        s.quit()
    }

    /// Send a message over SMTP (POP3 has no server-side Sent folder to copy into).
    pub fn send(&self, from: &str, rcpts: &[String], raw: &[u8]) -> Result<(), NetError> {
        crate::smtp::send(&self.cfg.smtp, &self.cfg.login, &self.password, from, rcpts, raw)
    }

    fn session(&self) -> Result<Pop, NetError> {
        Pop::open(&self.cfg, &self.password)
    }
}

/// One POP3 session.
struct Pop {
    line: Line<Stream>,
}

impl Pop {
    fn open(cfg: &ManualConfig, password: &str) -> Result<Pop, NetError> {
        let inc = &cfg.incoming;
        let implicit = inc.security == Security::Tls;
        let stream = Stream::connect(&inc.host, inc.port, implicit, TIMEOUT)?;
        let mut pop = Pop { line: Line::new(stream) };
        pop.ok()?; // greeting
        if cfg.incoming.security == Security::StartTls && !pop.line.inner.is_tls() {
            pop.cmd("STLS")?;
            let upgraded = pop.line.into_inner().start_tls(&inc.host)?;
            pop.line = Line::new(upgraded);
        }
        pop.cmd(&format!("USER {}", cfg.login))?;
        match pop.cmd(&format!("PASS {password}")) {
            Ok(_) => {}
            Err(NetError::Protocol(m)) => return Err(NetError::Auth(m)),
            Err(e) => return Err(e),
        }
        Ok(pop)
    }

    /// Read a single status line and require +OK.
    fn ok(&mut self) -> Result<String, NetError> {
        let l = self.line.read_line()?;
        if l.starts_with("+OK") {
            Ok(l)
        } else {
            Err(NetError::Protocol(l.trim_start_matches("-ERR").trim().to_string()))
        }
    }

    fn cmd(&mut self, c: &str) -> Result<String, NetError> {
        self.line.write_line(c)?;
        self.ok()
    }

    /// Read a dot-terminated multi-line response (after the +OK status).
    fn read_multiline(&mut self) -> Result<Vec<String>, NetError> {
        let mut lines = Vec::new();
        loop {
            let l = self.line.read_line()?;
            if l == "." {
                break;
            }
            lines.push(l.strip_prefix("..").map(|r| format!(".{r}")).unwrap_or(l));
        }
        Ok(lines)
    }

    /// (message number, unique id) for every message.
    fn uidl(&mut self) -> Result<Vec<(u32, String)>, NetError> {
        self.cmd("UIDL")?;
        let lines = self.read_multiline()?;
        let mut out = Vec::new();
        for l in lines {
            let mut parts = l.split_whitespace();
            if let (Some(n), Some(uid)) = (parts.next().and_then(|n| n.parse().ok()), parts.next()) {
                out.push((n, uid.to_string()));
            }
        }
        Ok(out)
    }

    fn retr(&mut self, num: u32) -> Result<Vec<u8>, NetError> {
        self.cmd(&format!("RETR {num}"))?;
        let lines = self.read_multiline()?;
        Ok(lines.join("\r\n").into_bytes())
    }

    fn dele(&mut self, num: u32) -> Result<(), NetError> {
        self.cmd(&format!("DELE {num}")).map(|_| ())
    }

    fn quit(&mut self) -> Result<(), NetError> {
        self.cmd("QUIT").map(|_| ())
    }
}
