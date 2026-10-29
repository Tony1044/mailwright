//! Sending a message over SMTP, with implicit TLS or STARTTLS and LOGIN/PLAIN authentication.

use std::io::Write as _;
use std::time::Duration;

use base64::Engine as _;
use mailwright_graph::{Security, ServerConfig};

use crate::tls::{Line, Stream};
use crate::NetError;

const TIMEOUT: Duration = Duration::from_secs(30);

/// Read one SMTP reply (handling multi-line replies) and check its code.
fn reply(line: &mut Line<Stream>, want: &[&str]) -> Result<String, NetError> {
    #[allow(unused_assignments)]
    let mut last = String::new();
    loop {
        let l = line.read_line()?;
        let cont = l.len() >= 4 && l.as_bytes()[3] == b'-';
        last = l;
        if !cont {
            break;
        }
    }
    if want.iter().any(|w| last.starts_with(w)) {
        Ok(last)
    } else if last.starts_with("5") && (last.contains("auth") || last.contains("Auth") || last.contains("5.7")) {
        Err(NetError::Auth(last))
    } else {
        Err(NetError::Protocol(format!("the mail server said: {last}")))
    }
}

fn cmd(line: &mut Line<Stream>, text: &str, want: &[&str]) -> Result<String, NetError> {
    line.write_line(text)?;
    reply(line, want)
}

/// Send `data` (a complete RFC 822 message) from `from` to `rcpts`.
pub fn send(cfg: &ServerConfig, login: &str, password: &str, from: &str, rcpts: &[String], data: &[u8]) -> Result<(), NetError> {
    if cfg.host.trim().is_empty() {
        return Err(NetError::Unsupported("No outgoing (SMTP) server is set for this account.".into()));
    }
    let implicit = cfg.security == Security::Tls;
    let stream = Stream::connect(&cfg.host, cfg.port, implicit, TIMEOUT)?;
    let mut line = Line::new(stream);
    reply(&mut line, &["220"])?;

    let ehlo = "mailwright";
    let caps = cmd(&mut line, &format!("EHLO {ehlo}"), &["250"])?;
    let mut caps = caps;

    if cfg.security == Security::StartTls && !line.inner.is_tls() {
        cmd(&mut line, "STARTTLS", &["220"])?;
        let upgraded = line.into_inner().start_tls(&cfg.host)?;
        line = Line::new(upgraded);
        caps = cmd(&mut line, &format!("EHLO {ehlo}"), &["250"])?;
    }

    if !login.is_empty() {
        authenticate(&mut line, &caps, login, password)?;
    }

    cmd(&mut line, &format!("MAIL FROM:<{from}>"), &["250"])?;
    for r in rcpts {
        cmd(&mut line, &format!("RCPT TO:<{r}>"), &["250", "251"])?;
    }
    cmd(&mut line, "DATA", &["354"])?;
    write_data(&mut line, data)?;
    reply(&mut line, &["250"])?;
    let _ = cmd(&mut line, "QUIT", &["221"]);
    Ok(())
}

/// Pick an auth method the server offers and sign in.
fn authenticate(line: &mut Line<Stream>, caps: &str, login: &str, password: &str) -> Result<(), NetError> {
    let caps = caps.to_ascii_uppercase();
    let b64 = |s: &str| base64::engine::general_purpose::STANDARD.encode(s.as_bytes());
    if caps.contains("LOGIN") {
        cmd(line, "AUTH LOGIN", &["334"])?;
        cmd(line, &b64(login), &["334"])?;
        cmd(line, &b64(password), &["235"])?;
        Ok(())
    } else {
        // AUTH PLAIN: \0 user \0 pass
        let token = base64::engine::general_purpose::STANDARD.encode(format!("\0{login}\0{password}").as_bytes());
        cmd(line, &format!("AUTH PLAIN {token}"), &["235"])?;
        Ok(())
    }
}

/// Write the message body with dot-stuffing, then the end-of-data marker.
fn write_data(line: &mut Line<Stream>, data: &[u8]) -> Result<(), NetError> {
    let text = String::from_utf8_lossy(data);
    let normalized = text.replace("\r\n", "\n");
    let mut out = Vec::with_capacity(data.len() + 16);
    for l in normalized.split('\n') {
        if l.starts_with('.') {
            out.push(b'.');
        }
        out.extend_from_slice(l.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    line.inner.write_all(&out).map_err(|e| NetError::Io(e.to_string()))?;
    line.inner.write_all(b".\r\n").map_err(|e| NetError::Io(e.to_string()))?;
    line.inner.flush().map_err(|e| NetError::Io(e.to_string()))
}
