//! Check for updates: only when the person asks (About's button). Nothing runs at startup or on a timer, and nothing is
//! downloaded, installed or started: the check only says whether a newer version is published. Ported from the original
//! Lastbook's update check.
//!
//! - One small GET of the update information (dist::update_url, app-v1.json at the download site's root) through setup's
//!   own HTTPS client, trusting only the roots built into the program, to the download host only (not the model hosts).
//! - While no download site is chosen (the host is the placeholder) there is no update information: the check answers at
//!   once and opens no connection.
//! - The information is for display only: {"version": 1, "mac": {"version", "min_os"}, "windows": {...}, "linux": {...}}.
//!
//! Routes: GET /api/update/state, POST /api/update/check.

use crate::dist;
use crate::setup::net::{self, Allow, NetError, Tls};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MAX_BODY: usize = 4096;
const OFFLINE: &str = "Lastbook Europe could not reach the internet. Everything else in Lastbook Europe still works without it; try again when you are online.";

/// What the check can say, each a code the interface words in the person's language (web/locales: update.code.<code>), with
/// its params; the English sentence stays beside them, for logs and for an interface that does not know the code.
pub const CODES: [&str; 10] = ["unsupported", "no_site", "network", "certificate", "refused", "offline", "not_published", "server", "bad_info", "too_old_os"];

/// A state that says something: its code and params beside the English.
fn said(state: &str, code: &str, params: Value, message: impl Into<String>) -> Value {
    json!({"state": state, "code": code, "params": params, "message": message.into()})
}

/// This system's key in the update information.
pub fn platform_key() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some("mac")
    } else if cfg!(windows) {
        Some("windows")
    } else if cfg!(target_os = "linux") {
        Some("linux")
    } else {
        None
    }
}

/// A version like 1.2.3 (up to four numbers) as numbers to compare, or None.
pub fn ver(s: &str) -> Option<Vec<u64>> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.is_empty() || parts.len() > 4 || parts.iter().any(|p| p.is_empty() || p.len() > 6 || !p.chars().all(|c| c.is_ascii_digit())) {
        return None;
    }
    Some(parts.iter().map(|p| p.parse().unwrap_or(0)).collect())
}

/// This system's entry in the update information: (version, minimum system), or None if it is not there or not plain.
pub fn validate(m: &Value, key: &str) -> Option<(String, Option<String>)> {
    if m["version"] != 1 || !m[key].is_object() {
        return None;
    }
    let v = m[key]["version"].as_str().filter(|v| ver(v).is_some())?;
    let min = m[key]["min_os"].as_str().filter(|v| ver(v).is_some()).map(str::to_string);
    Some((v.to_string(), min))
}

/// The system's version, to compare with a minimum: macOS 14.5, Windows 10.0.26200, Linux's VERSION_ID (Ubuntu's 24.04).
pub fn os_version() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("/usr/bin/sw_vers").arg("-productVersion").output().ok()?;
        return Some(String::from_utf8_lossy(&out.stdout).trim().to_string());
    }
    #[cfg(target_os = "linux")]
    {
        let t = std::fs::read_to_string("/etc/os-release").ok()?;
        return t.lines().find_map(|l| l.strip_prefix("VERSION_ID=")).map(|v| v.trim_matches('"').to_string());
    }
    #[allow(unreachable_code)]
    None
}

#[derive(Clone)]
pub struct Updater(Arc<Inner>);

struct Inner {
    url: Option<String>,
    allow: Allow,
    tls: Option<Tls>,
    key: Option<&'static str>,
    version: String,
    os: Option<String>,
    state: Mutex<Value>,
}

impl Updater {
    /// The product's check: the project's releases (and where GitHub sends their files) only, this program's version.
    pub fn product() -> Updater {
        Updater::new(dist::update_url(), Allow::Product, None, platform_key(), env!("CARGO_PKG_VERSION"), os_version())
    }

    pub fn new(url: Option<String>, allow: Allow, tls: Option<Tls>, key: Option<&'static str>, version: &str, os: Option<String>) -> Updater {
        Updater(Arc::new(Inner { url, allow, tls, key, version: version.to_string(), os, state: Mutex::new(json!({"state": "idle"})) }))
    }

    pub fn state(&self) -> Value {
        let mut out = self.0.state.lock().unwrap().clone();
        out["current"] = json!(self.0.version);
        out["website"] = json!(dist::WEBSITE);
        out
    }

    fn set(&self, v: Value) {
        *self.0.state.lock().unwrap() = v;
    }

