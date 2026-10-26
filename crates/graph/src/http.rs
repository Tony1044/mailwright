//! One shared HTTP agent: rustls with the operating system's certificate store (so a company's
//! TLS-inspecting proxy root, trusted by Windows, is trusted here too), the proxy from
//! `HTTPS_PROXY`, gzip, and our own status handling (errors are read, not thrown).

use std::sync::OnceLock;
use std::time::Duration;

use ureq::Agent;
use ureq::tls::{RootCerts, TlsConfig};

use crate::error::GraphError;

/// Largest JSON response read into memory.
pub const MAX_JSON: u64 = 64 * 1024 * 1024;
/// Largest attachment downloaded (Exchange Online's own limit is 150 MB).
pub const MAX_DOWNLOAD: u64 = 160 * 1024 * 1024;

pub fn agent() -> &'static Agent {
    static AGENT: OnceLock<Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        let tls = TlsConfig::builder().root_certs(RootCerts::PlatformVerifier).build();
        Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(20)))
            .timeout_send_request(Some(Duration::from_secs(30)))
            // Waiting for the server to start answering, then for the rest of the answer. A
            // connection that dies part-way (sleep, Wi-Fi, VPN) must never wait for ever, or
            // syncing stops for good; attachment transfers get longer limits of their own.
            .timeout_recv_response(Some(Duration::from_secs(90)))
            .timeout_recv_body(Some(Duration::from_secs(120)))
            .timeout_send_body(Some(Duration::from_secs(120)))
            .timeout_global(Some(Duration::from_secs(300)))
            .user_agent(concat!("Mailwright/", env!("CARGO_PKG_VERSION")))
            .tls_config(tls)
            .build()
            .into()
    })
}

pub fn host_of(url: &str) -> String {
    url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_else(|| url.to_string())
}

pub fn network_error(url: &str, e: ureq::Error) -> GraphError {
    GraphError::Network { host: host_of(url), detail: e.to_string() }
}

/// Read a response body as text, bounded.
pub fn read_text(resp: &mut ureq::http::Response<ureq::Body>, url: &str) -> Result<String, GraphError> {
    resp.body_mut().with_config().limit(MAX_JSON).read_to_string().map_err(|e| network_error(url, e))
}

/// Read a response body as bytes, bounded.
pub fn read_bytes(resp: &mut ureq::http::Response<ureq::Body>, url: &str, limit: u64) -> Result<Vec<u8>, GraphError> {
    resp.body_mut().with_config().limit(limit).read_to_vec().map_err(|e| network_error(url, e))
}

/// Retry-After in seconds, capped, when the server sent one.
pub fn retry_after(resp: &ureq::http::Response<ureq::Body>) -> Option<u64> {
    resp.headers().get("Retry-After").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse::<u64>().ok()).map(|s| s.min(120))
}

/// Limits for a big transfer (an attachment): an hour all told.
pub const LONG: Duration = Duration::from_secs(3600);
