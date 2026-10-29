//! IMAP, POP3 and SMTP for Mailwright, so it works with any mailbox, not only Microsoft 365.
//!
//! Each backend produces the same [`mailwright_graph::types::Message`] and `MailFolder` values the
//! Graph client does, so the store and the interface don't care which protocol a mailbox uses.

#![forbid(unsafe_code)]

pub mod imap_backend;
pub mod mime;
pub mod pop_backend;
pub mod providers;
pub mod smtp;
pub mod tls;

use std::fmt;

/// Anything that can go wrong talking to an IMAP, POP3 or SMTP server.
#[derive(Debug, Clone)]
pub enum NetError {
    Connect(String),
    Tls(String),
    Io(String),
    /// The server rejected the sign-in (bad username or password).
    Auth(String),
    /// The server refused a command.
    Protocol(String),
    /// Something we can't do over this protocol (e.g. folders over POP3).
    Unsupported(String),
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetError::Connect(m) => write!(f, "couldn't connect: {m}"),
            NetError::Tls(m) => write!(f, "secure connection failed: {m}"),
            NetError::Io(m) => write!(f, "{m}"),
            NetError::Auth(m) => write!(f, "sign-in was refused: {m}"),
            NetError::Protocol(m) => write!(f, "{m}"),
            NetError::Unsupported(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for NetError {}

/// Turn a protocol error into the shared GraphError the engine understands, so a bad password asks
/// the person to sign in again and a dropped connection reads as "working offline".
pub fn to_graph_error(e: NetError) -> mailwright_graph::GraphError {
    use mailwright_graph::GraphError;
    match e {
        NetError::Auth(m) => GraphError::SignInRequired(m),
        NetError::Connect(m) | NetError::Tls(m) | NetError::Io(m) => GraphError::Network { host: String::new(), detail: m },
        NetError::Unsupported(m) => GraphError::Config(m),
        NetError::Protocol(m) => GraphError::BadResponse(m),
    }
}
