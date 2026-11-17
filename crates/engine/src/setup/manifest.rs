//! What can be installed: manifest-v1.json, bundled with the app at build time, or fetched once from the download site
//! when setup opens (engine::dist: not until the site exists). A fetched one is used only if it parses, says version 1 and
//! every address in it is allowed, and it can only add entries or mark them available: what the app was built with stays.

use super::net::{self, Allow, Tls};
use crate::{dist, paths, regions};
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const KEYS: [&str; 4] = ["packs", "models", "maps", "address_indexes"];

/// A file or id name: `^[A-Za-z0-9][A-Za-z0-9._-]{0,120}$`.
pub fn name_ok(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty() && b.len() <= 121 && b[0].is_ascii_alphanumeric() && b.iter().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}

pub fn sha_ok(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn from(url: &Value, product_host: &str, allow: &Allow) -> bool {
    let Some(url) = url.as_str() else { return false };
    if !net::url_allowed(url, allow) {
        return false;
    }
    match allow {
        Allow::Only(_) => true,
        Allow::Product => url::Url::parse(url).ok().and_then(|u| u.host_str().map(|h| h.eq_ignore_ascii_case(product_host))).unwrap_or(false),
    }
}

fn int(v: &Value) -> Option<u64> {
    v.as_u64()
}

fn files(list: &Value, allow: &Allow) -> Option<Vec<Value>> {
    let mut out = Vec::new();
    for f in list.as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
        let (Some(name), Some(size), Some(sha)) = (f["name"].as_str(), int(&f["size"]), f["sha256"].as_str()) else { return None };
        if !name_ok(name) || !sha_ok(sha) || !from(&f["url"], dist::DOWNLOAD_HOST, allow) {
            return None;
        }
        out.push(json!({"name": name, "url": f["url"], "size": size, "sha256": sha, "kind": f["kind"].as_str().filter(|k| !k.is_empty()).unwrap_or("zip")}));
    }
    Some(out)
}

fn text(v: &Value, default: &str) -> String {
    v.as_str().filter(|s| !s.is_empty()).map(str::to_string).unwrap_or_else(|| default.to_string())
}

/// The manifest, cleaned to the fields setup uses, or None if anything in it is wrong.
pub fn validate(m: &Value, allow: &Allow) -> Option<Value> {
    if !m.is_object() || m["version"].as_u64() != Some(1) || m["version"].is_boolean() {
        return None;
    }
    let list = |k: &str| m[k].as_array().cloned().unwrap_or_default();
    let mut packs = Vec::new();
    for p in list("packs") {
        let id = p["id"].as_str().filter(|i| name_ok(i))?;
        let revision = match &p["revision"] {
            Value::Null => String::new(),
            Value::String(r) if r.is_empty() || name_ok(r) => r.clone(),
            _ => return None,
        };
        let fs = files(&p["files"], allow)?;
        let available = p["available"].as_bool().unwrap_or(false) && !fs.is_empty();
        if available && int(&p["installed_bytes"]).is_none() {
            return None;
        }
        let download: u64 = fs.iter().map(|f| f["size"].as_u64().unwrap_or(0)).sum();
        let map = p["map"].as_str().filter(|m| name_ok(m) && *m != "base");
        packs.push(json!({"id": id, "title": text(&p["title"], id), "summary": text(&p["summary"], ""), "records": int(&p["records"]),
                          "revision": revision, "installed_bytes": int(&p["installed_bytes"]), "download_bytes": download,
                          "available": available, "files": fs, "map": map}));
    }
    let mut out = Map::new();
    for key in ["maps", "address_indexes"] {
        let mut entries = Vec::new();
        for x in list(key) {
            let id = x["id"].as_str().filter(|i| name_ok(i) && regions::is_code(i))?;   // a region code: it names files, so nothing else
            let fs = files(&x["files"], allow)?;
            let available = x["available"].as_bool().unwrap_or(false) && !fs.is_empty() && fs.iter().all(|f| f["size"].as_u64().unwrap_or(0) > 0);
            if available && int(&x["installed_bytes"]).is_none() {
                return None;
            }
            let download: u64 = fs.iter().map(|f| f["size"].as_u64().unwrap_or(0)).sum();
            entries.push(json!({"id": id, "title": text(&x["title"], id), "kind": text(&x["kind"], "region"), "includes": text(&x["includes"], ""),
                                "installed_bytes": int(&x["installed_bytes"]), "download_bytes": download, "available": available, "files": fs}));
        }
        out.insert(key.into(), Value::Array(entries));
    }
    let mut models = Vec::new();
    for x in list("models") {
        let (Some(id), Some(file), Some(size), Some(sha), Some(memory)) =
            (x["id"].as_str(), x["file"].as_str(), int(&x["size"]), x["sha256"].as_str(), int(&x["memory_bytes"])) else { return None };
        if !name_ok(id) || !name_ok(file) || !sha_ok(sha) || !from(&x["url"], dist::MODEL_HOST, allow) {
            return None;
        }
        let mut entry = json!({"id": id, "title": text(&x["title"], id), "file": file, "url": x["url"], "size": size, "sha256": sha,
                               "memory_bytes": memory, "available": x["available"].as_bool().unwrap_or(true)});
        for extra in ["family", "params", "active_params", "ctx", "summary", "engine", "languages"] {   // the catalogue's own fields
            if !x[extra].is_null() {
                entry[extra] = x[extra].clone();
            }
        }
        models.push(entry);
    }
    out.insert("version".into(), json!(1));
    out.insert("published".into(), json!(m["published"].as_str().unwrap_or("")));
    out.insert("packs".into(), Value::Array(packs));
    out.insert("models".into(), Value::Array(models));
    Some(Value::Object(out))
}

