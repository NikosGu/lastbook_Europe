//! One file downloaded through net (the one way out), resumable, checked as it arrives.
//!
//!   - `<dest>.part` grows, with `<dest>.part.json` beside it (url, size, sha256, bytes done, ETag).
//!   - Resuming uses HTTP Range on the saved ETag. Some servers ignore If-Range, so the check is made here: another ETag on
//!     the 206, a 200 instead, or another total size starts that file over (once a 200 is asked about again first).
//!   - The SHA-256 is computed while the bytes arrive; after a resume the bytes already on disk are read once to rebuild it.
//!   - A broken connection is tried again with growing waits; NETWORK_TRIES breaks in a row with no progress give up.

use super::net::{self, Allow, NetError, Tls};
use super::SetupError;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

pub const TIMEOUT: Duration = Duration::from_secs(30);
const CHUNK: usize = 1024 * 1024;
const SAVE_EVERY: Duration = Duration::from_secs(2);
pub const NETWORK_TRIES: u32 = 6;
pub const BACKOFF: [f64; 6] = [2.0, 4.0, 8.0, 15.0, 30.0, 30.0];

/// Pause and cancel, from any thread. A download notices within one chunk or one second of waiting.
#[derive(Default)]
pub struct Control {
    why: Mutex<Option<String>>,
    cv: Condvar,
}

impl Control {
    pub fn new() -> Arc<Control> {
        Arc::new(Control::default())
    }

    pub fn stop(&self, why: &str) {
        let mut w = self.why.lock().unwrap();
        if w.as_deref() != Some("cancelled") {
            *w = Some(why.to_string());
        }
        self.cv.notify_all();
    }

    pub fn stopped(&self) -> Option<String> {
        self.why.lock().unwrap().clone()
    }

    pub fn check(&self) -> Result<(), SetupError> {
        match self.stopped() {
            Some(why) => Err(SetupError::Stopped(why)),
            None => Ok(()),
        }
    }

    pub fn sleep(&self, secs: f64) -> Result<(), SetupError> {
        let w = self.why.lock().unwrap();
        let (w, _) = self.cv.wait_timeout_while(w, Duration::from_secs_f64(secs), |w| w.is_none()).unwrap();
        match w.clone() {
            Some(why) => Err(SetupError::Stopped(why)),
            None => Ok(()),
        }
    }
}

pub fn disk_full(e: &io::Error) -> bool {
    match e.raw_os_error() {
        #[cfg(unix)]
        Some(code) => code == libc::ENOSPC || code == libc::EDQUOT,
        #[cfg(windows)]
        Some(code) => code == 112 || code == 39,
        _ => false,
    }
}

/// Bytes, done and total, and the phase: `downloading` or `verifying`.
pub type Progress = Box<dyn FnMut(u64, u64, &str) + Send>;

pub struct Download {
    pub url: String,
    pub dest: PathBuf,
    pub size: u64,
    pub sha256: String,
    control: Arc<Control>,
    progress: Progress,
    allow: Allow,
    tls: Option<Tls>,
    pub timeout: Duration,
    pub backoff: Vec<f64>,
}

enum Fetch {
    Restart { whole: bool },
    Net(NetError),
    Io(io::Error),
    Err(SetupError),
}

impl From<io::Error> for Fetch {
    fn from(e: io::Error) -> Fetch {
        Fetch::Io(e)
    }
}

impl Download {
    pub fn new(url: &str, dest: &Path, size: u64, sha256: &str, control: Arc<Control>, progress: Progress, allow: Allow, tls: Option<Tls>) -> Download {
        Download { url: url.into(), dest: dest.into(), size, sha256: sha256.to_ascii_lowercase(), control, progress, allow, tls, timeout: TIMEOUT, backoff: BACKOFF.to_vec() }
    }

    pub fn part(&self) -> PathBuf {
        with_suffix(&self.dest, ".part")
    }

    fn meta(&self) -> PathBuf {
        with_suffix(&self.dest, ".part.json")
    }

    pub fn done_bytes(&self) -> u64 {
        std::fs::metadata(self.part()).map(|m| m.len().min(self.size)).unwrap_or(0)
    }

    pub fn discard(&self) {
        for p in [self.part(), self.meta(), with_suffix(&self.dest, ".part.json.tmp")] {
            let _ = std::fs::remove_file(p);
        }
    }

