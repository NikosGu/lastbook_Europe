//! The one way out: HTTPS GETs for setup's downloads and the update check. Nothing else in the app opens a connection.
//!
//!   - HTTPS only, to the hosts engine::dist names (the project's releases on GitHub, where GitHub sends their files,
//!     and Hugging Face's hosts for the AI model), port 443; on github.com, this repository's release files only. Tests
//!     pass their own hosts, on any port.
//!   - No proxy: nothing is read from the system's or the environment's proxy settings. A request goes straight to the
//!     host named, or nowhere.
//!   - Trust: the roots in roots.pem only, never the system's store.
//!   - A redirect is followed only to another allowed address, at most MAX_REDIRECTS times.
//!   - A connection that sends nothing for `timeout` counts as broken.

use crate::dist;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use url::Url;

pub const MAX_REDIRECTS: usize = 5;
pub const USER_AGENT: &str = "Lastbook-setup/1";
const MAX_HEAD: usize = 64 * 1024;

/// Where requests may go.
#[derive(Clone, Debug)]
pub enum Allow {
    /// The product: engine::dist's hosts, port 443 only.
    Product,
    /// A test's own hosts (such as 127.0.0.1), on any port.
    Only(Vec<String>),
}

pub fn allowed_hosts() -> Vec<&'static str> {
    let mut v = vec![dist::DOWNLOAD_HOST];
    v.extend_from_slice(dist::DOWNLOAD_REDIRECT_HOSTS);
    v.push(dist::MODEL_HOST);
    v.extend_from_slice(dist::MODEL_REDIRECT_HOSTS);
    v
}

pub fn host_allowed(host: &str, allow: &Allow) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    match allow {
        Allow::Only(hosts) => hosts.iter().any(|h| *h == host),
        Allow::Product => allowed_hosts().contains(&host.as_str()) || dist::MODEL_SUFFIXES.iter().any(|s| host.ends_with(s) && host.len() > s.len()),
    }
}

/// An https address on an allowed host, with no user name or password, on port 443 (a test's own hosts: any port).
pub fn url_allowed(url: &str, allow: &Allow) -> bool {
    let Ok(u) = Url::parse(url) else { return false };
    if u.scheme() != "https" || !u.username().is_empty() || u.password().is_some() {
        return false;
    }
    let Some(host) = u.host_str() else { return false };
    if matches!(allow, Allow::Product) && u.port().is_some_and(|p| p != 443) {
        return false;
    }
    // on the download host (github.com), this repository's release files only, not anyone's
    if matches!(allow, Allow::Product) && host.eq_ignore_ascii_case(dist::DOWNLOAD_HOST) && !u.path().starts_with(&dist::download_path_prefix()) {
        return false;
    }
    host_allowed(host.trim_start_matches('[').trim_end_matches(']'), allow)
}

/// TLS 1.2 or later, the host name checked, and trust in the given roots only.
#[derive(Clone)]
pub struct Tls(Arc<rustls::ClientConfig>);

impl Tls {
    pub fn from_pem(pem: &[u8]) -> Result<Tls, String> {
        let mut roots = rustls::RootCertStore::empty();
        for cert in CertificateDer::pem_slice_iter(pem) {
            let cert = cert.map_err(|e| format!("roots: {e}"))?;
            roots.add(cert).map_err(|e| format!("roots: {e}"))?;
        }
        if roots.is_empty() {
            return Err("roots: no certificate".into());
        }
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .with_root_certificates(roots)
            .with_no_client_auth();
        Ok(Tls(Arc::new(config)))
    }

    /// The roots the program was built with (roots.pem beside this file, built into the executable): the only ones trusted.
    pub fn product() -> Result<Tls, String> {
        Tls::from_pem(ROOTS.as_bytes())
    }
}

/// The pinned roots: a few certificate authorities from Mozilla's list, never the system's store.
pub const ROOTS: &str = include_str!("roots.pem");

#[derive(Debug)]
pub enum NetError {
    /// The address is not one setup may reach.
    Refused(String),
    /// The server's certificate did not verify through roots.pem.
    Certificate(String),
    /// The connection broke, timed out, or the answer could not be read.
    Io(io::Error),
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetError::Refused(m) | NetError::Certificate(m) => write!(f, "{m}"),
            NetError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl From<io::Error> for NetError {
    fn from(e: io::Error) -> NetError {
        if let Some(inner) = e.get_ref().and_then(|i| i.downcast_ref::<rustls::Error>()) {
            if matches!(inner, rustls::Error::InvalidCertificate(_)) {
                return NetError::Certificate(inner.to_string());
            }
        }
        NetError::Io(e)
    }
}

/// An answer: its status and headers, and its body to read.
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    /// The address that answered, after redirects.
    pub url: String,
    body: Box<dyn Read + Send>,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn body(&mut self) -> &mut (dyn Read + Send) {
        &mut *self.body
    }