pub fn empty() -> Value {
    json!({"version": 1, "published": "", "packs": [], "models": [], "maps": [], "address_indexes": []})
}

/// Where the app keeps the manifest it was built with: its own files' setup/, or a release folder in the source tree.
pub fn bundled_paths() -> Vec<PathBuf> {
    let app = paths::app_dir();
    vec![app.join("setup").join("manifest-v1.json"), app.join("derived").join("release").join("setup").join("manifest-v1.json")]
}

pub fn load_bundled(paths: &[PathBuf]) -> Value {
    for p in paths {
        if let Some(m) = std::fs::read(p).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|v| validate(&v, &Allow::Product)) {
            return m;
        }
    }
    empty()
}

/// What the app was built with stays: for an id the bundled manifest pins (it has files, or the model's SHA-256), its
/// addresses, sizes and SHA-256 are kept and the fetched copy may only change whether it is available. A fetched copy may
/// add ids. One bad publish never takes an option away.
pub fn merge(bundled: &Value, fetched: &Value) -> Value {
    let mut out = fetched.clone();
    for key in KEYS {
        let mut built: Vec<Value> = bundled[key].as_array().cloned().unwrap_or_default();
        let mut merged = Vec::new();
        for x in fetched[key].as_array().cloned().unwrap_or_default() {
            let pos = built.iter().position(|b| b["id"] == x["id"]);
            let b = pos.map(|i| built.remove(i));
            let pinned = b.as_ref().is_some_and(|b| b["files"].as_array().is_some_and(|f| !f.is_empty()) || b["sha256"].as_str().is_some());
            if pinned {
                let mut b = b.unwrap();
                b["available"] = json!(x["available"].as_bool().unwrap_or(false));
                merged.push(b);
            } else {
                merged.push(x);
            }
        }
        merged.extend(built);
        out[key] = Value::Array(merged);
    }
    out
}

struct State {
    current: Value,
    source: &'static str,
    fetching: bool,
}

/// The bundled manifest at once; a fetched one is merged in when it arrives.
#[derive(Clone)]
pub struct Manifest {
    state: Arc<Mutex<State>>,
    remote: Option<String>,
    allow: Allow,
    tls: Option<Tls>,
}

impl Manifest {
    pub fn new(bundled: Value, remote: Option<String>, allow: Allow, tls: Option<Tls>) -> Manifest {
        Manifest { state: Arc::new(Mutex::new(State { current: bundled, source: "bundled", fetching: false })), remote, allow, tls }
    }

    /// The product's: the bundled copy, and the download site's once it exists.
    pub fn product() -> Manifest {
        Manifest::new(load_bundled(&bundled_paths()), dist::manifest_url(), Allow::Product, None)
    }