    fn rehash(&mut self, n: u64) -> Result<(Sha256, u64), SetupError> {
        let mut h = Sha256::new();
        let mut done = 0u64;
        let mut f = File::open(self.part()).map_err(io_failed)?;
        let mut buf = vec![0u8; 8 * CHUNK];
        while done < n {
            self.control.check()?;
            let want = buf.len().min((n - done) as usize);
            let got = f.read(&mut buf[..want]).map_err(io_failed)?;
            if got == 0 {
                break;
            }
            h.update(&buf[..got]);
            done += got as u64;
            (self.progress)(done, n, "verifying");
        }
        Ok((h, done))
    }

    /// Returns once the whole file is there and its SHA-256 matches, renamed to `dest`.
    pub fn run(&mut self) -> Result<PathBuf, SetupError> {
        if self.dest.is_file() && file_len(&self.dest) == Some(self.size) && !self.part().exists() {
            return Ok(self.dest.clone()); // finished and checked before a restart
        }
        if let Some(parent) = self.dest.parent() {
            std::fs::create_dir_all(parent).map_err(io_failed)?;
        }
        let mut meta = load_meta(&self.meta());
        if !meta.is_null() && (meta["sha256"].as_str() != Some(&self.sha256) || meta["size"].as_u64() != Some(self.size)) {
            self.discard(); // left by another version of this file
            meta = Value::Null;
        }
        let tls = match &self.tls {
            Some(t) => t.clone(),
            None => Tls::product().map_err(|m| SetupError::failed("network", m))?,
        };
        let mut start = self.done_bytes();
        if start > 0 && meta["etag"].as_str().is_none() {
            start = 0; // nothing to tell the server which file those bytes came from
        }
        // write, not append: on Windows a file opened to append cannot be cut to length ("Access is denied")
        OpenOptions::new().create(true).write(true).truncate(false).open(self.part()).and_then(|f| f.set_len(start)).map_err(io_failed)?;
        let (mut h, s) = if start > 0 { self.rehash(start)? } else { (Sha256::new(), 0) };
        start = s;
        let mut etag = meta["etag"].as_str().map(str::to_string);
        let mut tries = 0u32;
        let mut ignored_once = false;
        let name = self.dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        while start < self.size {
            self.control.check()?;
            match self.fetch(start, etag.clone(), &mut h, &tls) {
                Ok((s, e)) => {
                    start = s;
                    etag = e;
                    tries = 0;
                }
                Err(Fetch::Err(e)) => return Err(e),
                Err(Fetch::Restart { whole }) => {
                    if whole && !ignored_once {
                        ignored_once = true; // one 200 to a Range can be a passing hiccup at the edge: ask once more first
                        eprintln!("setup: the server sent the whole of {name} to a resume; asking again");
                        self.control.sleep(1.0)?;
                        continue;
                    }
                    eprintln!("setup: the file on the server changed or ignores Range; starting {name} over");
                    OpenOptions::new().write(true).open(self.part()).and_then(|f| f.set_len(0)).map_err(io_failed)?;
                    h = Sha256::new();
                    start = 0;
                    etag = None;
                }
                Err(Fetch::Net(NetError::Certificate(_))) => {
                    return Err(SetupError::failed("network", "Lastbook could not confirm it is talking to the download server, so it stopped. A network that intercepts secure connections (some offices, hotels) can cause this."));
                }
                Err(Fetch::Net(NetError::Refused(m))) => return Err(SetupError::failed("network", m)),
                Err(Fetch::Net(NetError::Io(e))) | Err(Fetch::Io(e)) => {
                    if disk_full(&e) {
                        return Err(SetupError::failed("no_space", "The disk filled up while downloading. Free some space, then try again."));
                    }
                    let now = self.done_bytes(); // h already holds every byte written: it is updated just after each write
                    tries = if now > start { 0 } else { tries + 1 };
                    start = now;
                    etag = load_meta(&self.meta())["etag"].as_str().map(str::to_string).or(etag);
                    if tries >= NETWORK_TRIES {
                        return Err(SetupError::failed("network", format!("The connection kept breaking ({}). Check the internet connection, then try again.", kind_name(&e))));
                    }
                    eprintln!("setup: download broke at {start} of {}: {e}; trying again", self.size);
                    let wait = self.backoff[(tries as usize).min(self.backoff.len() - 1)];
                    self.control.sleep(wait)?;
                }
            }
        }
        if hex::encode(h.finalize()) != self.sha256 {
            self.discard();
            return Err(SetupError::BadChecksum("The downloaded file did not match its published checksum.".into()));
        }
        std::fs::rename(self.part(), &self.dest).map_err(io_failed)?;
        let _ = std::fs::remove_file(self.meta());
        Ok(self.dest.clone())
    }

