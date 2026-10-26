//! Signing in to Microsoft Entra ID as a public client (no secret).
//!
//! The usual way is the authorization code flow with PKCE: the system browser opens Microsoft's
//! sign-in page (so MFA, conditional access and single sign-on all work as they do for Outlook),
//! and the browser is sent back to `http://localhost:<port>` where Mailwright listens for the code.
//! Where a browser can't reach the loopback address, the device code flow shows a code to enter at
//! microsoft.com/devicelogin instead. Either way the result is an access token for Graph and a
//! refresh token that keeps the session going.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::Digest as _;

use crate::config::{AccountConfig, SCOPES};
use crate::error::GraphError;
use crate::http;

/// Tokens from the token endpoint.
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds when the access token expires.
    pub expires_at: i64,
}

impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print tokens.
        f.debug_struct("Tokens").field("expires_at", &self.expires_at).finish_non_exhaustive()
    }
}

impl Tokens {
    /// Still good for at least two more minutes.
    pub fn is_fresh(&self) -> bool {
        !self.access_token.is_empty() && now() + 120 < self.expires_at
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct DeviceCode {
    pub user_code: String,
    pub device_code: String,
    pub verification_uri: String,
    pub expires_in: i64,
    pub interval: i64,
    pub message: String,
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn scope_string() -> String {
    SCOPES.join(" ")
}

fn random_url_safe(bytes: usize) -> Result<String, GraphError> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| GraphError::Config(format!("no random numbers: {e}")))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf))
}

/// PKCE verifier and its S256 challenge (RFC 7636).
pub fn pkce_pair() -> Result<(String, String), GraphError> {
    let verifier = random_url_safe(48)?;
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(verifier.as_bytes()));
    Ok((verifier, challenge))
}

pub fn authorize_url(cfg: &AccountConfig, redirect_uri: &str, state: &str, challenge: &str) -> String {
    let mut u = match url::Url::parse(&format!("{}/oauth2/v2.0/authorize", cfg.authority())) {
        Ok(u) => u,
        Err(_) => return String::new(),
    };
    u.query_pairs_mut()
        .append_pair("client_id", cfg.client_id.trim())
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("response_mode", "query")
        .append_pair("scope", &scope_string())
        .append_pair("state", state)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        ;
    if cfg.username.is_empty() {
        u.query_pairs_mut().append_pair("prompt", "select_account");
    } else {
        u.query_pairs_mut().append_pair("login_hint", &cfg.username);
        if let Some(domain) = cfg.username.rsplit_once('@').map(|(_, d)| d.to_ascii_lowercase()) {
            if !crate::discovery::is_consumer_domain(&domain) {
                u.query_pairs_mut().append_pair("domain_hint", &domain);
            }
        }
    }
    u.to_string()
}

fn token_endpoint(cfg: &AccountConfig) -> String {
    format!("{}/oauth2/v2.0/token", cfg.authority())
}

fn post_token(cfg: &AccountConfig, form: &[(&str, &str)]) -> Result<Tokens, GraphError> {
    let url = token_endpoint(cfg);
    let mut resp = http::agent().post(&url).send_form(form.iter().copied()).map_err(|e| http::network_error(&url, e))?;
    let text = http::read_text(&mut resp, &url)?;
    let tr: TokenResponse = serde_json::from_str(&text).map_err(|e| GraphError::BadResponse(format!("token response: {e}")))?;
    if let Some(err) = tr.error {
        let desc = tr.error_description.unwrap_or_default();
        let first_line = desc.lines().next().unwrap_or_default().to_string();
        return Err(match err.as_str() {
            "invalid_grant" | "interaction_required" | "consent_required" | "login_required" => GraphError::SignInRequired(first_line),
            "authorization_pending" | "slow_down" => GraphError::Api { status: 400, code: err, message: first_line },
            "authorization_declined" | "expired_token" | "access_denied" => GraphError::SignInCancelled(first_line),
            _ => GraphError::Api { status: resp.status().as_u16(), code: err, message: first_line },
        });
    }
    if tr.access_token.is_empty() {
        return Err(GraphError::BadResponse("the token response has no access token".into()));
    }
    Ok(Tokens {
        access_token: tr.access_token,
        refresh_token: tr.refresh_token.unwrap_or_default(),
        expires_at: now() + tr.expires_in.unwrap_or(3600).clamp(60, 86_400),
    })
}