    pub fn check(&self) -> Value {
        let Some(key) = self.0.key else {
            return json!({"error": "Updates are for the Mac, Windows and Linux app.", "code": "unsupported", "params": {}});
        };
        let Some(url) = self.0.url.clone() else {
            self.set(said("no_site", "no_site", json!({}), "This copy has no download site to check for updates yet."));
            return self.state(); // nothing to ask, so nothing is asked
        };
        if self.0.state.lock().unwrap()["state"] == "checking" {
            return self.state();
        }
        self.set(json!({"state": "checking"}));
        let me = self.clone();
        std::thread::Builder::new().name("update-check".into()).spawn(move || {
            let result = me.fetch(&url);
            me.set(me.judge(result, key));
        }).ok();
        self.state()
    }

    /// (HTTP status, body) of the update information, or a state saying what went wrong.
    fn fetch(&self, url: &str) -> Result<(u16, Vec<u8>), Value> {
        let tls = match &self.0.tls {
            Some(t) => t.clone(),
            None => Tls::product().map_err(|m| said("error", "network", json!({}), m))?,
        };
        let mut r = net::get(url, 0, None, &self.0.allow, &tls, Duration::from_secs(15)).map_err(|e| match e {
            NetError::Certificate(_) => said("error", "certificate", json!({}), "Lastbook Europe could not confirm it was talking to the real download site, so it stopped. A network that intercepts secure connections (some offices, hotels) can cause this."),
            NetError::Refused(m) => said("error", "refused", json!({}), m),
            NetError::Io(_) => said("error", "offline", json!({}), OFFLINE),
        })?;
        if r.status != 200 {
            return Ok((r.status, Vec::new()));
        }
        match r.read_limited(MAX_BODY) {
            Ok(Some(b)) => Ok((200, b)),
            Ok(None) => Ok((200, vec![0; MAX_BODY + 1])), // larger than any real answer
            Err(_) => Err(said("error", "offline", json!({}), OFFLINE)),
        }
    }

    fn judge(&self, result: Result<(u16, Vec<u8>), Value>, key: &str) -> Value {
        let (status, data) = match result {
            Ok(x) => x,
            Err(state) => return state,
        };
        if status == 403 || status == 404 {
            return said("not_published", "not_published", json!({}), "No update information yet. Try again later.");
        }
        if status != 200 || data.len() > MAX_BODY {
            return said("error", "server", json!({}), "The download site did not answer properly. Try again later.");
        }
        let m: Value = serde_json::from_slice(&data).unwrap_or(Value::Null);
        let Some((version, min_os)) = validate(&m, key) else {
            if m["version"] == 1 && m.get(key).is_none() {
                return said("not_published", "not_published", json!({}), "No update information yet. Try again later."); // this system not listed yet
            }
            return said("error", "bad_info", json!({}), "The update information on the download site could not be read. Try again later.");
        };
        if ver(&self.0.version).is_some_and(|mine| ver(&version).is_some_and(|v| v <= mine)) {
            return json!({"state": "up_to_date", "latest": version});
        }
        if let (Some(min), Some(os)) = (min_os.as_deref().and_then(ver), self.0.os.as_deref().and_then(ver)) {
            if os < min {
                let name = match key {
                    "mac" => "macOS",
                    "windows" => "Windows",
                    _ => "Linux",
                };
                let min = min_os.unwrap_or_default();
                let mut s = said("too_old_os", "too_old_os", json!({"version": version, "os": name, "min_os": min}), format!("Version {version} needs {name} {min} or later."));
                s["version"] = json!(version);
                return s;
            }
        }
        json!({"state": "available", "version": version})
    }

