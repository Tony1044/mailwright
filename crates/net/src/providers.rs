//! Server settings for common mail providers, so most people only type their address and password.
//! When a provider isn't known, a sensible generic guess (imap./smtp. on the domain) is offered.

use mailwright_graph::{MailProtocol, ManualConfig, Security, ServerConfig};

/// A known provider's servers, found from the email domain.
struct Provider {
    /// Domains this applies to (after the @).
    domains: &'static [&'static str],
    imap_host: &'static str,
    smtp_host: &'static str,
    smtp_port: u16,
    smtp_security: Security,
    /// A note shown on the setup page (e.g. app passwords).
    note: &'static str,
}

const PROVIDERS: &[Provider] = &[
    Provider {
        domains: &["gmail.com", "googlemail.com"],
        imap_host: "imap.gmail.com",
        smtp_host: "smtp.gmail.com",
        smtp_port: 465,
        smtp_security: Security::Tls,
        note: "Gmail needs an app password (Google Account \u{203a} Security \u{203a} App passwords), not your normal password, and IMAP turned on in Gmail settings.",
    },
    Provider {
        domains: &["yahoo.com", "yahoo.co.uk", "ymail.com"],
        imap_host: "imap.mail.yahoo.com",
        smtp_host: "smtp.mail.yahoo.com",
        smtp_port: 465,
        smtp_security: Security::Tls,
        note: "Yahoo needs an app password (Account Security \u{203a} Generate app password).",
    },
    Provider {
        domains: &["outlook.com", "hotmail.com", "live.com", "msn.com"],
        imap_host: "outlook.office365.com",
        smtp_host: "smtp.office365.com",
        smtp_port: 587,
        smtp_security: Security::StartTls,
        note: "For a personal Outlook.com account you can also use the Microsoft 365 sign-in on the first screen, which is more reliable.",
    },
    Provider {
        domains: &["icloud.com", "me.com", "mac.com"],
        imap_host: "imap.mail.me.com",
        smtp_host: "smtp.mail.me.com",
        smtp_port: 587,
        smtp_security: Security::StartTls,
        note: "iCloud needs an app-specific password (appleid.apple.com \u{203a} Sign-In and Security).",
    },
    Provider {
        domains: &["aol.com"],
        imap_host: "imap.aol.com",
        smtp_host: "smtp.aol.com",
        smtp_port: 465,
        smtp_security: Security::Tls,
        note: "AOL needs an app password.",
    },
    Provider {
        domains: &["gmx.com", "gmx.co.uk", "gmx.net"],
        imap_host: "imap.gmx.com",
        smtp_host: "mail.gmx.com",
        smtp_port: 465,
        smtp_security: Security::Tls,
        note: "Turn on IMAP/POP access in your GMX settings first.",
    },
    Provider {
        domains: &["fastmail.com", "fastmail.fm"],
        imap_host: "imap.fastmail.com",
        smtp_host: "smtp.fastmail.com",
        smtp_port: 465,
        smtp_security: Security::Tls,
        note: "Fastmail needs an app password (Settings \u{203a} Privacy & Security \u{203a} App passwords).",
    },
];

/// The part of an email after the @, lower-cased.
pub fn domain_of(email: &str) -> &str {
    email.rsplit('@').next().unwrap_or("").trim()
}

/// Suggested IMAP settings for an address, plus any note. Falls back to imap./smtp. on the domain.
pub fn suggest(email: &str) -> (ManualConfig, String) {
    let domain = domain_of(email).to_ascii_lowercase();
    if let Some(p) = PROVIDERS.iter().find(|p| p.domains.contains(&domain.as_str())) {
        return (
            ManualConfig {
                protocol: MailProtocol::Imap,
                incoming: ServerConfig { host: p.imap_host.into(), port: 993, security: Security::Tls },
                smtp: ServerConfig { host: p.smtp_host.into(), port: p.smtp_port, security: p.smtp_security },
                login: email.to_string(),
            },
            p.note.to_string(),
        );
    }
    (
        ManualConfig {
            protocol: MailProtocol::Imap,
            incoming: ServerConfig { host: format!("imap.{domain}"), port: 993, security: Security::Tls },
            smtp: ServerConfig { host: format!("smtp.{domain}"), port: 587, security: Security::StartTls },
            login: email.to_string(),
        },
        String::new(),
    )
}

/// The standard port for a protocol and security, used when the person flips a control.
pub fn default_port(kind: Kind, security: Security) -> u16 {
    match (kind, security) {
        (Kind::Imap, Security::StartTls) => 143,
        (Kind::Imap, _) => 993,
        (Kind::Pop, Security::StartTls) => 110,
        (Kind::Pop, _) => 995,
        (Kind::Smtp, Security::Tls) => 465,
        (Kind::Smtp, Security::StartTls) => 587,
        (Kind::Smtp, Security::None) => 25,
    }
}

#[derive(Clone, Copy)]
pub enum Kind {
    Imap,
    Pop,
    Smtp,
}
