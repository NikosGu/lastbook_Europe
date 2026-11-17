//! A local HTTPS server for setup's tests, with a certificate made for the run. It serves what `Files` holds and can cut
//! the connection mid-file, send a wrong byte, change a file's ETag, ignore Range, or redirect anywhere. Tests only.

use super::net::{Allow, Tls};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct Files {
    pub data: HashMap<String, Vec<u8>>,
    pub etag: HashMap<String, String>,
    /// name -> byte counts after which the next answers are cut, one per answer
    pub cut_after: HashMap<String, Vec<u64>>,
    /// name -> how many more answers carry a flipped byte
    pub corrupt: HashMap<String, u32>,
    pub ignore_range: HashSet<String>,
    /// name -> how many more Range requests get a 200 with the whole file
    pub ignore_once: HashMap<String, u32>,
    /// name -> Location
    pub redirect: HashMap<String, String>,
    /// "GET /name Range: bytes=N-" for each request
    pub log: Vec<String>,
}

pub struct TestServer {
    pub base: String,
    pub files: Arc<Mutex<Files>>,
    pub tls: Tls,
}

impl TestServer {
    pub fn allow(&self) -> Allow {
        Allow::Only(vec!["127.0.0.1".into()])
    }

    pub fn put(&self, name: &str, data: Vec<u8>) {
        self.files.lock().unwrap().data.insert(name.into(), data);
    }
}

pub fn start() -> TestServer {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let ck = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()]).expect("certificate");
    let cert_der = ck.cert.der().clone();
    let key = rustls::pki_types::PrivateKeyDer::try_from(ck.signing_key.serialize_der()).expect("key");
    let config = Arc::new(
        rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![cert_der], key)
            .expect("server config"),
    );
    let tls = Tls::from_pem(ck.cert.pem().as_bytes()).expect("client roots");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("https://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let files = Arc::new(Mutex::new(Files::default()));
    let f2 = files.clone();
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let config = config.clone();
            let files = f2.clone();
            std::thread::spawn(move || {
                let Ok(server) = rustls::ServerConnection::new(config) else { return };
                let mut stream = rustls::StreamOwned::new(server, conn);
                let _ = serve(&mut stream, &files);
            });
        }
    });
    TestServer { base, files, tls }
}

fn serve(stream: &mut (impl std::io::Read + Write), files: &Mutex<Files>) -> std::io::Result<()> {
    let mut reader = BufReader::new(&mut *stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
    let (mut range, mut if_range) = (None, None);
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 || h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            match k.trim().to_ascii_lowercase().as_str() {
                "range" => range = v.trim().strip_prefix("bytes=").and_then(|r| r.trim_end_matches('-').parse::<u64>().ok()),
                "if-range" => if_range = Some(v.trim().to_string()),
                _ => {}
            }
        }
    }
    drop(reader);
    let name = path.trim_start_matches('/').split('?').next().unwrap_or("").to_string();
    let (status, headers, body, cut) = {
        let mut f = files.lock().unwrap();
        f.log.push(format!("GET /{name}{}", range.map(|r| format!(" Range: bytes={r}-")).unwrap_or_default()));
        if let Some(loc) = f.redirect.get(&name).cloned() {
            (302, vec![("Location".to_string(), loc)], Vec::new(), None)
        } else if let Some(data) = f.data.get(&name).cloned() {
            let etag = f.etag.get(&name).cloned().unwrap_or_else(|| format!("\"{}\"", data.len()));
            let mut whole = range.is_none() || f.ignore_range.contains(&name);
            if !whole {
                if let Some(n) = f.ignore_once.get_mut(&name) {
                    if *n > 0 {
                        *n -= 1;
                        whole = true;
                    }
                }
            }
            if if_range.as_ref().is_some_and(|t| *t != etag) {
                whole = true;
            }
            let start = if whole { 0 } else { range.unwrap() as usize };
            let mut body = data[start.min(data.len())..].to_vec();
            if let Some(n) = f.corrupt.get_mut(&name) {
                if *n > 0 && !body.is_empty() {
                    *n -= 1;
                    let i = body.len() / 2;
                    body[i] ^= 0xff;
                }
            }
            let cut = f.cut_after.get_mut(&name).and_then(|v| (!v.is_empty()).then(|| v.remove(0)));
            let mut headers = vec![("ETag".to_string(), etag), ("Content-Length".to_string(), body.len().to_string())];
            let status = if whole { 200 } else { 206 };
            if !whole {
                headers.push(("Content-Range".to_string(), format!("bytes {start}-{}/{}", data.len().saturating_sub(1), data.len())));
            }
            (status, headers, body, cut)
        } else {
            (404, vec![("Content-Length".to_string(), "0".to_string())], Vec::new(), None)
        }
    };
    let mut head = format!("HTTP/1.1 {status} X\r\nConnection: close\r\n");
    for (k, v) in &headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    let send = cut.map(|c| (c as usize).min(body.len())).unwrap_or(body.len());
    stream.write_all(&body[..send])?;
    stream.flush()
}