    /// Reads up to `limit` bytes of the body; None if there is more.
    pub fn read_limited(&mut self, limit: usize) -> io::Result<Option<Vec<u8>>> {
        let mut out = Vec::new();
        (&mut self.body).take(limit as u64 + 1).read_to_end(&mut out)?;
        Ok((out.len() <= limit).then_some(out))
    }
}

/// A GET from byte `start` (a Range, with If-Range when the ETag is known), after following allowed redirects.
pub fn get(url: &str, start: u64, etag: Option<&str>, allow: &Allow, tls: &Tls, timeout: Duration) -> Result<HttpResponse, NetError> {
    let mut url = url.to_string();
    for _ in 0..=MAX_REDIRECTS {
        if !url_allowed(&url, allow) {
            return Err(NetError::Refused("Lastbook Europe refused to download from an address it does not know.".into()));
        }
        let mut headers = vec![("User-Agent", USER_AGENT.to_string()), ("Accept-Encoding", "identity".to_string()), ("Connection", "close".to_string())];
        if start > 0 {
            headers.push(("Range", format!("bytes={start}-")));
            if let Some(etag) = etag {
                headers.push(("If-Range", etag.to_string()));
            }
        }
        let resp = request(&url, &headers, tls, timeout)?;
        if matches!(resp.status, 301 | 302 | 303 | 307 | 308) {
            let Some(loc) = resp.header("Location").map(str::to_string) else {
                return Err(NetError::Io(io::Error::new(io::ErrorKind::InvalidData, "The download server answered with a move to nowhere.")));
            };
            url = Url::parse(&url).and_then(|base| base.join(&loc)).map(|u| u.to_string()).map_err(|_| NetError::Refused("The download server moved the file to an address Lastbook Europe cannot read.".into()))?;
            continue;
        }
        return Ok(resp);
    }
    Err(NetError::Io(io::Error::new(io::ErrorKind::InvalidData, "The download server sent Lastbook Europe round in circles.")))
}

/// One small GET: the bytes, or None on any failure, a status other than 200, or more than `limit` bytes.
pub fn fetch_small(url: &str, limit: usize, allow: &Allow, tls: &Tls, timeout: Duration) -> Option<Vec<u8>> {
    let mut r = get(url, 0, None, allow, tls, timeout).ok()?;
    if r.status != 200 {
        return None;
    }
    r.read_limited(limit).ok().flatten()
}

fn request(url: &str, headers: &[(&str, String)], tls: &Tls, timeout: Duration) -> Result<HttpResponse, NetError> {
    let u = Url::parse(url).map_err(|_| NetError::Refused("not an address".into()))?;
    let host = u.host_str().ok_or_else(|| NetError::Refused("no host".into()))?.trim_start_matches('[').trim_end_matches(']').to_string();
    let port = u.port().unwrap_or(443);
    let mut last = io::Error::new(io::ErrorKind::NotFound, format!("{host} has no address"));
    let mut tcp = None;
    for addr in (host.as_str(), port).to_socket_addrs()? {
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(s) => {
                tcp = Some(s);
                break;
            }
            Err(e) => last = e,
        }
    }
    let tcp = tcp.ok_or(last)?;
    tcp.set_read_timeout(Some(timeout))?;
    tcp.set_write_timeout(Some(timeout))?;
    let name = ServerName::try_from(host.clone()).map_err(|_| NetError::Refused(format!("{host} is not a host name")))?;
    let conn = rustls::ClientConnection::new(tls.0.clone(), name).map_err(|e| NetError::Io(io::Error::other(e)))?;
    let mut stream = rustls::StreamOwned::new(conn, tcp);
    let mut path = u.path().to_string();
    if path.is_empty() {
        path.push('/');
    }
    if let Some(q) = u.query() {
        path.push('?');
        path.push_str(q);
    }
    let host_header = if u.port().is_some() { format!("{host}:{port}") } else { host.clone() };
    let mut head = format!("GET {path} HTTP/1.1\r\nHost: {host_header}\r\n");
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.flush()?;
    let mut reader = BufReader::with_capacity(256 * 1024, stream);
    let (status, headers) = read_head(&mut reader)?;
    let chunked = headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("Transfer-Encoding") && v.to_ascii_lowercase().contains("chunked"));
    let length = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("Content-Length")).and_then(|(_, v)| v.trim().parse::<u64>().ok());
    let body: Box<dyn Read + Send> = if chunked {
        Box::new(Chunked { inner: reader, left: 0, done: false })
    } else if let Some(n) = length {
        Box::new(reader.take(n))
    } else {
        Box::new(reader)
    };
    Ok(HttpResponse { status, headers, url: url.to_string(), body })
}