    /// Reads until the end or a break: (bytes done, ETag). Restart when the server's file is not the one begun.
    fn fetch(&mut self, mut start: u64, etag: Option<String>, h: &mut Sha256, tls: &Tls) -> Result<(u64, Option<String>), Fetch> {
        let mut r = net::get(&self.url, start, etag.as_deref(), &self.allow, tls, self.timeout).map_err(Fetch::Net)?;
        if r.status == 416 || r.status >= 400 {
            if (500..600).contains(&r.status) {
                return Err(Fetch::Io(io::Error::other(format!("HTTP {}", r.status))));
            }
            return Err(Fetch::Err(SetupError::failed("network", format!("The download server refused the file (HTTP {}).", r.status))));
        }
        let new_etag = r.header("ETag").map(str::to_string);
        match r.status {
            206 => {
                let cr = r.header("Content-Range").and_then(content_range);
                let moved = matches!((&etag, &new_etag), (Some(a), Some(b)) if a != b);
                if cr.is_none_or(|(s, total)| s != start || total != self.size) || moved {
                    return Err(Fetch::Restart { whole: false });
                }
            }
            200 => {
                if start > 0 {
                    return Err(Fetch::Restart { whole: true }); // the whole file again: its copy changed, or Range is ignored
                }
                if let Some(len) = r.header("Content-Length").and_then(|v| v.trim().parse::<u64>().ok()) {
                    if len != self.size {
                        return Err(Fetch::Err(SetupError::BadChecksum(format!("The file on the server is {len} bytes, not the {} Lastbook expects.", self.size))));
                    }
                }
            }
            s => return Err(Fetch::Io(io::Error::other(format!("HTTP {s}")))),
        }
        let etag = new_etag.or(etag);
        let mut meta = json!({"url": self.url, "size": self.size, "sha256": self.sha256, "etag": etag, "bytes_done": start});
        save_meta(&self.meta(), &meta)?;
        let mut saved = Instant::now();
        let mut f = OpenOptions::new().write(true).open(self.part())?;
        f.seek(SeekFrom::Start(start))?;
        let mut buf = vec![0u8; CHUNK];
        while start < self.size {
            self.control.check().map_err(Fetch::Err)?;
            let want = buf.len().min((self.size - start) as usize);
            let got = r.body().read(&mut buf[..want])?;
            if got == 0 {
                return Err(Fetch::Io(io::Error::new(io::ErrorKind::UnexpectedEof, "the connection closed early")));
            }
            f.write_all(&buf[..got])?;
            h.update(&buf[..got]);
            start += got as u64;
            (self.progress)(start, self.size, "downloading");
            if saved.elapsed() > SAVE_EVERY {
                f.flush()?;
                meta["bytes_done"] = json!(start);
                save_meta(&self.meta(), &meta)?;
                saved = Instant::now();
            }
        }
        f.flush()?;
        f.sync_all()?;
        meta["bytes_done"] = json!(start);
        save_meta(&self.meta(), &meta)?;
        Ok((start, etag))
    }
}

fn kind_name(e: &io::Error) -> String {
    format!("{:?}", e.kind())
}

fn io_failed(e: io::Error) -> SetupError {
    if disk_full(&e) {
        SetupError::failed("no_space", "The disk filled up while downloading. Free some space, then try again.")
    } else {
        SetupError::failed("network", format!("Lastbook could not write a file ({e})."))
    }
}

fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

fn file_len(p: &Path) -> Option<u64> {
    std::fs::metadata(p).ok().map(|m| m.len())
}

fn load_meta(path: &Path) -> Value {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null)
}

fn save_meta(path: &Path, d: &Value) -> io::Result<()> {
    let tmp = with_suffix(path, ".tmp");
    std::fs::write(&tmp, serde_json::to_vec(d).unwrap_or_default())?;
    std::fs::rename(tmp, path)
}

