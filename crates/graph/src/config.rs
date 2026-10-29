use serde::{Deserialize, Serialize};

/// The public Microsoft endpoints. Overridable for testing against a mock server with the
/// `MAILWRIGHT_LOGIN_BASE` and `MAILWRIGHT_GRAPH_BASE` environment variables.
pub const LOGIN_BASE: &str = "https://login.microsoftonline.com";
pub const GRAPH_BASE: &str = "https://graph.microsoft.com/v1.0";

/// The app registration built into this copy of Mailwright (set with `MAILWRIGHT_CLIENT_ID` when
/// building): a multi-tenant registration, so users only ever type their email address.
pub const BUILTIN_CLIENT_ID: Option<&str> = match option_env!("MAILWRIGHT_CLIENT_ID") {
    Some(id) => Some(id),
    None => Some(MAILWRIGHT_APP_ID),
};

/// Mailwright's multi-tenant app registration (any Entra ID tenant and personal Microsoft accounts).
pub const MAILWRIGHT_APP_ID: &str = "6b43e75b-9b99-4d73-8575-390390a86137";

/// The client ID to use: an override (an organisation's own registration), else the built-in one.
pub fn client_id(override_id: &str) -> Option<String> {
    let o = override_id.trim();
    if is_guid(o) {
        return Some(o.to_string());
    }
    std::env::var("MAILWRIGHT_CLIENT_ID").ok().filter(|v| is_guid(v.trim())).or(BUILTIN_CLIENT_ID.map(str::to_string)).filter(|v| is_guid(v.trim()))
}

/// Delegated permissions Mailwright asks for. All are user-consentable in a default tenant;
/// an organisation that requires admin consent grants them once for everyone.
pub const SCOPES: &[&str] = &[
    "offline_access",
    "openid",
    "profile",
    "email",
    "User.Read",
    "Mail.ReadWrite",
    "Mail.Send",
    "Calendars.ReadWrite",
    "People.Read",
    "MailboxSettings.ReadWrite",
];

/// One mailbox: which app registration and tenant to sign in with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct AccountConfig {
    /// Application (client) ID of the app registration in Microsoft Entra ID.
    pub client_id: String,
    /// `organizations` (any work or school account), `common` (also personal Microsoft
    /// accounts), or a tenant ID or domain.
    pub tenant: String,
    /// The signed-in user's address, once known (also the credential store's account name).
    pub username: String,
    pub display_name: String,
    /// Sign-in and Graph endpoints other than Microsoft's (the sample mailbox, tests).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub login_base: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_base: Option<String>,
    /// Set for a plain IMAP/POP3 + SMTP mailbox instead of Microsoft 365. When present, the account
    /// uses these servers rather than Microsoft Graph.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manual: Option<ManualConfig>,
}

impl AccountConfig {
    /// Whether this is a plain IMAP/POP3 mailbox rather than a Microsoft 365 one.
    pub fn is_manual(&self) -> bool {
        self.manual.is_some()
    }
}

/// How a message is received from a plain mailbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MailProtocol {
    #[default]
    Imap,
    Pop,
}

/// How a connection to a server is secured.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Security {
    /// Secure from the first byte (implicit TLS): IMAP 993, POP3 995, SMTP 465.
    #[default]
    Tls,
    /// Start plain, then upgrade with STARTTLS: usually SMTP 587, IMAP 143.
    StartTls,
    /// No encryption (not recommended; for a local server only).
    None,
}

/// One server's address.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub security: Security,
}

/// A plain mailbox's incoming (IMAP/POP3) and outgoing (SMTP) servers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct ManualConfig {
    pub protocol: MailProtocol,
    pub incoming: ServerConfig,
    pub smtp: ServerConfig,
    /// The user name to sign in with (often the email address itself).
    pub login: String,
}

impl AccountConfig {
    pub fn login_base(&self) -> String {
        self.login_base.clone().or_else(|| std::env::var("MAILWRIGHT_LOGIN_BASE").ok()).unwrap_or_else(|| LOGIN_BASE.to_string())
    }

    pub fn graph_base(&self) -> String {
        self.graph_base.clone().or_else(|| std::env::var("MAILWRIGHT_GRAPH_BASE").ok()).unwrap_or_else(|| GRAPH_BASE.to_string())
    }

    pub fn tenant_or_default(&self) -> &str {
        let t = self.tenant.trim();
        if t.is_empty() { "organizations" } else { t }
    }

    /// A client ID is a GUID; a tenant is `organizations`, `common`, `consumers`, a GUID or a
    /// domain name. Returns what's wrong, for the sign-in screen.
    pub fn validate(&self) -> Result<(), String> {
        if !is_guid(self.client_id.trim()) {
            return Err("The Application (client) ID is a GUID, like 1b2c3d4e-0000-1111-2222-333344445555.".into());
        }
        let t = self.tenant_or_default();
        let ok = matches!(t, "organizations" | "common" | "consumers")
            || is_guid(t)
            || (t.contains('.') && t.len() <= 253 && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-'));
        if !ok {
            return Err("The tenant is \"organizations\", your tenant ID, or your domain (contoso.com).".into());
        }
        Ok(())
    }

    pub fn authority(&self) -> String {
        format!("{}/{}", self.login_base().trim_end_matches('/'), self.tenant_or_default())
    }
}

pub fn is_guid(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 5
        && [8, 4, 4, 4, 12].iter().zip(&parts).all(|(n, p)| p.len() == *n && p.chars().all(|c| c.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_client_id_and_tenant() {
        let mut c = AccountConfig { client_id: "1b2c3d4e-0000-1111-2222-333344445555".into(), ..Default::default() };
        assert!(c.validate().is_ok());
        assert_eq!(c.tenant_or_default(), "organizations");
        c.tenant = "contoso.onmicrosoft.com".into();
        assert!(c.validate().is_ok());
        c.tenant = "bad tenant!".into();
        assert!(c.validate().is_err());
        c.tenant = String::new();
        c.client_id = "not-a-guid".into();
        assert!(c.validate().is_err());
    }
}
