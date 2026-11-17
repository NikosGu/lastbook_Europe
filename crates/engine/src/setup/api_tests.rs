//! Setup's flows against the local HTTPS test server, ported from the original Lastbook's app/tests/test_setup.py.

use super::api::*;
use super::install::{self, DATA_NAME};
use super::manifest::{self, Manifest};
use super::testserver::{self, TestServer};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn sha(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("lb-api-{name}-{}-{}", std::process::id(), install::now_secs()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A zip shaped like a real pack: everything under LastbookData/, stored. Returns the unpacked size.
fn make_zip(path: &Path, files: &[(&str, Vec<u8>)], top: bool) -> u64 {
    let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, data) in files {
        let name = if top { format!("{DATA_NAME}/{name}") } else { name.to_string() };
        z.start_file(name, opts).unwrap();
        z.write_all(data).unwrap();
    }
    z.finish().unwrap();
    files.iter().map(|(_, d)| d.len() as u64).sum()
}

fn noise(n: usize, seed: u8) -> Vec<u8> {
    (0..n).map(|i| ((i * 31 + seed as usize) % 253) as u8).collect()
}

struct Env {
    srv: TestServer,
    dir: PathBuf,
    settings: PathBuf,
    parent: PathBuf,
    manifest: Value,
}

impl Env {
    fn new(name: &str) -> Env {
        let srv = testserver::start();
        let dir = tmp(name);
        let settings = dir.join("settings");
        let parent = dir.join("parent");
        std::fs::create_dir_all(&parent).unwrap();
        let mut env = Env { srv, dir, settings, parent, manifest: json!({"version": 1, "packs": [], "models": [], "maps": [], "address_indexes": []}) };
        env.add_pack("small", &[("library-data/catalog.json", b"{\"records\": []}".to_vec()), ("pack.json", b"{\"name\": \"small\", \"map\": \"base\"}".to_vec()),
                                 ("library-data/big.bin", noise(3_000_000, 1)), ("derived/geo/map.sqlite", noise(200_000, 2)),
                                 ("map-set.json", b"".to_vec())]);
        env
    }

    fn url(&self, name: &str) -> String {
        format!("{}/{name}", self.srv.base)
    }

    fn put(&mut self, name: &str, data: Vec<u8>) -> Value {
        let f = json!({"name": name, "url": self.url(name), "size": data.len(), "sha256": sha(&data), "kind": "zip"});
        self.srv.put(name, data);
        f
    }

    fn add_pack(&mut self, id: &str, files: &[(&str, Vec<u8>)]) {
        let files: Vec<(&str, Vec<u8>)> = files.iter().filter(|(n, _)| *n != "map-set.json").cloned().collect();
        let zip = self.dir.join(format!("Lastbook-{id}.zip"));
        let installed = make_zip(&zip, &files, true);
        let f = self.put(&format!("Lastbook-{id}.zip"), std::fs::read(&zip).unwrap());
        self.manifest["packs"].as_array_mut().unwrap().push(json!({"id": id, "title": id, "available": true, "installed_bytes": installed, "files": [f]}));
    }

    fn add_map(&mut self, id: &str) -> Map<String, Value> {
        let mut files: Vec<(String, Vec<u8>)> = vec![("derived/geo/national.sqlite".into(), noise(300_000, 3)), ("derived/geo/terrain.sqlite".into(), noise(500_000, 4)),
                                                     ("derived/geo/national-status.json".into(), b"{\"state\":\"complete\"}".to_vec()),
                                                     ("library-data/geography/resources/atlas.sqlite".into(), noise(100_000, 5))];
        let sizes: Map<String, Value> = files.iter().map(|(n, d)| (n.clone(), json!(d.len()))).collect();
        files.push(("map-set.json".into(), serde_json::to_vec(&json!({"name": id, "files": sizes})).unwrap()));
        let zip = self.dir.join(format!("Lastbook-Map-{id}.zip"));
        let refs: Vec<(&str, Vec<u8>)> = files.iter().map(|(n, d)| (n.as_str(), d.clone())).collect();
        let installed = make_zip(&zip, &refs, false);      // the real region zips hold derived/geo/... at the top
        let f = self.put(&format!("Lastbook-Map-{id}.zip"), std::fs::read(&zip).unwrap());
        self.manifest["maps"].as_array_mut().unwrap().push(json!({"id": id, "title": "Bremen", "kind": "region", "includes": "x", "installed_bytes": installed, "available": true, "files": [f]}));
        sizes
    }

    fn add_addresses(&mut self, id: &str) -> Vec<u8> {
        let payload = noise(80_000, 6);
        let zip = self.dir.join(format!("Lastbook-Addresses-{id}.zip"));
        make_zip(&zip, &[(&format!("derived/geo/addresses/{id}.sqlite"), payload.clone())], false);
        let f = self.put(&format!("Lastbook-Addresses-{id}.zip"), std::fs::read(&zip).unwrap());
        self.manifest["address_indexes"].as_array_mut().unwrap().push(json!({"id": id, "title": "Bremen", "available": true, "installed_bytes": payload.len(), "files": [f]}));
        payload
    }

    fn add_model(&mut self, id: &str, size: usize, memory: u64) -> Vec<u8> {
        let data = noise(size, 7);
        let file = format!("{id}.gguf");
        self.srv.put(&file, data.clone());
        let entry = json!({"id": id, "title": id, "file": file, "url": self.url(&file), "size": size, "sha256": sha(&data), "memory_bytes": memory, "available": true});
        self.manifest["models"].as_array_mut().unwrap().push(entry);
        data
    }

    fn setup(&self, mode: Mode, current: Option<PathBuf>, ai: Option<Arc<dyn AiHooks>>) -> Setup {
        self.setup_with(mode, current, ai, None)
    }

    fn setup_with(&self, mode: Mode, current: Option<PathBuf>, ai: Option<Arc<dyn AiHooks>>, models_here: Option<PathBuf>) -> Setup {
        let mut o = self.options(mode, current);
        o.models_here = models_here;
        o.ai = ai;
        Setup::new(o)
    }

    /// The options every test starts from: this test's server and manifest, short waits, a disk of our own.
    fn options(&self, mode: Mode, current: Option<PathBuf>) -> Options {
        let m = manifest::validate(&self.manifest, &self.srv.allow()).expect("manifest");
        let mut o = Options::new(mode, self.settings.clone(), self.parent.clone(), current);
        o.manifest = Manifest::new(m, None, self.srv.allow(), Some(self.srv.tls.clone()));
        o.allow = self.srv.allow();
        o.tls = Some(self.srv.tls.clone());
        o.backoff = Some(vec![0.05; 6]);
        o.windows_paths = false;
        o.disk_space = Some(Arc::new(|_: &Path| (500_000_000_000, 1_000_000_000_000))); // a disk of our own
        o
    }

    /// As setup, on a disk with this much room.
    fn setup_on_disk(&self, mode: Mode, current: Option<PathBuf>, free: u64, total: u64) -> Setup {
        let mut o = self.options(mode, current);
        o.disk_space = Some(Arc::new(move |_: &Path| (free, total)));
        Setup::new(o)
    }

    fn library(&self, name: &str) -> PathBuf {
        let lib = self.parent.join(name);
        std::fs::create_dir_all(lib.join("library-data")).unwrap();
        std::fs::write(lib.join("library-data/catalog.json"), b"{}").unwrap();
        lib
    }
}

fn wait(s: &Setup, states: &[&str]) -> Value {
    let end = Instant::now() + Duration::from_secs(30);
    while Instant::now() < end {
        let j = s.public_job();
        if states.contains(&j["state"].as_str().unwrap_or("")) {
            return j;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("job never reached {states:?}: {}", s.public_job());
}

fn kinds(j: &Value) -> Vec<String> {
    j["items"].as_array().unwrap().iter().map(|i| i["kind"].as_str().unwrap().to_string()).collect()
}

#[test]
fn the_smallest_library_is_suggested_whatever_its_name() {
    let mut env = Env::new("smallest");
    env.add_pack("large", &[("library-data/catalog.json", b"{\"records\": []}".to_vec()), ("pack.json", b"{\"name\": \"large\", \"map\": \"base\"}".to_vec()),
                            ("library-data/big.bin", noise(6_000_000, 7))]);
    let packs = env.manifest["packs"].as_array_mut().unwrap();
    packs.reverse(); // the larger one listed first
    packs[1]["id"] = json!("starter");
    packs[1]["title"] = json!("Starter library");
    let st = env.setup(Mode::FirstRun, None, None).state();
    assert_eq!(st["recommend"]["pack"], "starter");
    assert!(st["recommend"]["why"].as_str().unwrap().starts_with("Starter library is selected"), "{}", st["recommend"]["why"]);
}

#[test]
fn first_run_installs_and_switches_at_restart() {
    let env = Env::new("first");
    let s = env.setup(Mode::FirstRun, None, None);
    let st = s.state();
    assert_eq!(st["mode"], "first-run");
    assert_eq!(st["recommend"]["pack"], "small");
    assert!(st["manifest"]["packs"][0]["files"][0].get("url").is_none());
    let j = s.start(Some("small"), None, None, Map::new(), false, false, None, None);
    assert_eq!(kinds(&j), ["pack"]);
    assert_eq!(wait(&s, &["done", "failed"])["state"], "done");
    assert_eq!(s.finish()["restart"], true);
    let (lib, note) = startup(&env.settings, None);
    let lib = lib.expect("library");
    assert_eq!(lib, env.parent.join(DATA_NAME));
    assert!(install::is_library(&lib));
    assert_eq!(std::fs::read(lib.join("library-data/big.bin")).unwrap(), noise(3_000_000, 1));
    opened(&env.settings, note.as_ref(), None);
    assert!(!env.parent.join("LastbookData.setup").exists());
    // the same library again in manage mode: nothing to add
    let m = env.setup(Mode::Manage, Some(lib.clone()), None);
    let r = m.start(Some("small"), None, None, Map::new(), false, false, None, None);
    assert_eq!((r["code"].as_str(), &r["params"], r["error"].as_str()), (Some("nothing_to_add"), &json!({}), Some("Nothing to add: this library and the local AI are already here.")));
}

#[test]
fn pack_with_region_map_and_addresses() {
    let mut env = Env::new("map");
    let sizes = env.add_map("DE-HB");
    let payload = env.add_addresses("DE-HB");
    let s = env.setup(Mode::FirstRun, None, None);
    let j = s.start(Some("small"), None, None, Map::new(), false, false, Some("DE-HB"), None);
    assert_eq!(kinds(&j), ["pack", "map", "addresses"]);
    assert_eq!(wait(&s, &["done", "failed"])["state"], "done", "{}", s.public_job());
    let (lib, _) = startup(&env.settings, None);
    let lib = lib.unwrap();
    assert_eq!(install::map_name(&lib), "DE-HB");
    assert!(!lib.join("derived/geo/map.sqlite").exists(), "the pack's own map set went");
    for (rel, size) in &sizes {
        assert_eq!(std::fs::metadata(lib.join(rel)).unwrap().len(), size.as_u64().unwrap(), "{rel}");
    }
    assert_eq!(std::fs::read(lib.join("derived/geo/addresses/DE-HB.sqlite")).unwrap(), payload);
    let m = env.setup(Mode::Manage, Some(lib.clone()), None);
    assert_eq!(m.current_info()["map"], "DE-HB");
    assert_eq!(m.current_info()["address_indexes"], json!(["DE-HB"]));
    assert_eq!(m.start(None, None, None, Map::new(), false, false, Some("DE-HB"), None)["code"], "nothing_to_add");
}

#[test]
fn a_map_that_names_another_region_is_refused_with_both_names() {
    let mut env = Env::new("map-mismatch");
    env.add_map("LU");
    env.manifest["maps"][0]["id"] = json!("LI");      // the zip's map-set.json says LU
    let s = env.setup(Mode::FirstRun, None, None);
    s.start(Some("small"), None, None, Map::new(), false, false, Some("LI"), None);
    let j = wait(&s, &["done", "failed"]);
    assert_eq!(j["state"], "failed");
    assert_eq!((j["error"]["code"].as_str(), j["error"]["message"].as_str()), (Some("unpack"), Some("The map file says it is LU, not LI.")));
    assert_eq!(j["error"]["params"], json!({"found": "LU", "expected": "LI"}));
    assert_eq!(j["items"][1]["error"], j["error"], "the item that stopped says the same");
}

#[test]
fn map_only_in_manage_mode_swaps_at_restart() {
    let mut env = Env::new("manage-map");
    env.add_map("LU");
    let cur = env.library(DATA_NAME);
    std::fs::write(cur.join("pack.json"), b"{\"name\": \"small\", \"map\": \"base\"}").unwrap();
    std::fs::create_dir_all(cur.join("derived/geo")).unwrap();
    for n in ["map.sqlite", "water.sqlite", "water.sqlite-wal", "national.sqlite"] {
        std::fs::write(cur.join("derived/geo").join(n), format!("old {n}")).unwrap();
    }
    let s = env.setup(Mode::Manage, Some(cur.clone()), None);
    assert_eq!(s.current_info()["map"], "base");
    let j = s.start(None, None, None, Map::new(), false, false, Some("LU"), None);
    assert_eq!(kinds(&j), ["map"]);
    assert_eq!(wait(&s, &["done", "failed"])["state"], "done", "{}", s.public_job());
    assert_eq!(std::fs::read(cur.join("derived/geo/water.sqlite")).unwrap(), b"old water.sqlite");   // untouched until the restart
    let (lib, note) = startup(&env.settings, None);
    let note = note.unwrap();
    assert!(note.get("error").is_none(), "{note:?}");
    assert_eq!(install::map_name(&lib.unwrap()), "LU");
    let mut geo: Vec<String> = std::fs::read_dir(cur.join("derived/geo")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    geo.sort();
    assert_eq!(geo, ["national-status.json", "national.sqlite", "terrain.sqlite"]);     // the old set, -wal too, went
    assert!(Path::new(note["map_old"].as_str().unwrap()).join("derived/geo/water.sqlite-wal").is_file());
}

#[test]
fn a_file_already_here_is_used_or_refused() {
    let env = Env::new("found");
    let data = env.srv.files.lock().unwrap().data["Lastbook-small.zip"].clone();
    std::fs::write(env.parent.join("Lastbook-small.zip"), &data).unwrap();
    let s = env.setup(Mode::FirstRun, None, None);
    let found = s.found();
    assert!(found.iter().any(|f| f["name"] == "Lastbook-small.zip" && f["size_ok"] == true));
    let j = s.start(Some("small"), None, None, Map::new(), false, false, None, None);
    assert_eq!(j["items"][0]["source"], "found");
    assert_eq!(wait(&s, &["done", "failed"])["state"], "done");
    assert!(env.srv.files.lock().unwrap().log.is_empty(), "nothing was downloaded");
    assert!(env.parent.join("Lastbook-small.zip").is_file(), "the person's own file stays");
    // a file of the right size and wrong bytes, chosen by the person: refused, left alone, not offered again
    let env = Env::new("found-bad");
    let mut bad = env.srv.files.lock().unwrap().data["Lastbook-small.zip"].clone();
    let mid = bad.len() / 2;
    bad[mid] ^= 0xff;
    let own = env.dir.join("mine").join("Lastbook-small.zip");
    std::fs::create_dir_all(own.parent().unwrap()).unwrap();
    std::fs::write(&own, &bad).unwrap();
    let s = env.setup(Mode::FirstRun, None, None);
    s.use_path(&own.to_string_lossy());
    let j = wait(&s, &["done", "failed"]);
    assert_eq!(j["error"]["code"], "found_mismatch");
    assert_eq!(j["error"]["params"], json!({"name": "Lastbook-small.zip", "folder": "mine"}));
    assert_eq!(j["error"]["file"], json!(own.to_string_lossy()));
    assert_eq!(std::fs::read(&own).unwrap(), bad);
    assert!(s.found().iter().all(|f| f["path"] != json!(own.to_string_lossy()) || f["rejected"] == true));
    s.retry();
    assert_eq!(wait(&s, &["done", "failed"])["state"], "done");     // downloaded instead
}

#[test]
fn pause_resume_cancel_and_space() {
    let env = Env::new("pause");
    env.srv.files.lock().unwrap().cut_after.insert("Lastbook-small.zip".into(), vec![1_000_000]);
    let s = env.setup(Mode::FirstRun, None, None);
    let mut slow = s.clone();
    let _ = &mut slow;
    s.start(Some("small"), None, None, Map::new(), false, false, None, None);
    std::thread::sleep(Duration::from_millis(30));
    let j = s.pause();
    if j["state"] == "paused" {
        let r = s.resume();     // waiting, or already further: the job's thread may have moved on before this answer
        assert!(ACTIVE.contains(&r["state"].as_str().unwrap_or("")) || r["state"] == "done", "{r}");
    }
    assert_eq!(wait(&s, &["done", "failed"])["state"], "done");
    // cancel a fresh one: nothing kept, nothing switched
    let env2 = Env::new("cancel");
    env2.srv.files.lock().unwrap().cut_after.insert("Lastbook-small.zip".into(), vec![500_000; 3]);
    let s2 = env2.setup(Mode::FirstRun, None, None);
    s2.start(Some("small"), None, None, Map::new(), false, false, None, None);
    std::thread::sleep(Duration::from_millis(50));
    let j = s2.cancel();
    assert_eq!(j["error"], json!({"code": "cancelled", "message": "Cancelled. Nothing was changed.", "params": {}}));
    assert!(!env2.parent.join("LastbookData.setup/downloads").exists());
    assert!(startup(&env2.settings, None).0.is_none());
    // a choice that cannot fit: the numbers come back, nothing starts
    let mut env3 = Env::new("space");
    env3.manifest["packs"][0]["installed_bytes"] = json!(u64::MAX / 4);
    let s3 = env3.setup(Mode::FirstRun, None, None);
    let r = s3.start(Some("small"), None, None, Map::new(), false, false, None, None);
    assert_eq!(r["code"], "no_space");
    assert!(r["need_bytes"].as_u64().unwrap() > r["free_bytes"].as_u64().unwrap());
    assert_eq!(r["params"], json!({"need_bytes": r["need_bytes"], "free_bytes": 500_000_000_000u64}));
    assert!(r["error"].as_str().unwrap().starts_with("This needs "));
    assert_eq!(s3.handle("POST", "start", &json!({"pack": "small"})).0, 507);
}

#[test]
fn a_job_says_its_failures_with_params() {
    // room at the start, and the disk filled up before the download: the file, and the numbers
    let env = Env::new("dl-space");
    let mut o = env.options(Mode::FirstRun, None);
    let looked = Arc::new(Mutex::new(0u32));
    o.disk_space = Some(Arc::new(move |_: &Path| {
        let mut n = looked.lock().unwrap();
        *n += 1;
        if *n == 1 { (500_000_000_000, 1_000_000_000_000) } else { (1000, 1_000_000_000_000) }
    }));
    let s = Setup::new(o);
    s.start(Some("small"), None, None, Map::new(), false, false, None, None);
    let j = wait(&s, &["done", "failed"]);
    let size = env.srv.files.lock().unwrap().data["Lastbook-small.zip"].len() as u64;
    assert_eq!((j["error"]["code"].as_str(), &j["error"]["params"]), (Some("no_space"), &json!({"name": "Lastbook-small.zip", "need_bytes": size, "free_bytes": 1000})), "{j}");
    assert_eq!((j["error"]["need_bytes"].as_u64(), j["error"]["free_bytes"].as_u64()), (Some(size), Some(1000)), "as before, beside the params");
    assert!(j["error"]["message"].as_str().unwrap().starts_with("Downloading Lastbook-small.zip needs "));
    // damaged twice on the server: the file's name
    let env = Env::new("dl-checksum");
    env.srv.files.lock().unwrap().corrupt.insert("Lastbook-small.zip".into(), 2);
    let s = env.setup(Mode::FirstRun, None, None);
    s.start(Some("small"), None, None, Map::new(), false, false, None, None);
    let j = wait(&s, &["done", "failed"]);
    assert_eq!((j["error"]["code"].as_str(), &j["error"]["params"]), (Some("checksum"), &json!({"name": "Lastbook-small.zip"})));
    assert!(j["error"]["message"].as_str().unwrap().starts_with("Lastbook-small.zip did not match its published checksum twice."));
    // not on the server: its answer's status
    let env = Env::new("dl-404");
    env.srv.files.lock().unwrap().data.remove("Lastbook-small.zip");
    let s = env.setup(Mode::FirstRun, None, None);
    s.start(Some("small"), None, None, Map::new(), false, false, None, None);
    let j = wait(&s, &["done", "failed"]);
    assert_eq!((j["error"]["code"].as_str(), &j["error"]["params"]), (Some("network"), &json!({"status": 404})));
    assert_eq!(j["error"]["message"], "The download server refused the file (HTTP 404).");
    // a sentence with nothing in it to name: empty params, never none
    let mut env = Env::new("dl-revision");
    env.manifest["packs"][0]["revision"] = json!("r2");     // the pack's own pack.json names no revision
    let s = env.setup(Mode::FirstRun, None, None);
    s.start(Some("small"), None, None, Map::new(), false, false, None, None);
    let j = wait(&s, &["done", "failed"]);
    assert_eq!(j["error"], json!({"code": "unpack", "message": "The library content version does not match this download.", "params": {}}));
}

#[test]
fn a_saved_job_comes_back_paused() {
    let env = Env::new("restart");
    env.srv.files.lock().unwrap().cut_after.insert("Lastbook-small.zip".into(), vec![400_000; 20]);
    let s = env.setup(Mode::FirstRun, None, None);
    s.start(Some("small"), None, None, Map::new(), false, false, None, None);
    std::thread::sleep(Duration::from_millis(80));
    s.pause();
    let again = env.setup(Mode::FirstRun, None, None);       // as after a restart
    let j = again.public_job();
    assert_eq!(j["state"], "paused", "{j}");
    env.srv.files.lock().unwrap().cut_after.clear();
    again.resume();
    assert_eq!(wait(&again, &["done", "failed"])["state"], "done");
    // a job file naming a folder setup would not make is not obeyed
    let f = job_file(&env.settings);
    let mut bad = install::read_json(&f).unwrap();
    bad["plan"]["target"] = json!("/etc");
    bad["plan"]["parent"] = json!("/");
    install::write_json(&f, &bad).unwrap();
    assert!(env.setup(Mode::FirstRun, None, None).public_job().is_null());
}

#[derive(Default)]
struct FakeAi {
    checked: Mutex<Vec<(PathBuf, String)>>,
    stopped: Mutex<u32>,
}

impl AiHooks for FakeAi {
    fn measure(&self, _model: &Value, _path: &Path) -> Option<Value> {
        Some(json!({"fits": true, "needs_bytes": 1, "estimate": false, "why": "Measured on this computer: it fits."}))
    }
    fn mark_checked(&self, path: &Path, sha256: &str) {
        self.checked.lock().unwrap().push((path.to_path_buf(), sha256.to_string()));
    }
    fn stop(&self, _why: &str) {
        *self.stopped.lock().unwrap() += 1;
    }
    fn rediscover(&self) {}
}

#[test]
fn models_are_chosen_downloaded_and_removed() {
    let mut env = Env::new("model");
    let small = env.add_model("gemma-small", 300_000, 1);                 // fits anywhere
    env.add_model("gemma-huge", 400_000, 1 << 50);                         // fits nowhere
    let lib = env.library(DATA_NAME);
    std::fs::write(lib.join("pack.json"), b"{\"name\": \"small\"}").unwrap();
    let ai = Arc::new(FakeAi::default());
    let s = env.setup(Mode::Manage, Some(lib.clone()), Some(ai.clone()));
    let hw = s.state()["hardware"].clone();
    assert_eq!(hw["ai"]["model"], "gemma-small");
    assert_eq!(hw["ai_models"]["gemma-huge"]["fits"], false);
    let (status, j) = s.handle("POST", "start", &json!({"ai": true}));        // the window's older form: the suggested model
    assert_eq!(status, 200, "{j}");
    assert_eq!(j["items"][0]["id"], "gemma-small");
    assert_eq!(wait(&s, &["done", "failed"])["state"], "done", "{}", s.public_job());
    assert_eq!(std::fs::read(lib.join("models/gemma-small.gguf")).unwrap(), small);
    assert_eq!(ai.checked.lock().unwrap()[0].1, sha(&small));
    assert_eq!(s.current_info()["models"], json!(["gemma-small"]));
    let (status, r) = s.handle("POST", "remove-ai", &json!({}));
    assert_eq!((status, r["freed_bytes"].as_u64()), (200, Some(300_000)));
    assert!(!lib.join("models/gemma-small.gguf").exists());
    assert_eq!(*ai.stopped.lock().unwrap(), 1);
    let (status, r) = s.handle("POST", "remove-ai", &json!({}));
    assert_eq!((status, r["code"].as_str(), &r["params"]), (400, Some("no_ai_to_remove"), &json!({})));
    let (status, r) = s.handle("POST", "start", &json!({"model": "nope"}));
    assert_eq!((status, r["code"].as_str(), &r["params"]), (400, Some("model_unavailable"), &json!({})));
    assert_eq!(r["error"], "That local AI is not offered for this version.");
}

/// The AI's own measure, as the AI gives it.
struct Measured(Value);

impl AiHooks for Measured {
    fn measure(&self, _model: &Value, _path: &Path) -> Option<Value> {
        Some(self.0.clone())
    }
    fn mark_checked(&self, _path: &Path, _sha256: &str) {}
    fn stop(&self, _why: &str) {}
    fn rediscover(&self) {}
}

#[test]
fn the_ai_says_why_it_fits_or_not() {
    // no model offered
    let env = Env::new("ai-none");
    let hw = env.setup(Mode::FirstRun, None, None).hardware();
    assert_eq!((hw["ai"]["why"].as_str(), &hw["ai"]["why_codes"]), (Some("No local AI is offered for this version."), &json!([{"code": "none_offered", "params": {}}])));
    // an estimate from this computer's memory: the reason and the numbers in it
    let mut env = Env::new("ai-estimate");
    env.add_model("gemma-huge", 1000, 1 << 50);
    let hw = env.setup(Mode::FirstRun, None, None).hardware();
    let ai = &hw["ai"];
    assert_eq!((ai["fits"].as_bool(), ai["model"].as_str()), (Some(false), Some("gemma-huge")));
    let reason = &ai["why_codes"][0];
    match hw["ram_bytes"].as_u64() {
        Some(ram) => {
            assert_eq!(reason["code"], "too_big");
            assert_eq!(reason["params"]["ram_bytes"].as_u64(), Some(ram));
            assert!(reason["params"]["need_bytes"].as_u64().unwrap() > 1 << 49, "what the graphics card cannot hold");
        }
        None => assert_eq!(reason["code"], "ram_unknown"),
    }
    assert_eq!(hw["ai_models"]["gemma-huge"]["why_codes"], ai["why_codes"]);
    // the AI's own measure of a model file here: setup's reason for each verdict, the AI's English beside it
    for (verdict, code) in [(json!({"fits": true}), "measured_fits"), (json!({"fits": false, "from_disk": true}), "measured_from_disk"),
                            (json!({"fits": false, "from_disk": false}), "measured_too_big")] {
        let mut env = Env::new(&format!("ai-{code}"));
        let data = env.add_model("gemma-small", 2000, 1);
        let lib = env.library(DATA_NAME);
        std::fs::create_dir_all(lib.join("models")).unwrap();
        std::fs::write(lib.join("models/gemma-small.gguf"), &data).unwrap();
        let mut m = verdict.clone();
        m["needs_bytes"] = json!(3000);
        m["estimate"] = json!(false);
        m["why"] = json!("The AI's own sentence.");
        let s = env.setup(Mode::Manage, Some(lib), Some(Arc::new(Measured(m))));
        let end = Instant::now() + Duration::from_secs(10);
        let fit = loop {
            let f = s.hardware()["ai_models"]["gemma-small"].clone();
            if f["estimate"] == false || Instant::now() > end {
                break f;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(fit["estimate"], false, "measured");
        assert_eq!((fit["why"].as_str(), &fit["why_codes"]), (Some("The AI's own sentence."), &json!([{"code": code, "params": {}}])), "{verdict}");
    }
}

#[test]
fn recommend_says_why_as_codes() {
    // nothing published yet
    let mut env = Env::new("why-none");
    env.manifest["packs"] = json!([]);
    let r = env.setup(Mode::FirstRun, None, None).recommend(&json!({}));
    assert_eq!((r["why"].as_str(), &r["why_codes"]), (Some("No library is ready to download yet."), &json!([{"code": "none_ready", "params": {}}])));
    // room for it: start small
    let env = Env::new("why-small");
    let s = env.setup(Mode::FirstRun, None, None);
    let r = s.recommend(&json!({}));
    assert_eq!(r["why"], "Small is selected to get started. You can choose a larger library.");
    assert_eq!(r["why_codes"], json!([{"code": "start_small", "params": {"pack": "small"}}]));
    assert_eq!(r["model_place"], "library");
    let need = r["need_bytes"]["small"].as_u64().unwrap();
    // no room anywhere: the smallest, with the numbers
    let s = env.setup_on_disk(Mode::FirstRun, None, 1_000_000, 100_000_000_000);
    let r = s.recommend(&json!({"disks": []}));
    assert_eq!(r["pack"], Value::Null);
    assert_eq!(r["why_codes"], json!([{"code": "none_fits", "params": {"pack": "small", "need_bytes": need, "free_bytes": 1_000_000}}]));
    assert!(r["why"].as_str().unwrap().starts_with("The smallest library, small, needs "));
    // room on another disk (/dev is on another device than the temporary folder on every system the tests run on)
    let stick = json!({"path": "/dev", "name": "STICK", "free_bytes": 900_000_000_000u64, "total_bytes": 1_000_000_000_000u64, "kind": "removable"});
    let r = s.recommend(&json!({"disks": [stick]}));
    assert_eq!((r["pack"].as_str(), r["folder"].as_str()), (Some("small"), Some("/dev")));
    assert_eq!(r["why_codes"], json!([{"code": "other_disk", "params": {"free_bytes": 1_000_000, "pack": "small", "disk": "STICK"}}]));
    assert_eq!(r["why"], "This computer’s own disk has 0.0 GB free, not enough for any library, so small goes on STICK. Keep that disk connected: Lastbook reads the library from it.");
    let mut nas = stick.clone();
    nas["kind"] = json!("network");
    nas["name"] = Value::Null;      // no name: its path names it
    let r = s.recommend(&json!({"disks": [nas]}));
    assert_eq!(r["why_codes"], json!([{"code": "other_disk_network", "params": {"free_bytes": 1_000_000, "pack": "small", "disk": "/dev"}}]));
    assert!(r["why"].as_str().unwrap().contains("so small goes on /dev, a network share. Keep this computer on that network"));
    // the app's note on its default folder comes first
    let mut o = env.options(Mode::FirstRun, None);
    o.default_why = Some("Saves to C:\\Lastbook Europe because your user folder’s path is too long for the library’s longest file names.".into());
    let r = Setup::new(o).recommend(&json!({}));
    assert_eq!(r["why_codes"], json!([{"code": "default_folder", "params": {"folder": env.parent.to_string_lossy()}}, {"code": "start_small", "params": {"pack": "small"}}]));
    assert!(r["why"].as_str().unwrap().starts_with("Saves to C:\\Lastbook Europe because"));
    // the local AI fits the memory but not the disk as well
    let mut env = Env::new("why-ai");
    env.add_model("gemma-small", 300_000, 1);
    let need = env.setup(Mode::FirstRun, None, None).recommend(&json!({}))["need_bytes"]["small"].as_u64().unwrap();
    let s = env.setup_on_disk(Mode::FirstRun, None, need + 5_000_000_000 + 100_000, 100_000_000_000);
    let r = s.recommend(&json!({"ai_models": {"gemma-small": {"fits": true}}}));
    assert_eq!((r["pack"].as_str(), r["ai"].as_bool()), (Some("small"), Some(false)));
    assert_eq!(r["why_codes"], json!([{"code": "start_small", "params": {"pack": "small"}}, {"code": "ai_no_room", "params": {}}]));
    assert!(r["why"].as_str().unwrap().ends_with(" The local AI fits this computer’s memory, but not on the disk as well."));
    // no model fits this computer: the AI's own reasons follow
    let too_big = json!({"fits": false, "why": "The local AI needs about 22 GB of this computer’s memory, and it has 17 GB. Lastbook works without it: search and reading are the same.",
                         "why_codes": [{"code": "too_big", "params": {"need_bytes": 21_711_738_816u64, "ram_bytes": 17_179_869_184u64}}]});
    let r = s.recommend(&json!({"ai_models": {"gemma-small": {"fits": false}}, "ai": too_big}));
    assert_eq!(r["why_codes"], json!([{"code": "start_small", "params": {"pack": "small"}}, too_big["why_codes"][0]]));
    assert!(r["why"].as_str().unwrap().ends_with(too_big["why"].as_str().unwrap()));
}

#[test]
fn a_model_can_live_on_this_computer_while_the_library_is_elsewhere() {
    let mut env = Env::new("model-here");
    let small = env.add_model("gemma-small", 200_000, 1);
    let lib = env.library(DATA_NAME);
    std::fs::write(lib.join("pack.json"), b"{\"name\": \"small\"}").unwrap();
    let here = env.dir.join("this-computer").join("models");
    let ai = Arc::new(FakeAi::default());
    let s = env.setup_with(Mode::Manage, Some(lib.clone()), Some(ai.clone()), Some(here.clone()));
    // the library is on this computer's own disk in the test: the default is beside it
    assert_eq!(s.default_model_place(&lib), "library");
    let (status, j) = s.handle("POST", "start", &json!({"model": "gemma-small", "model_place": "computer"}));
    assert_eq!(status, 200, "{j}");
    assert_eq!(wait(&s, &["done", "failed"])["state"], "done", "{}", s.public_job());
    assert_eq!(std::fs::read(here.join("gemma-small.gguf")).unwrap(), small, "on this computer");
    assert!(!lib.join("models/gemma-small.gguf").exists(), "not beside the library");
    assert_eq!(s.current_info()["models"], json!(["gemma-small"]), "found in either place");
    assert_eq!(ai.checked.lock().unwrap()[0].0, here.join("gemma-small.gguf"));
    assert_eq!(s.current_info()["disk_kind"], "local");
    assert_eq!(s.state()["recommend"]["model_place"], "library", "the default the window starts from");
    // removed from where it is
    let (status, r) = s.handle("POST", "remove-ai", &json!({}));
    assert_eq!((status, r["freed_bytes"].as_u64()), (200, Some(200_000)), "{r}");
    assert!(!here.join("gemma-small.gguf").exists());
    assert_eq!(s.current_info()["models"], json!([]));
}

#[test]
fn repair_checks_then_fetches_what_is_wrong() {
    let env = Env::new("repair");
    let s = env.setup(Mode::FirstRun, None, None);
    s.start(Some("small"), None, None, Map::new(), false, false, None, None);
    assert_eq!(wait(&s, &["done", "failed"])["state"], "done");
    let (lib, note) = startup(&env.settings, None);
    let lib = lib.unwrap();
    opened(&env.settings, note.as_ref(), None);
    // a pack-manifest.tsv as the pack builder writes it
    let mut rows = String::from("path\tbytes\tsha256\n");
    for rel in ["library-data/catalog.json", "library-data/big.bin", "pack.json"] {
        let b = std::fs::read(lib.join(rel)).unwrap();
        rows.push_str(&format!("{rel}\t{}\t{}\n", b.len(), sha(&b)));
    }
    std::fs::write(lib.join("pack-manifest.tsv"), rows).unwrap();
    let m = env.setup(Mode::Manage, Some(lib.clone()), None);
    let before = env.srv.files.lock().unwrap().log.len();
    m.start(None, None, None, Map::new(), false, true, None, None);
    assert_eq!(wait(&m, &["done", "failed"])["state"], "done");
    assert_eq!(env.srv.files.lock().unwrap().log.len(), before, "all well: nothing downloaded");
    let mut b = std::fs::read(lib.join("library-data/big.bin")).unwrap();
    b[10] ^= 1;
    std::fs::write(lib.join("library-data/big.bin"), b).unwrap();
    // no room to fetch the pack again: the numbers come with the code
    let m = env.setup_on_disk(Mode::Manage, Some(lib.clone()), 1000, 100_000_000_000);
    m.start(None, None, None, Map::new(), false, true, None, None);
    let j = wait(&m, &["done", "failed"]);
    let zip = env.srv.files.lock().unwrap().data["Lastbook-small.zip"].len() as u64;
    assert_eq!((j["error"]["code"].as_str(), j["error"]["params"]["free_bytes"].as_u64()), (Some("no_space"), Some(1000)), "{j}");
    assert!(j["error"]["params"]["need_bytes"].as_u64().unwrap() > zip, "the download and its unpacked files: {j}");
    assert!(j["error"]["message"].as_str().unwrap().starts_with("Some files need fetching again."));
    let m = env.setup(Mode::Manage, Some(lib.clone()), None);
    m.start(None, None, None, Map::new(), false, true, None, None);
    assert_eq!(wait(&m, &["done", "failed"])["state"], "done");
    assert!(env.srv.files.lock().unwrap().log.len() > before, "the pack was fetched again");
    let (lib2, _) = startup(&env.settings, None);
    assert_eq!(std::fs::read(lib2.unwrap().join("library-data/big.bin")).unwrap(), noise(3_000_000, 1));
}

#[test]
fn routes_and_their_codes() {
    let env = Env::new("routes");
    let s = env.setup(Mode::FirstRun, None, None);
    assert_eq!(s.handle("GET", "nothing", &json!({})).0, 404);
    assert_eq!(s.handle("POST", "finish", &json!({})), (409, json!({"error": "Setup has not finished yet.", "code": "not_done", "params": {}})));
    assert_eq!(s.handle("POST", "start", &json!({})), (400, json!({"error": "Choose a library first.", "code": "choose_library", "params": {}})));
    assert_eq!(s.handle("POST", "start", &json!({"pack": "medium"})), (400, json!({"error": "That library is not available yet.", "code": "pack_unavailable", "params": {}})));
    assert_eq!(s.handle("POST", "start", &json!({"pack": "small", "map": "CA"})).1, json!({"error": "That region’s map is not available yet.", "code": "map_unavailable", "params": {}}));
    assert_eq!(s.handle("POST", "start", &json!({"pack": "small", "address_id": "CA"})).1,
               json!({"error": "That region’s address search is not available yet.", "code": "addresses_unavailable", "params": {}}));
    assert_eq!(s.handle("POST", "start", &json!({"ai": true})).1, json!({"error": "No local AI is offered for this version.", "code": "no_ai_offered", "params": {}}));
    assert_eq!(s.handle("POST", "start", &json!({"repair": true})).1, json!({"error": "There is no library to repair yet.", "code": "nothing_to_repair", "params": {}}));
    assert_eq!(s.handle("POST", "use", &json!({"path": "/nowhere/at/all"})), (400, json!({"error": "Lastbook cannot find that file or folder.", "code": "missing", "params": {}})));
    assert_eq!(s.handle("POST", "pick", &json!({})).1, json!({"path": null, "cancelled": true}));
    assert_eq!(s.handle("POST", "delete-old", &json!({"path": "/etc"})).1,
               json!({"error": "Lastbook only deletes an old library it moved aside itself.", "code": "not_moved_aside", "params": {}}));
    assert_eq!(s.handle("POST", "remove-ai", &json!({})).1, json!({"error": "There is no local AI in this library to remove.", "code": "no_ai_to_remove", "params": {}}));
    assert!(path_too_deep(Path::new(&format!("C:\\{}", "x".repeat(70))), true));
    assert!(!path_too_deep(Path::new("C:\\Users\\a\\Documents\\LastbookData"), true));
    // the picker's answer names the disk's kind
    let mut o = env.options(Mode::FirstRun, None);
    let place = env.dir.clone();
    o.pick_folder = Some(Box::new(move |_| Some(place.clone())));
    let r = Setup::new(o).handle("POST", "pick", &json!({})).1;
    assert_eq!((r["path"].as_str(), r["kind"].as_str(), r["free_bytes"].as_u64()), (Some(&*env.dir.to_string_lossy()), Some("local"), Some(500_000_000_000)));
}

/// An answer that refuses: (code, params), after checking that the English is there beside them.
fn said(r: &Value) -> (String, Value) {
    assert!(r["error"].as_str().is_some_and(|e| !e.is_empty()), "the English sentence: {r}");
    (r["code"].as_str().unwrap_or("").to_string(), r["params"].clone())
}

#[test]
fn every_refusal_says_its_code_and_params() {
    let mut env = Env::new("refusals");
    env.add_map("LU");
    let model = env.add_model("gemma-small", 5000, 1);
    env.add_pack("medium", &[("library-data/catalog.json", b"{}".to_vec())]);
    env.manifest["packs"][1]["available"] = json!(false);
    let s = env.setup(Mode::FirstRun, None, None);
    let c = |code: &str, params: Value| (code.to_string(), params);
    // where a new library would go
    let file = env.dir.join("a-file");
    std::fs::write(&file, b"x").unwrap();
    let r = s.start(Some("small"), None, Some(&file.join("inside").to_string_lossy()), Map::new(), false, false, None, None);
    assert_eq!(said(&r), c("cannot_make_folder", json!({})));
    assert!(r["error"].as_str().unwrap().starts_with("Lastbook cannot make a folder there ("), "the system's reason stays in the English");
    let taken = env.dir.join("taken");
    std::fs::create_dir_all(taken.join(DATA_NAME)).unwrap();
    std::fs::write(taken.join(DATA_NAME).join("notes.txt"), b"mine").unwrap();
    let r = s.start(Some("small"), None, Some(&taken.to_string_lossy()), Map::new(), false, false, None, None);
    assert_eq!(said(&r), c("not_library_there", json!({"name": "LastbookData"})));
    let mut o = env.options(Mode::FirstRun, None);
    o.windows_paths = true;
    let deep = env.dir.join("x".repeat(60));
    let r = Setup::new(o).start(Some("small"), None, Some(&deep.to_string_lossy()), Map::new(), false, false, None, None);
    let room = WINDOWS_MAX_PATH - LONGEST_INSIDE - 1 - DATA_NAME.len() - 1;
    assert_eq!(said(&r), c("too_deep", json!({"room": room})));
    assert!(r["error"].as_str().unwrap().contains(&format!("at most {room} characters long")));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let shut = env.dir.join("shut");
        std::fs::create_dir_all(&shut).unwrap();
        std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o555)).unwrap();
        if std::fs::write(shut.join("probe"), b"").is_err() {     // not when the tests run as root, who may save anywhere
            let r = s.start(Some("small"), None, Some(&shut.to_string_lossy()), Map::new(), false, false, None, None);
            assert_eq!(said(&r), c("cannot_save_there", json!({})));
        }
        std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    // what the person chose to use
    let plain = env.dir.join("Holiday photos");
    std::fs::create_dir_all(&plain).unwrap();
    let r = s.use_path(&plain.to_string_lossy());
    assert_eq!(said(&r), c("not_library", json!({"name": "Holiday photos"})));
    assert_eq!(r["error"], "“Holiday photos” is not a Lastbook library. Choose the folder called LastbookData.");
    let short = env.dir.join("Lastbook-small.zip");
    std::fs::write(&short, b"too short").unwrap();
    let published = env.srv.files.lock().unwrap().data["Lastbook-small.zip"].len() as u64;
    assert_eq!(said(&s.use_path(&short.to_string_lossy())), c("wrong_size", json!({"name": "Lastbook-small.zip", "size_bytes": 9, "expected_bytes": published})));
    let map = env.dir.join("Lastbook-Map-LU.zip");
    std::fs::write(&map, b"short").unwrap();
    let map_size = env.srv.files.lock().unwrap().data["Lastbook-Map-LU.zip"].len() as u64;
    assert_eq!(said(&s.use_path(&map.to_string_lossy())), c("wrong_size", json!({"name": "Lastbook-Map-LU.zip", "size_bytes": 5, "expected_bytes": map_size})));
    let gguf = env.dir.join("gemma-small.gguf");
    std::fs::write(&gguf, &model[..100]).unwrap();
    assert_eq!(said(&s.use_path(&gguf.to_string_lossy())), c("wrong_size_model", json!({"size_bytes": 100, "expected_bytes": 5000})));
    let later = env.dir.join("Lastbook-medium.zip");
    std::fs::write(&later, &env.srv.files.lock().unwrap().data["Lastbook-medium.zip"]).unwrap();
    assert_eq!(said(&s.use_path(&later.to_string_lossy())), c("pack_not_in_version", json!({})));
    let other = env.dir.join("notes.txt");
    std::fs::write(&other, b"hello").unwrap();
    assert_eq!(said(&s.use_path(&other.to_string_lossy())), c("unknown_file", json!({})));
    // the open library
    let lib = env.library(DATA_NAME);
    let m = env.setup(Mode::Manage, Some(lib.clone()), None);
    assert_eq!(said(&m.handle("POST", "start", &json!({"repair": true})).1), c("repair_unknown", json!({})), "no pack.json: which library is it?");
    let st = env.settings.join("setup.json");
    std::fs::create_dir_all(&env.settings).unwrap();
    let moved = env.parent.join("LastbookData-mine");
    install::write_json(&st, &json!({"old": [moved.to_string_lossy()]})).unwrap();
    assert_eq!(said(&m.handle("POST", "delete-old", &json!({"path": moved.to_string_lossy()})).1), c("not_old_library", json!({})));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(lib.join("models")).unwrap();
        std::fs::write(lib.join("models/gemma-small.gguf"), &model).unwrap();
        std::fs::set_permissions(lib.join("models"), std::fs::Permissions::from_mode(0o555)).unwrap();
        if std::fs::write(lib.join("models/probe"), b"").is_err() {
            assert_eq!(said(&m.handle("POST", "remove-ai", &json!({})).1), c("cannot_remove_model", json!({})));
            std::fs::set_permissions(&lib, std::fs::Permissions::from_mode(0o555)).unwrap();
            assert_eq!(said(&m.use_path(&lib.to_string_lossy())), c("library_read_only", json!({"name": "LastbookData"})));
            std::fs::set_permissions(&lib, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::set_permissions(lib.join("models"), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    // setup's own folder cannot hold its progress
    let blocked = Env::new("refusals-settings");
    std::fs::write(&blocked.settings, b"a file where the folder should be").unwrap();
    assert_eq!(said(&blocked.setup(Mode::FirstRun, None, None).start(Some("small"), None, None, Map::new(), false, false, None, None)), c("cannot_save_progress", json!({})));
}

#[test]
fn busy_is_said_by_its_code() {
    let env = Env::new("busy");
    env.srv.files.lock().unwrap().cut_after.insert("Lastbook-small.zip".into(), vec![300_000; 40]);
    let lib = env.library(DATA_NAME);
    std::fs::write(lib.join("pack.json"), b"{\"name\": \"small\"}").unwrap();
    let s = env.setup(Mode::Manage, Some(lib.clone()), None);
    let j = s.start(Some("small"), None, Some(&env.dir.join("elsewhere").to_string_lossy()), Map::new(), false, false, None, None);
    assert_eq!(j["state"], "waiting", "{j}");
    let busy = json!({"error": "A download is already under way. Pause or cancel it first.", "code": "busy", "params": {}});
    assert_eq!(s.handle("POST", "start", &json!({"pack": "small"})), (409, busy.clone()));
    assert_eq!(s.handle("POST", "start", &json!({"repair": true})), (409, busy));
    if s.public_job()["state"] != "paused" {
        let under_way = json!({"error": "A download is under way. Pause or cancel it first.", "code": "busy", "params": {}});
        let (status, r) = s.handle("POST", "use", &json!({"path": lib.to_string_lossy()}));
        assert!(r["state"] == "done" || (status, &r) == (409, &under_way), "{r}");     // done only if the download already finished
        assert!(matches!(s.handle("POST", "remove-ai", &json!({})).1["code"].as_str(), Some("busy" | "no_ai_to_remove")));
    }
    s.cancel();
}

#[test]
fn every_code_has_words_in_the_interface() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/locales/en.json");
    let words: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for code in CODES {
        assert!(words[format!("setup.code.{code}")].is_string(), "web/locales/en.json has no setup.code.{code}");
    }
    for code in WHY_CODES {
        assert!(words[format!("setup.rec.{code}")].is_string(), "web/locales/en.json has no setup.rec.{code}");
    }
    for (code, key) in JOB_CODES {
        assert!(words[key].is_string(), "web/locales/en.json has no {key}, the words for a job's {code}");
    }
    let script = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/setup.js")).unwrap();
    for (code, key) in JOB_CODES {
        assert!(script.contains(&format!("{code}: ")) && script.contains(&format!("'{key}")), "web/setup.js's errorBox says {code} with {key}");
    }
}