    /// Answers one /api/update/ request: (status, JSON).
    pub fn handle(&self, method: &str, name: &str) -> (u16, Value) {
        match (method, name) {
            ("GET" | "HEAD", "state") => (200, self.state()),
            ("POST", "check") => (200, self.check()),
            _ => (404, json!({"error": "not found"})),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup::testserver;

    fn wait(u: &Updater) -> Value {
        for _ in 0..200 {
            let s = u.state();
            if s["state"] != "checking" {
                return s;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("still checking");
    }

    #[test]
    fn versions_and_entries() {
        assert_eq!(ver("10.0.26200"), Some(vec![10, 0, 26200]));
        assert!(ver("1.2.x").is_none() && ver("").is_none() && ver("1.2.3.4.5").is_none());
        let m = json!({"version": 1, "mac": {"version": "0.2.0", "min_os": "13.0"}, "windows": {"version": "bad"}});
        assert_eq!(validate(&m, "mac"), Some(("0.2.0".into(), Some("13.0".into()))));
        assert_eq!(validate(&m, "windows"), None);
        assert_eq!(validate(&json!({"version": 2, "mac": {"version": "1.0"}}), "mac"), None);
    }

    #[test]
    fn the_product_asks_the_newest_release() {
        assert_eq!(dist::update_url().as_deref(), Some("https://github.com/NikosGu/lastbook_Europe/releases/latest/download/app-v1.json"));
        assert!(net::url_allowed(&dist::update_url().unwrap(), &Allow::Product));
    }

    #[test]
    fn no_site_means_no_connection() {
        let u = Updater::new(None, Allow::Product, None, platform_key(), env!("CARGO_PKG_VERSION"), os_version());
        let s = u.check();
        assert_eq!(s["state"], "no_site");
        assert_eq!((s["code"].as_str(), &s["params"]), (Some("no_site"), &json!({})));
        assert!(s["message"].as_str().is_some_and(|m| m.contains("no download site")), "the English stays beside the code");
        assert_eq!(s["current"], env!("CARGO_PKG_VERSION"));
        assert_eq!(u.handle("GET", "nothing").0, 404);
    }

    #[test]
    fn answers_from_a_local_server() {
        let srv = testserver::start();
        let url = format!("{}/app-v1.json", srv.base);
        let check = |body: &str, version: &str, os: Option<&str>| -> Value {
            srv.put("app-v1.json", body.as_bytes().to_vec());
            let u = Updater::new(Some(url.clone()), srv.allow(), Some(srv.tls.clone()), Some("mac"), version, os.map(str::to_string));
            u.check();
            wait(&u)
        };
        let info = r#"{"version": 1, "mac": {"version": "0.2.0", "min_os": "14.0"}}"#;
        assert_eq!(check(info, "0.1.0", Some("15.1"))["state"], "available");
        assert_eq!(check(info, "0.2.0", Some("15.1"))["state"], "up_to_date");
        let s = check(info, "0.1.0", Some("13.6"));
        assert_eq!((s["state"].as_str(), s["message"].as_str()), (Some("too_old_os"), Some("Version 0.2.0 needs macOS 14.0 or later.")));
        assert_eq!((s["code"].as_str(), &s["params"]), (Some("too_old_os"), &json!({"version": "0.2.0", "os": "macOS", "min_os": "14.0"})));
        let s = check(r#"{"version": 1, "windows": {"version": "0.2.0"}}"#, "0.1.0", None);
        assert_eq!((s["state"].as_str(), s["code"].as_str(), &s["params"]), (Some("not_published"), Some("not_published"), &json!({})));
        let s = check("not json", "0.1.0", None);
        assert_eq!((s["state"].as_str(), s["code"].as_str(), &s["params"]), (Some("error"), Some("bad_info"), &json!({})));
        srv.put("app-v1.json", vec![b'x'; MAX_BODY + 1]);
        let u = Updater::new(Some(url.clone()), srv.allow(), Some(srv.tls.clone()), Some("mac"), "0.1.0", None);
        u.check();
        assert_eq!(wait(&u)["code"], "server");
        // a host it does not know is never asked
        let u = Updater::new(Some("https://example.com/app-v1.json".into()), srv.allow(), Some(srv.tls.clone()), Some("mac"), "0.1.0", None);
        u.check();
        let s = wait(&u);
        assert_eq!((s["state"].as_str(), s["code"].as_str(), &s["params"]), (Some("error"), Some("refused"), &json!({})));
        assert!(s["message"].as_str().is_some_and(|m| !m.is_empty()));
        // a server that is not there: offline, said as such
        let gone = format!("https://127.0.0.1:{}/app-v1.json", std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port());
        let u = Updater::new(Some(gone), srv.allow(), Some(srv.tls.clone()), Some("mac"), "0.1.0", None);
        u.check();
        assert_eq!(wait(&u)["code"], "offline");
        let s = Updater::new(None, srv.allow(), None, None, "0.1.0", None).check();
        assert_eq!((s["code"].as_str(), &s["params"]), (Some("unsupported"), &json!({})));
    }

    #[test]
    fn every_code_has_words_in_the_interface() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/locales/en.json");
        let words: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        for code in CODES {
            assert!(words[format!("update.code.{code}")].is_string(), "web/locales/en.json has no update.code.{code}");
        }
    }
}