/// (start, total) from `bytes 100-199/1000`.
pub fn content_range(v: &str) -> Option<(u64, u64)> {
    let rest = v.trim().strip_prefix("bytes ")?;
    let (span, total) = rest.split_once('/')?;
    Some((span.split('-').next()?.trim().parse().ok()?, total.trim().parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(content_range("bytes 100-199/1000"), Some((100, 1000)));
        assert_eq!(content_range("bytes */1000"), None);
        assert_eq!(content_range(""), None);
    }

    use super::super::testserver;

    fn sha(b: &[u8]) -> String {
        hex::encode(Sha256::digest(b))
    }

    fn dl(srv: &testserver::TestServer, name: &str, dest: &Path, control: Arc<Control>) -> Download {
        let data = srv.files.lock().unwrap().data[name].clone();
        let mut d = Download::new(&format!("{}/{name}", srv.base), dest, data.len() as u64, &sha(&data), control, Box::new(|_, _, _| {}), srv.allow(), Some(srv.tls.clone()));
        d.backoff = vec![0.05; 6];
        d.timeout = Duration::from_secs(5);
        d
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lb-dl-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn payload(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i * 7 % 251) as u8).collect()
    }

    #[test]
    fn whole_file() {
        let srv = testserver::start();
        srv.put("a.zip", payload(2_097_275));
        let d = tmp("whole");
        let got = dl(&srv, "a.zip", &d.join("a.zip"), Control::new()).run().unwrap();
        assert_eq!(std::fs::read(&got).unwrap(), payload(2_097_275));
        assert!(!d.join("a.zip.part").exists() && !d.join("a.zip.part.json").exists());
    }

    #[test]
    fn resumes_after_a_cut() {
        let srv = testserver::start();
        srv.put("a.zip", payload(2_097_275));
        srv.files.lock().unwrap().cut_after.insert("a.zip".into(), vec![700_000, 800_000]);
        let d = tmp("cut");
        dl(&srv, "a.zip", &d.join("a.zip"), Control::new()).run().unwrap();
        assert_eq!(std::fs::read(d.join("a.zip")).unwrap(), payload(2_097_275));
        let log = srv.files.lock().unwrap().log.clone();
        assert_eq!(log, ["GET /a.zip", "GET /a.zip Range: bytes=700000-", "GET /a.zip Range: bytes=1500000-"]);
    }

    #[test]
    fn a_changed_file_starts_over() {
        // Half of the old copy is on disk with its ETag; the server's copy has another ETag: the server answers the If-Range
        // with the whole file, asked once more, then the download starts over and comes out right.
        let srv = testserver::start();
        srv.put("a.zip", payload(1_500_000));
        srv.files.lock().unwrap().etag.insert("a.zip".into(), "\"two\"".into());
        let d = tmp("etag");
        std::fs::write(d.join("a.zip.part"), &payload(1_500_000)[..500_000]).unwrap();
        let data = payload(1_500_000);
        std::fs::write(d.join("a.zip.part.json"), serde_json::to_vec(&json!({"url": format!("{}/a.zip", srv.base), "size": 1_500_000,
            "sha256": sha(&data), "etag": "\"one\"", "bytes_done": 500_000})).unwrap()).unwrap();
        dl(&srv, "a.zip", &d.join("a.zip"), Control::new()).run().unwrap();
        assert_eq!(std::fs::read(d.join("a.zip")).unwrap(), data);
        let log = srv.files.lock().unwrap().log.clone();
        assert_eq!(log, ["GET /a.zip Range: bytes=500000-", "GET /a.zip Range: bytes=500000-", "GET /a.zip"]);
    }

    #[test]
    fn whole_file_to_a_resume_is_asked_again_then_restarted() {
        let srv = testserver::start();
        srv.put("m.gguf", payload(2_097_275));
        {
            let mut f = srv.files.lock().unwrap();
            f.cut_after.insert("m.gguf".into(), vec![700_000]);
            f.ignore_once.insert("m.gguf".into(), 1);
        }
        let d = tmp("ignore");
        dl(&srv, "m.gguf", &d.join("m.gguf"), Control::new()).run().unwrap();
        assert_eq!(std::fs::read(d.join("m.gguf")).unwrap(), payload(2_097_275));
        srv.files.lock().unwrap().ignore_range.insert("m.gguf".into());
        srv.files.lock().unwrap().cut_after.insert("m.gguf".into(), vec![700_000]);
        let d2 = tmp("ignore2");
        dl(&srv, "m.gguf", &d2.join("m.gguf"), Control::new()).run().unwrap();     // started over: Range is ignored for good
        assert_eq!(std::fs::read(d2.join("m.gguf")).unwrap(), payload(2_097_275));
    }

    #[test]
    fn wrong_bytes_are_refused() {
        let srv = testserver::start();
        srv.put("a.zip", payload(300_000));
        srv.files.lock().unwrap().corrupt.insert("a.zip".into(), 1);
        let d = tmp("corrupt");
        let r = dl(&srv, "a.zip", &d.join("a.zip"), Control::new()).run();
        assert!(matches!(r, Err(SetupError::BadChecksum(_))));
        assert!(!d.join("a.zip.part").exists() && !d.join("a.zip").exists());
        dl(&srv, "a.zip", &d.join("a.zip"), Control::new()).run().unwrap();     // the next answer is right
    }

    #[test]
    fn redirects_and_refusals() {
        let srv = testserver::start();
        srv.put("b.zip", payload(1000));
        {
            let mut f = srv.files.lock().unwrap();
            f.redirect.insert("a.zip".into(), format!("{}/b.zip", srv.base));
            f.redirect.insert("evil.zip".into(), "https://evil.example/b.zip".into());
        }
        let d = tmp("redirect");
        let data = payload(1000);
        let mut r = Download::new(&format!("{}/a.zip", srv.base), &d.join("a.zip"), 1000, &sha(&data), Control::new(), Box::new(|_, _, _| {}), srv.allow(), Some(srv.tls.clone()));
        r.run().unwrap();
        let mut evil = Download::new(&format!("{}/evil.zip", srv.base), &d.join("e.zip"), 1000, &sha(&data), Control::new(), Box::new(|_, _, _| {}), srv.allow(), Some(srv.tls.clone()));
        assert!(matches!(evil.run(), Err(SetupError::Failed { code, .. }) if code == "network"));
        let mut missing = Download::new(&format!("{}/nothing.zip", srv.base), &d.join("n.zip"), 1000, &sha(&data), Control::new(), Box::new(|_, _, _| {}), srv.allow(), Some(srv.tls.clone()));
        assert!(matches!(missing.run(), Err(SetupError::Failed { message, .. }) if message.contains("HTTP 404")));
        // a certificate the roots do not hold: refused, not trusted from the system
        let other = testserver::start();
        other.put("x.zip", data.clone());
        let mut untrusted = Download::new(&format!("{}/x.zip", other.base), &d.join("x.zip"), 1000, &sha(&data), Control::new(), Box::new(|_, _, _| {}), other.allow(), Some(srv.tls.clone()));
        assert!(matches!(untrusted.run(), Err(SetupError::Failed { message, .. }) if message.contains("could not confirm")));
    }

    #[test]
    fn pause_then_resume() {
        let srv = testserver::start();
        srv.put("a.zip", payload(3_000_000));
        srv.files.lock().unwrap().cut_after.insert("a.zip".into(), vec![1_000_000]);
        let d = tmp("pause");
        let c = Control::new();
        let c2 = c.clone();
        let mut first = dl(&srv, "a.zip", &d.join("a.zip"), c.clone());
        first.backoff = vec![5.0; 6];       // after the cut it waits, and is paused while waiting
        let t = std::thread::spawn(move || first.run());
        std::thread::sleep(Duration::from_millis(400));
        c2.stop("paused");
        assert!(matches!(t.join().unwrap(), Err(SetupError::Stopped(w)) if w == "paused"));
        assert!(d.join("a.zip.part").exists());
        dl(&srv, "a.zip", &d.join("a.zip"), Control::new()).run().unwrap();
        assert_eq!(std::fs::read(d.join("a.zip")).unwrap(), payload(3_000_000));
        assert!(srv.files.lock().unwrap().log.last().unwrap().contains("Range: bytes=1000000-"));
    }

    #[test]
    fn control_stops_a_sleep() {
        let c = Control::new();
        let c2 = c.clone();
        let t = std::thread::spawn(move || c2.sleep(30.0));
        std::thread::sleep(Duration::from_millis(50));
        c.stop("paused");
        assert!(matches!(t.join().unwrap(), Err(SetupError::Stopped(w)) if w == "paused"));
        c.stop("cancelled");
        c.stop("paused");
        assert_eq!(c.stopped().as_deref(), Some("cancelled"));
    }
}