    pub fn current(&self) -> Value {
        self.state.lock().unwrap().current.clone()
    }

    pub fn set_current(&self, m: Value) {
        self.state.lock().unwrap().current = m;
    }

    pub fn source(&self) -> &'static str {
        self.state.lock().unwrap().source
    }

    /// One background GET of the remote manifest per Manifest; failures leave the bundled copy.
    pub fn fetch_once(&self) {
        let Some(remote) = self.remote.clone() else { return };
        {
            let mut s = self.state.lock().unwrap();
            if s.fetching {
                return;
            }
            s.fetching = true;
        }
        let me = self.clone();
        std::thread::spawn(move || me.fetch(&remote));
    }

    pub fn fetch(&self, remote: &str) {
        let tls = match &self.tls {
            Some(t) => t.clone(),
            None => match Tls::product() {
                Ok(t) => t,
                Err(_) => return,
            },
        };
        let Some(data) = net::fetch_small(remote, 1 << 20, &self.allow, &tls, Duration::from_secs(10)) else { return };
        self.take_fetched(&data);
    }

    /// A fetched manifest's bytes: merged in if they validate.
    pub fn take_fetched(&self, data: &[u8]) {
        let Some(m) = serde_json::from_slice::<Value>(data).ok().and_then(|v| validate(&v, &self.allow)) else { return };
        let mut s = self.state.lock().unwrap();
        s.current = merge(&s.current, &m);
        s.source = "fetched";
    }

    fn find(&self, key: &str, id: &str) -> Option<Value> {
        self.current()[key].as_array()?.iter().find(|x| x["id"].as_str() == Some(id)).cloned()
    }

    pub fn pack(&self, id: &str) -> Option<Value> {
        self.find("packs", id)
    }

    pub fn map(&self, id: &str) -> Option<Value> {
        self.find("maps", id)
    }

    pub fn address_index(&self, id: &str) -> Option<Value> {
        self.find("address_indexes", id)
    }

    /// An available model: the one named, or the first.
    pub fn model(&self, id: Option<&str>) -> Option<Value> {
        self.current()["models"].as_array()?.iter().find(|m| m["available"].as_bool().unwrap_or(false) && id.is_none_or(|i| m["id"].as_str() == Some(i))).cloned()
    }

    pub fn models(&self) -> Vec<Value> {
        self.current()["models"].as_array().cloned().unwrap_or_default()
    }

    /// What the window sees: everything but the addresses.
    pub fn public(&self) -> Value {
        let m = self.current();
        let strip = |v: &Value| {
            let mut v = v.clone();
            if let Some(o) = v.as_object_mut() {
                o.remove("url");
            }
            v
        };
        let with_files = |list: &Value| -> Value {
            Value::Array(list.as_array().cloned().unwrap_or_default().into_iter().map(|mut x| {
                let fs: Vec<Value> = x["files"].as_array().cloned().unwrap_or_default().iter().map(strip).collect();
                x["files"] = Value::Array(fs);
                x
            }).collect())
        };
        json!({"source": self.source(), "published": m["published"], "packs": with_files(&m["packs"]),
               "models": Value::Array(m["models"].as_array().cloned().unwrap_or_default().iter().map(strip).collect()),
               "maps": with_files(&m["maps"]), "address_indexes": with_files(&m["address_indexes"])})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h() -> String {
        dist::DOWNLOAD_HOST.to_string()
    }

    /// A pack whose file is on `url_host`: on the download host, in the data release, as the product's are.
    fn pack(id: &str, url_host: &str, sha: char, size: u64) -> Value {
        let url = if url_host == h() { format!("{}/{id}.zip", dist::download_base()) } else { format!("https://{url_host}/{id}.zip") };
        json!({"id": id, "available": true, "installed_bytes": 5, "files": [{"name": format!("{id}.zip"), "url": url, "size": size, "sha256": sha.to_string().repeat(64)}]})
    }

    #[test]
    fn rules() {
        let mut m = json!({"version": 1, "packs": [pack("small", "evil.com", '0', 1)], "models": []});
        assert!(validate(&m, &Allow::Product).is_none());
        m["packs"][0] = pack("small", &h(), '0', 1);
        assert!(validate(&m, &Allow::Product).is_some());
        let mut v2 = m.clone();
        v2["version"] = json!(2);
        assert!(validate(&v2, &Allow::Product).is_none());
        m["packs"][0]["files"][0]["name"] = json!("../a.zip");
        assert!(validate(&m, &Allow::Product).is_none());
        // a model only from Hugging Face; a pack never from there
        let model = json!({"version": 1, "models": [{"id": "gemma", "file": "m.gguf", "url": "https://huggingface.co/m", "size": 1, "sha256": "1".repeat(64), "memory_bytes": 1}]});
        assert!(validate(&model, &Allow::Product).is_some());
        let bad = json!({"version": 1, "packs": [pack("small", "huggingface.co", '0', 1)]});
        assert!(validate(&bad, &Allow::Product).is_none());
        // on github.com, only this project's releases
        let mut elsewhere = json!({"version": 1, "packs": [pack("small", &h(), '0', 1)]});
        elsewhere["packs"][0]["files"][0]["url"] = json!("https://github.com/someone/else/releases/download/v1/small.zip");
        assert!(validate(&elsewhere, &Allow::Product).is_none());
    }

    #[test]
    fn the_data_release_base_is_valid() {
        // packaging/data/manifest-base.json: the model catalogue the data release's manifest starts from
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/data/manifest-base.json");
        let base: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let v = validate(&base, &Allow::Product).expect("valid");
        assert!(!v["models"].as_array().unwrap().is_empty());
    }

    #[test]
    fn region_ids() {
        let entry = |id: &str| json!({"version": 1, "maps": [{"id": id, "title": "x", "available": true, "installed_bytes": 9,
            "files": [{"name": "m.zip", "url": format!("{}/m.zip", dist::download_base()), "size": 3, "sha256": "2".repeat(64)}]}]});
        assert!(validate(&entry("DE-HB"), &Allow::Product).is_some());
        assert!(validate(&entry("LU"), &Allow::Product).is_some());
        for bad in ["../DE-HB", "de-hb", "Bremen", "DE_HB"] {
            assert!(validate(&entry(bad), &Allow::Product).is_none(), "{bad}");
        }
        let mut idx = entry("DE-HB");
        idx["address_indexes"] = idx["maps"].take();
        let v = validate(&idx, &Allow::Product).unwrap();
        assert_eq!(v["address_indexes"][0]["kind"], "region");
    }

    #[test]
    fn fetched_keeps_bundled_entries() {
        let bundled = validate(&json!({"version": 1, "published": "2026-11-12", "packs": [pack("small", &h(), '0', 1)],
            "models": [{"id": "gemma", "file": "m.gguf", "url": "https://huggingface.co/m", "size": 1, "sha256": "1".repeat(64), "memory_bytes": 1}]}), &Allow::Product).unwrap();
        let mut small2 = pack("small", &h(), '3', 3);
        small2["title"] = json!("Small v2");
        let fetched = json!({"version": 1, "published": "2026-11-13", "packs": [pack("medium", &h(), '2', 2), small2], "models": []});
        let m = Manifest::new(bundled, None, Allow::Product, None);
        m.take_fetched(serde_json::to_vec(&fetched).unwrap().as_slice());
        assert_eq!(m.source(), "fetched");
        let ids: Vec<_> = m.current()["packs"].as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap().to_string()).collect();
        assert_eq!(ids, ["medium", "small"]);
        assert_eq!(m.pack("small").unwrap()["title"], "small");           // pinned by the bundled copy
        assert_eq!(m.pack("small").unwrap()["files"][0]["sha256"], "0".repeat(64));
        assert_eq!(m.model(None).unwrap()["id"], "gemma");                   // fetched had none: the bundled one stays
        assert!(m.public()["packs"][0]["files"][0].get("url").is_none());
        m.take_fetched(b"{not json");
        assert_eq!(m.current()["packs"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn names() {
        for ok in ["small", "Lastbook-Map-DE-HB.zip", "gemma-4-26B-A4B-it-UD-Q4_K_M.gguf"] {
            assert!(name_ok(ok), "{ok}");
        }
        for bad in ["", "../x", ".hidden", "a/b", "a b", &"x".repeat(122)] {
            assert!(!name_ok(bad), "{bad}");
        }
    }
}
