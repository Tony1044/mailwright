//! Working out what kind of account an email address is, the way Outlook does before it signs
//! in: Microsoft's realm lookup for the address (a Microsoft 365 organisation, managed or
//! federated, or a personal Microsoft account), backed up by the domain's MX records (mail hosted
//! by Exchange Online, or by someone else entirely).

use serde::Deserialize;

use crate::error::GraphError;
use crate::http;

/// Where to look things up (overridable for tests and the sample mailbox).
#[derive(Clone, Debug)]
pub struct DiscoveryConfig {
    pub login_base: String,
    /// DNS over HTTPS servers answering `?name=…&type=MX` in JSON.
    pub doh: Vec<String>,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        DiscoveryConfig {
            login_base: std::env::var("MAILWRIGHT_LOGIN_BASE").unwrap_or_else(|_| crate::config::LOGIN_BASE.to_string()),
            doh: match std::env::var("MAILWRIGHT_DOH") {
                Ok(v) => vec![v],
                Err(_) => vec!["https://cloudflare-dns.com/dns-query".into(), "https://dns.google/resolve".into()],
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccountKind {
    /// A work or school account in a Microsoft 365 organisation. `tenant` is what to sign in
    /// against (the domain); `federated` names the organisation's own sign-in service, if any.
    Work { tenant: String, organisation: String, federated: bool },
    /// Outlook.com, Hotmail, Live.
    Personal,
    /// Mail for this domain isn't Exchange Online (`provider` says who, if we can tell).
    Elsewhere { provider: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Discovery {
    pub email: String,
    pub domain: String,
    pub kind: AccountKind,
    pub mx: Vec<String>,
}

impl Discovery {
    /// The tenant to sign in against.
    pub fn tenant(&self) -> Option<String> {
        match &self.kind {
            AccountKind::Work { tenant, .. } => Some(tenant.clone()),
            AccountKind::Personal => Some("consumers".into()),
            AccountKind::Elsewhere { .. } => None,
        }
    }

    /// One line for the sign-in screen.
    pub fn describe(&self) -> String {
        match &self.kind {
            AccountKind::Work { organisation, federated, .. } => {
                let org = if organisation.is_empty() { self.domain.clone() } else { organisation.clone() };
                if *federated { format!("Microsoft 365 work or school account ({org}, signing in through your organisation)") } else { format!("Microsoft 365 work or school account ({org})") }
            }
            AccountKind::Personal => "Personal Microsoft account (Outlook.com)".into(),
            AccountKind::Elsewhere { provider } if !provider.is_empty() => format!("Mail for {} is hosted by {provider}, not Microsoft 365", self.domain),
            AccountKind::Elsewhere { .. } => format!("We couldn't find a Microsoft 365 or Outlook.com account for {}", self.domain),
        }
    }
}

const CONSUMER_DOMAINS: &[&str] = &["outlook.com", "hotmail.com", "hotmail.co.uk", "live.com", "live.co.uk", "msn.com", "passport.com", "outlook.co.uk", "windowslive.com"];

pub fn is_consumer_domain(domain: &str) -> bool {
    let d = domain.to_ascii_lowercase();
    CONSUMER_DOMAINS.contains(&d.as_str()) || d.starts_with("hotmail.") || d.starts_with("outlook.") || d.starts_with("live.")
}

/// A plausible email address: something@domain.tld.
pub fn valid_email(email: &str) -> bool {
    let e = email.trim();
    match e.rsplit_once('@') {
        Some((local, domain)) => {
            !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.') && !e.contains(char::is_whitespace) && domain.chars().all(|c| c.is_alphanumeric() || c == '.' || c == '-')
        }
        None => false,
    }
}

#[derive(Deserialize, Default, Debug)]
#[serde(default)]
struct Realm {
    #[serde(rename = "NameSpaceType")]
    namespace_type: String,
    #[serde(rename = "FederationBrandName")]
    brand: Option<String>,
    #[serde(rename = "DomainName")]
    domain: Option<String>,
}

#[derive(Deserialize, Default, Debug)]
#[serde(default)]
struct DohAnswer {
    #[serde(rename = "Answer")]
    answer: Vec<DohRecord>,
}

#[derive(Deserialize, Default, Debug)]
#[serde(default)]
struct DohRecord {
    #[serde(rename = "type")]
    kind: u16,
    data: String,
}

fn realm(cfg: &DiscoveryConfig, email: &str) -> Result<Realm, GraphError> {
    let mut u = url::Url::parse(&format!("{}/common/userrealm/", cfg.login_base.trim_end_matches('/'))).map_err(|e| GraphError::Config(e.to_string()))?;
    u.path_segments_mut().map_err(|_| GraphError::Config("bad login base".into()))?.pop_if_empty().push(email);
    u.query_pairs_mut().append_pair("api-version", "2.1");
    let url = u.to_string();
    let mut resp = http::agent().get(&url).call().map_err(|e| http::network_error(&url, e))?;
    let text = http::read_text(&mut resp, &url)?;
    serde_json::from_str(&text).map_err(|e| GraphError::BadResponse(format!("realm: {e}")))
}

/// MX hosts for a domain, by DNS over HTTPS (empty if no server answered).
pub fn mx_records(cfg: &DiscoveryConfig, domain: &str) -> Vec<String> {
    for server in &cfg.doh {
        let mut u = match url::Url::parse(server) {
            Ok(u) => u,
            Err(_) => continue,
        };
        u.query_pairs_mut().append_pair("name", domain).append_pair("type", "MX");
        let url = u.to_string();
        let Ok(mut resp) = http::agent().get(&url).header("Accept", "application/dns-json").call() else { continue };
        if !resp.status().is_success() {
            continue;
        }
        let Ok(text) = http::read_text(&mut resp, &url) else { continue };
        let Ok(ans) = serde_json::from_str::<DohAnswer>(&text) else { continue };
        let mut hosts: Vec<(u32, String)> = ans
            .answer
            .iter()
            .filter(|r| r.kind == 15)
            .filter_map(|r| {
                let mut parts = r.data.split_whitespace();
                let pref = parts.next()?.parse().ok()?;
                Some((pref, parts.next()?.trim_end_matches('.').to_ascii_lowercase()))
            })
            .collect();
        hosts.sort();
        return hosts.into_iter().map(|(_, h)| h).collect();
    }
    Vec::new()
}

fn provider_of(mx: &[String]) -> Option<&'static str> {
    let any = |pat: &str| mx.iter().any(|h| h.ends_with(pat));
    if any(".mail.protection.outlook.com") || any(".olc.protection.outlook.com") || any("outlook.com") {
        Some("Microsoft 365")
    } else if any("google.com") || any("googlemail.com") {
        Some("Google")
    } else if any("yahoodns.net") {
        Some("Yahoo")
    } else if any("icloud.com") {
        Some("Apple iCloud")
    } else if any("zoho.com") || any("zoho.eu") {
        Some("Zoho")
    } else if any("protonmail.ch") {
        Some("Proton")
    } else {
        None
    }
}

/// Find out what kind of account `email` is.
pub fn discover(cfg: &DiscoveryConfig, email: &str) -> Result<Discovery, GraphError> {
    let email = email.trim().to_string();
    if !valid_email(&email) {
        return Err(GraphError::Config("Enter an email address, like name@example.com.".into()));
    }
    let domain = email.rsplit_once('@').map(|(_, d)| d.to_ascii_lowercase()).unwrap_or_default();
    if is_consumer_domain(&domain) {
        return Ok(Discovery { email, domain, kind: AccountKind::Personal, mx: Vec::new() });
    }
    let realm = realm(cfg, &email);
    let mx = mx_records(cfg, &domain);
    let kind = match realm {
        Ok(r) if r.namespace_type.eq_ignore_ascii_case("Managed") || r.namespace_type.eq_ignore_ascii_case("Federated") => AccountKind::Work {
            tenant: r.domain.clone().filter(|d| !d.is_empty()).unwrap_or_else(|| domain.clone()).to_ascii_lowercase(),
            organisation: r.brand.unwrap_or_default(),
            federated: r.namespace_type.eq_ignore_ascii_case("Federated"),
        },
        Ok(_) => match provider_of(&mx) {
            Some("Microsoft 365") => AccountKind::Work { tenant: domain.clone(), organisation: String::new(), federated: false },
            Some(p) => AccountKind::Elsewhere { provider: p.to_string() },
            None => AccountKind::Elsewhere { provider: String::new() },
        },
        Err(e) => {
            // No answer from Microsoft: the MX records may still say it's Exchange Online.
            match provider_of(&mx) {
                Some("Microsoft 365") => AccountKind::Work { tenant: domain.clone(), organisation: String::new(), federated: false },
                Some(p) => AccountKind::Elsewhere { provider: p.to_string() },
                None => return Err(e),
            }
        }
    };
    Ok(Discovery { email, domain, kind, mx })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emails_and_consumer_domains() {
        assert!(valid_email("tony@contoso.co.uk"));
        assert!(!valid_email("tony@contoso"));
        assert!(!valid_email("tony contoso.com"));
        assert!(is_consumer_domain("Hotmail.co.uk"));
        assert!(!is_consumer_domain("contoso.com"));
    }

    #[test]
    fn mx_hosts_name_the_provider() {
        assert_eq!(provider_of(&["contoso-com.mail.protection.outlook.com".into()]), Some("Microsoft 365"));
        assert_eq!(provider_of(&["aspmx.l.google.com".into()]), Some("Google"));
        assert_eq!(provider_of(&["mx.example.net".into()]), None);
    }
}