/// Swap a refresh token for new tokens. Entra rotates refresh tokens: keep the new one.
pub fn refresh(cfg: &AccountConfig, refresh_token: &str) -> Result<Tokens, GraphError> {
    if refresh_token.is_empty() {
        return Err(GraphError::SignInRequired("no saved sign-in".into()));
    }
    let scopes = scope_string();
    let mut t = post_token(cfg, &[
        ("client_id", cfg.client_id.trim()),
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("scope", &scopes),
    ])?;
    if t.refresh_token.is_empty() {
        t.refresh_token = refresh_token.to_string();
    }
    Ok(t)
}

/// The authorization code flow with PKCE, through the system browser and a loopback redirect.
/// `open_browser` is given the sign-in URL. Returns when the browser comes back, the user cancels
/// (`cancel` set) or after ten minutes.
pub fn sign_in_with_browser(cfg: &AccountConfig, open_browser: &dyn Fn(&str) -> bool, cancel: Arc<AtomicBool>) -> Result<Tokens, GraphError> {
    cfg.validate().map_err(GraphError::Config)?;
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| GraphError::Config(format!("can't listen on localhost: {e}")))?;
    let port = listener.local_addr().map_err(|e| GraphError::Config(e.to_string()))?.port();
    listener.set_nonblocking(true).map_err(|e| GraphError::Config(e.to_string()))?;
    let redirect_uri = format!("http://localhost:{port}");
    let state = random_url_safe(16)?;
    let (verifier, challenge) = pkce_pair()?;
    let url = authorize_url(cfg, &redirect_uri, &state, &challenge);
    if !open_browser(&url) {
        return Err(GraphError::Config("couldn't open a web browser for sign-in".into()));
    }

    let deadline = Instant::now() + Duration::from_secs(600);
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(GraphError::SignInCancelled("cancelled".into()));
        }
        if Instant::now() > deadline {
            return Err(GraphError::SignInCancelled("sign-in timed out".into()));
        }
        match listener.accept() {
            Ok((stream, _)) => {
                if let Some(result) = handle_redirect(stream, &state) {
                    let code = result?;
                    return post_token(cfg, &[
                        ("client_id", cfg.client_id.trim()),
                        ("grant_type", "authorization_code"),
                        ("code", &code),
                        ("redirect_uri", &redirect_uri),
                        ("code_verifier", &verifier),
                        ("scope", &scope_string()),
                    ]);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(GraphError::Config(format!("loopback listener: {e}"))),
        }
    }
}

/// One request to the loopback address. `None` for requests that aren't the redirect (a
/// browser asking for /favicon.ico); otherwise the code or the error the sign-in page sent.
fn handle_redirect(stream: TcpStream, state: &str) -> Option<Result<String, GraphError>> {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return None,
    });
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() || line.len() > 16 * 1024 {
        return None;
    }
    let target = line.split_whitespace().nth(1).unwrap_or("");
    let parsed = url::Url::parse(&format!("http://localhost{target}")).ok()?;
    let mut code = None;
    let mut got_state = None;
    let mut error = None;
    let mut error_description = None;
    for (k, v) in parsed.query_pairs() {
        match k.as_ref() {
            "code" => code = Some(v.to_string()),
            "state" => got_state = Some(v.to_string()),
            "error" => error = Some(v.to_string()),
            "error_description" => error_description = Some(v.to_string()),
            _ => {}
        }
    }
    if code.is_none() && error.is_none() {
        respond(stream, 404, "Not found");
        return None;
    }
    let result = if let Some(err) = error {
        let desc = error_description.unwrap_or_default();
        respond(stream, 200, &page("Sign-in didn't finish", &format!("{err}: {}", desc.lines().next().unwrap_or(""))));
        Err(if err == "access_denied" { GraphError::SignInCancelled(desc) } else { GraphError::SignInRequired(format!("{err}: {desc}")) })
    } else if got_state.as_deref() != Some(state) {
        respond(stream, 400, &page("Sign-in didn't finish", "The response didn't match this sign-in. Close this tab and try again."));
        Err(GraphError::SignInRequired("state mismatch".into()))
    } else {
        respond(stream, 200, &page("You're signed in to Mailwright", "You can close this tab and go back to Mailwright."));
        Ok(code.unwrap_or_default())
    };
    Some(result)
}