fn read_head(r: &mut impl BufRead) -> Result<(u16, Vec<(String, String)>), NetError> {
    let bad = |m: &str| NetError::Io(io::Error::new(io::ErrorKind::InvalidData, m.to_string()));
    let mut line = String::new();
    let mut total = 0;
    let mut read_line = |line: &mut String| -> Result<(), NetError> {
        line.clear();
        let n = r.by_ref().take(MAX_HEAD as u64).read_line(line)?;
        total += n;
        if n == 0 {
            return Err(NetError::Io(io::Error::new(io::ErrorKind::UnexpectedEof, "the server closed the connection")));
        }
        if total > MAX_HEAD {
            return Err(NetError::Io(io::Error::new(io::ErrorKind::InvalidData, "the answer's head is too long")));
        }
        Ok(())
    };
    read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let version = parts.next().unwrap_or("");
    if !version.starts_with("HTTP/1.") {
        return Err(bad("not an HTTP/1 answer"));
    }
    let status: u16 = parts.next().and_then(|s| s.parse().ok()).ok_or_else(|| bad("no status"))?;
    let mut headers = Vec::new();
    loop {
        read_line(&mut line)?;
        let l = line.trim_end_matches(['\r', '\n']);
        if l.is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    Ok((status, headers))
}

/// Transfer-Encoding: chunked.
struct Chunked<R> {
    inner: R,
    left: u64,
    done: bool,
}

impl<R: BufRead> Read for Chunked<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.done || buf.is_empty() {
            return Ok(0);
        }
        if self.left == 0 {
            let mut line = String::new();
            (&mut self.inner).take(1024).read_line(&mut line)?;
            let size = line.trim().split(';').next().unwrap_or("");
            self.left = u64::from_str_radix(size, 16).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad chunk size"))?;
            if self.left == 0 {
                self.done = true;
                loop {      // trailers, then the blank line
                    line.clear();
                    let n = (&mut self.inner).take(1024).read_line(&mut line)?;
                    if n == 0 || line.trim().is_empty() {
                        break;
                    }
                }
                return Ok(0);
            }
        }
        let want = buf.len().min(self.left as usize);
        let n = self.inner.read(&mut buf[..want])?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the connection closed early"));
        }
        self.left -= n as u64;
        if self.left == 0 {
            let mut crlf = [0u8; 2];
            self.inner.read_exact(&mut crlf)?;
        }
        Ok(n)
    }
}

/// The certificate roots file, read once per use: kept beside the program, never in the data folder.
pub fn tls_for_tests(pem_path: &Path) -> Result<Tls, String> {
    Tls::from_pem(&std::fs::read(pem_path).map_err(|e| e.to_string())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_hosts() {
        let h = dist::DOWNLOAD_HOST;
        let ok = [format!("{}/Lastbook-Small.zip", dist::download_base()), dist::update_url().unwrap(),
                  "https://release-assets.githubusercontent.com/github-production-release-asset/1/abc?sp=r&sig=x".into(),
                  "https://huggingface.co/unsloth/x/resolve/abc/f.gguf?download=true".into(),
                  "https://cas-bridge.xethub.hf.co/xet-bridge-us/abc?X-Amz=1".into(), "https://us.gcp.cdn.hf.co/x".into()];
        let bad = [format!("http://{h}/x"), format!("https://{h}:8443/x"), "https://evil.com/x".into(), "https://hf.co.evil.com/x".into(),
                   format!("https://user:pw@{h}/x"), "https://downloads.lastbook.org/x".into(), "https://lastbook.org/x".into(),
                   "https://other.r2.dev/x".into(), "ftp://huggingface.co/x".into(), "https://.hf.co/x".into(), "https://localhost/x".into(),
                   "https://127.0.0.1/x".into(), "file:///etc/passwd".into(), format!("https://{h}.evil.com/x"),
                   // github.com: only this repository's releases
                   format!("https://{h}/someone/else/releases/download/v1/x.zip"), format!("https://{h}/NikosGu/lastbook_Europe/archive/main.zip"),
                   format!("https://{h}/x")];
        for u in &ok {
            assert!(url_allowed(u, &Allow::Product), "{u}");
        }
        for u in &bad {
            assert!(!url_allowed(u, &Allow::Product), "{u}");
        }
        assert!(url_allowed("https://127.0.0.1:8443/x", &Allow::Only(vec!["127.0.0.1".into()])));
    }

    #[test]
    fn product_roots_load() {
        assert!(Tls::product().is_ok());
        assert!(Tls::from_pem(b"not a certificate").is_err());
    }

    #[test]
    fn chunked_bodies() {
        let raw = b"4\r\nWiki\r\n5;x=1\r\npedia\r\n0\r\nTrailer: y\r\n\r\n";
        let mut c = Chunked { inner: BufReader::new(&raw[..]), left: 0, done: false };
        let mut out = String::new();
        c.read_to_string(&mut out).unwrap();
        assert_eq!(out, "Wikipedia");
    }
}
