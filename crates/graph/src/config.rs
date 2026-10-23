use serde::{Deserialize, Serialize};

/// The public Microsoft endpoints. Overridable for testing against a mock server with the
/// `MAILCRAFT_LOGIN_BASE` and `MAILCRAFT_GRAPH_BASE` environment variables.
pub const LOGIN_BASE: &str = "https://login.microsoftonline.com";
pub const GRAPH_BASE: &str = "https://graph.microsoft.com/v1.0";

/// Delegated permissions MailCraft asks for. All are user-consentable in a default tenant;
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
}

impl AccountConfig {
    pub fn login_base(&self) -> String {
        self.login_base.clone().or_else(|| std::env::var("MAILCRAFT_LOGIN_BASE").ok()).unwrap_or_else(|| LOGIN_BASE.to_string())
    }

    pub fn graph_base(&self) -> String {
        self.graph_base.clone().or_else(|| std::env::var("MAILCRAFT_GRAPH_BASE").ok()).unwrap_or_else(|| GRAPH_BASE.to_string())
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
