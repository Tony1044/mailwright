//! A plain-TLS client stream over TCP, using the same rustls (ring) stack as the rest of
//! Mailwright, so there are no extra system libraries and it cross-compiles to Windows unchanged.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

use rustls::{ClientConnection, StreamOwned};
use rustls_pki_types::ServerName;

use crate::NetError;

/// A rustls client config that trusts the platform's roots, built on the ring provider (the same
/// one the rest of Mailwright uses).
fn config() -> Result<Arc<rustls::ClientConfig>, NetError> {
    use rustls_platform_verifier::ConfigVerifierExt;
    // Install ring as the process default provider once, so the builder has one to use.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let cfg = rustls::ClientConfig::with_platform_verifier().map_err(|e| NetError::Tls(e.to_string()))?;
    Ok(Arc::new(cfg))
}

/// A connected, TLS-wrapped stream.
pub type Tls = StreamOwned<ClientConnection, TcpStream>;

/// Open a TCP connection to `host:port` and complete a TLS handshake (implicit TLS).
pub fn connect(host: &str, port: u16, timeout: Duration) -> Result<Tls, NetError> {
    let tcp = dial(host, port, timeout)?;
    wrap(tcp, host)
}

/// Open a plain TCP connection (for STARTTLS, which upgrades later).
pub fn dial(host: &str, port: u16, timeout: Duration) -> Result<TcpStream, NetError> {
    let mut last = None;
    let addrs = (host, port).to_socket_addrs_net()?;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(s) => {
                let _ = s.set_read_timeout(Some(Duration::from_secs(60)));
                let _ = s.set_write_timeout(Some(Duration::from_secs(60)));
                let _ = s.set_nodelay(true);
                return Ok(s);
            }
            Err(e) => last = Some(e),
        }
    }
    Err(NetError::Connect(format!("{host}:{port}: {}", last.map(|e| e.to_string()).unwrap_or_else(|| "no address".into()))))
}

/// Wrap an existing TCP stream in TLS (for STARTTLS, after the upgrade command).
pub fn wrap(tcp: TcpStream, host: &str) -> Result<Tls, NetError> {
    let cfg = config()?;
    let server = ServerName::try_from(host.to_string()).map_err(|_| NetError::Connect(format!("invalid host name {host}")))?;
    let conn = ClientConnection::new(cfg, server).map_err(|e| NetError::Tls(e.to_string()))?;
    Ok(StreamOwned::new(conn, tcp))
}

/// A connection that may be plain TCP or TLS, so STARTTLS can upgrade it in place.
pub enum Stream {
    Plain(TcpStream),
    Tls(Box<Tls>),
}

impl Stream {
    /// Connect with the given security: implicit TLS, or plain (for STARTTLS or no encryption).
    pub fn connect(host: &str, port: u16, implicit_tls: bool, timeout: Duration) -> Result<Stream, NetError> {
        if implicit_tls {
            Ok(Stream::Tls(Box::new(connect(host, port, timeout)?)))
        } else {
            Ok(Stream::Plain(dial(host, port, timeout)?))
        }
    }

    /// Upgrade a plain connection to TLS (after a STARTTLS command).
    pub fn start_tls(self, host: &str) -> Result<Stream, NetError> {
        match self {
            Stream::Plain(tcp) => Ok(Stream::Tls(Box::new(wrap(tcp, host)?))),
            tls => Ok(tls),
        }
    }

    pub fn is_tls(&self) -> bool {
        matches!(self, Stream::Tls(_))
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(s) => s.read(buf),
            Stream::Tls(s) => s.read(buf),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(s) => s.write(buf),
            Stream::Tls(s) => s.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Stream::Plain(s) => s.flush(),
            Stream::Tls(s) => s.flush(),
        }
    }
}

/// A line-oriented helper over a stream (for POP3 and SMTP).
pub struct Line<S: Read + Write> {
    pub inner: S,
    buf: Vec<u8>,
}

impl<S: Read + Write> Line<S> {
    pub fn new(inner: S) -> Line<S> {
        Line { inner, buf: Vec::with_capacity(1024) }
    }

    /// Read one CRLF-terminated line (without the CRLF).
    pub fn read_line(&mut self) -> Result<String, NetError> {
        loop {
            if let Some(pos) = self.buf.windows(2).position(|w| w == b"\r\n") {
                let line: Vec<u8> = self.buf.drain(..pos + 2).collect();
                return Ok(String::from_utf8_lossy(&line[..line.len() - 2]).into_owned());
            }
            let mut chunk = [0u8; 4096];
            let n = self.inner.read(&mut chunk).map_err(|e| NetError::Io(e.to_string()))?;
            if n == 0 {
                return Err(NetError::Io("the server closed the connection".into()));
            }
            self.buf.extend_from_slice(&chunk[..n]);
        }
    }

    /// Take back the underlying stream (for a STARTTLS upgrade). Any buffered bytes are dropped,
    /// which is safe straight after a STARTTLS reply: nothing follows it before the handshake.
    pub fn into_inner(self) -> S {
        self.inner
    }

    pub fn write_line(&mut self, s: &str) -> Result<(), NetError> {
        self.inner.write_all(s.as_bytes()).map_err(|e| NetError::Io(e.to_string()))?;
        self.inner.write_all(b"\r\n").map_err(|e| NetError::Io(e.to_string()))?;
        self.inner.flush().map_err(|e| NetError::Io(e.to_string()))
    }

    /// Read `n` bytes exactly (POP3 multi-line bodies are read by byte count where known).
    pub fn take_buffered(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.buf)
    }
}

/// Resolve host:port to socket addresses, mapping errors to NetError.
trait ToSocketAddrsNet {
    fn to_socket_addrs_net(&self) -> Result<std::vec::IntoIter<std::net::SocketAddr>, NetError>;
}

impl ToSocketAddrsNet for (&str, u16) {
    fn to_socket_addrs_net(&self) -> Result<std::vec::IntoIter<std::net::SocketAddr>, NetError> {
        use std::net::ToSocketAddrs;
        self.to_socket_addrs().map(|i| i.collect::<Vec<_>>().into_iter()).map_err(|e| NetError::Connect(format!("can't find {}: {e}", self.0)))
    }
}
