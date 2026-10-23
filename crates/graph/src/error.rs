use thiserror::Error;

/// Anything that can go wrong talking to Microsoft Entra ID or Graph.
#[derive(Debug, Error)]
pub enum GraphError {
    /// No network, DNS failure, TLS failure, timeout.
    #[error("can't reach {host}: {detail}")]
    Network { host: String, detail: String },
    /// Graph answered with an error status.
    #[error("{status} {code}: {message}")]
    Api { status: u16, code: String, message: String },
    /// The sign-in needs the user again (expired or revoked refresh token, consent missing).
    #[error("sign-in required: {0}")]
    SignInRequired(String),
    /// The user cancelled sign-in, or it timed out.
    #[error("sign-in cancelled: {0}")]
    SignInCancelled(String),
    /// A response that isn't the JSON we expect.
    #[error("unexpected response: {0}")]
    BadResponse(String),
    /// Local configuration problem (missing client id, bad tenant).
    #[error("{0}")]
    Config(String),
    /// The operating system's credential store failed.
    #[error("credential store: {0}")]
    Credentials(String),
}

impl GraphError {
    /// Worth retrying later (offline, server busy, throttled).
    pub fn is_transient(&self) -> bool {
        match self {
            GraphError::Network { .. } => true,
            GraphError::Api { status, .. } => matches!(*status, 408 | 429 | 500 | 502 | 503 | 504),
            _ => false,
        }
    }

    /// The item no longer exists on the server (deleted elsewhere).
    pub fn is_not_found(&self) -> bool {
        matches!(self, GraphError::Api { status: 404, .. })
    }
}
