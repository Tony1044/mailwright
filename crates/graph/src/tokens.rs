//! Where the refresh token lives between runs: the operating system's credential store (Windows
//! Credential Manager; the Secret Service on Linux, i.e. GNOME Keyring or KWallet). Access tokens
//! stay in memory only. When no credential store is available (a minimal Linux desktop), the token
//! goes in a file only the user can read, and [`TokenStore::location`] says so.

use std::path::PathBuf;
use std::sync::Mutex;

use crate::error::GraphError;

const SERVICE: &str = "MailCraft";

pub trait TokenStore: Send + Sync {
    fn load(&self, account: &str) -> Result<Option<String>, GraphError>;
    fn save(&self, account: &str, refresh_token: &str) -> Result<(), GraphError>;
    fn clear(&self, account: &str) -> Result<(), GraphError>;
    /// Where the token is kept, for the account settings page.
    fn location(&self) -> String;
}

/// The credential store, falling back to a private file.
pub struct SystemTokenStore {
    fallback_dir: PathBuf,
}

impl SystemTokenStore {
    pub fn new(fallback_dir: PathBuf) -> SystemTokenStore {
        SystemTokenStore { fallback_dir }
    }

    fn entry(account: &str) -> Result<keyring::Entry, keyring::Error> {
        keyring::Entry::new(SERVICE, account)
    }

    fn file(&self, account: &str) -> PathBuf {
        let safe: String = account.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '@' { c } else { '_' }).collect();
        self.fallback_dir.join(format!("token-{safe}"))
    }

    fn keyring_works(&self) -> bool {
        // A probe entry: some Linux sessions have no Secret Service at all.
        match Self::entry("mailcraft-probe") {
            Ok(e) => !matches!(e.get_password(), Err(keyring::Error::PlatformFailure(_)) | Err(keyring::Error::NoStorageAccess(_))),
            Err(_) => false,
        }
    }
}

impl TokenStore for SystemTokenStore {
    fn load(&self, account: &str) -> Result<Option<String>, GraphError> {
        if let Ok(e) = Self::entry(account) {
            match e.get_password() {
                Ok(t) => return Ok(Some(t)),
                Err(keyring::Error::NoEntry) => {}
                Err(err) => log::warn!("credential store: {err}"),
            }
        }
        match std::fs::read_to_string(self.file(account)) {
            Ok(t) if !t.trim().is_empty() => Ok(Some(t.trim().to_string())),
            _ => Ok(None),
        }
    }

    fn save(&self, account: &str, refresh_token: &str) -> Result<(), GraphError> {
        if let Ok(e) = Self::entry(account)
            && e.set_password(refresh_token).is_ok()
        {
            let _ = std::fs::remove_file(self.file(account));
            return Ok(());
        }
        std::fs::create_dir_all(&self.fallback_dir).map_err(|e| GraphError::Credentials(e.to_string()))?;
        let path = self.file(account);
        write_private(&path, refresh_token.as_bytes()).map_err(|e| GraphError::Credentials(format!("{}: {e}", path.display())))
    }

    fn clear(&self, account: &str) -> Result<(), GraphError> {
        if let Ok(e) = Self::entry(account) {
            let _ = e.delete_credential();
        }
        let _ = std::fs::remove_file(self.file(account));
        Ok(())
    }

    fn location(&self) -> String {
        if self.keyring_works() {
            if cfg!(windows) { "Windows Credential Manager".into() } else { "the desktop keyring (Secret Service)".into() }
        } else {
            format!("a private file in {}", self.fallback_dir.display())
        }
    }
}

#[cfg(unix)]
fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
    f.write_all(bytes)
}

#[cfg(not(unix))]
fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)
}

/// In memory only (tests, and the sample mailbox).
#[derive(Default)]
pub struct MemoryTokenStore {
    tokens: Mutex<std::collections::HashMap<String, String>>,
}

impl TokenStore for MemoryTokenStore {
    fn load(&self, account: &str) -> Result<Option<String>, GraphError> {
        Ok(self.tokens.lock().map_err(|_| GraphError::Credentials("poisoned".into()))?.get(account).cloned())
    }
    fn save(&self, account: &str, refresh_token: &str) -> Result<(), GraphError> {
        self.tokens.lock().map_err(|_| GraphError::Credentials("poisoned".into()))?.insert(account.to_string(), refresh_token.to_string());
        Ok(())
    }
    fn clear(&self, account: &str) -> Result<(), GraphError> {
        self.tokens.lock().map_err(|_| GraphError::Credentials("poisoned".into()))?.remove(account);
        Ok(())
    }
    fn location(&self) -> String {
        "memory".into()
    }
}