fn page(title: &str, text: &str) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    format!(
        "<!doctype html><meta charset=utf-8><title>Mailwright</title><body style=\"font-family:Segoe UI,Arial,sans-serif;margin:60px\"><h2>{}</h2><p>{}</p></body>",
        esc(title),
        esc(text)
    )
}

fn respond(mut stream: TcpStream, status: u16, body: &str) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        _ => "Not Found",
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
}

/// Start the device code flow: the code to show and where to enter it.
pub fn start_device_code(cfg: &AccountConfig) -> Result<DeviceCode, GraphError> {
    cfg.validate().map_err(GraphError::Config)?;
    let url = format!("{}/oauth2/v2.0/devicecode", cfg.authority());
    let scopes = scope_string();
    let mut resp = http::agent()
        .post(&url)
        .send_form([("client_id", cfg.client_id.trim()), ("scope", scopes.as_str())])
        .map_err(|e| http::network_error(&url, e))?;
    let status = resp.status().as_u16();
    let text = http::read_text(&mut resp, &url)?;
    if status >= 400 {
        let tr: TokenResponse = serde_json::from_str(&text).unwrap_or_default();
        return Err(GraphError::Api {
            status,
            code: tr.error.unwrap_or_default(),
            message: tr.error_description.unwrap_or_default().lines().next().unwrap_or_default().to_string(),
        });
    }
    serde_json::from_str(&text).map_err(|e| GraphError::BadResponse(format!("device code: {e}")))
}

/// Wait for the user to finish the device code sign-in.
pub fn finish_device_code(cfg: &AccountConfig, dc: &DeviceCode, cancel: Arc<AtomicBool>) -> Result<Tokens, GraphError> {
    let mut interval = dc.interval.clamp(1, 30) as u64;
    let deadline = Instant::now() + Duration::from_secs(dc.expires_in.clamp(60, 1800) as u64);
    loop {
        for _ in 0..interval * 10 {
            if cancel.load(Ordering::Relaxed) {
                return Err(GraphError::SignInCancelled("cancelled".into()));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if Instant::now() > deadline {
            return Err(GraphError::SignInCancelled("the code expired".into()));
        }
        match post_token(cfg, &[
            ("client_id", cfg.client_id.trim()),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("device_code", &dc.device_code),
        ]) {
            Ok(t) => return Ok(t),
            Err(GraphError::Api { code, .. }) if code == "authorization_pending" => {}
            Err(GraphError::Api { code, .. }) if code == "slow_down" => interval = (interval + 5).min(30),
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_is_s256_of_the_verifier() {
        let (v, c) = pkce_pair().unwrap();
        assert!(v.len() >= 43 && v.len() <= 128);
        let expect = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(v.as_bytes()));
        assert_eq!(c, expect);
    }

    #[test]
    fn authorize_url_carries_the_flow_parameters() {
        let cfg = AccountConfig { client_id: "1b2c3d4e-0000-1111-2222-333344445555".into(), tenant: "contoso.com".into(), ..Default::default() };
        let u = authorize_url(&cfg, "http://localhost:5000", "st", "ch");
        let parsed = url::Url::parse(&u).unwrap();
        assert!(parsed.path().ends_with("/contoso.com/oauth2/v2.0/authorize"));
        let q: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["redirect_uri"], "http://localhost:5000");
        assert!(q["scope"].contains("Mail.ReadWrite") && q["scope"].contains("offline_access"));
    }

    #[test]
    fn tokens_never_print() {
        let t = Tokens { access_token: "secret-access".into(), refresh_token: "secret-refresh".into(), expires_at: 1 };
        let s = format!("{t:?}");
        assert!(!s.contains("secret"));
    }
}
