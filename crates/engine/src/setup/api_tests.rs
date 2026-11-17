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
        let m = manifest::validate(&self.manifest, &self.srv.allow()).expect("manifest");
        let mut o = Options::new(mode, self.settings.clone(), self.parent.clone(), current);
        o.manifest = Manifest::new(m, None, self.srv.allow(), Some(self.srv.tls.clone()));
        o.allow = self.srv.allow();
        o.tls = Some(self.srv.tls.clone());
        o.backoff = Some(vec![0.05; 6]);
        o.windows_paths = false;
        o.disk_space = Some(Arc::new(|_: &Path| (500_000_000_000, 1_000_000_000_000))); // a disk of our own
        o.ai = ai;
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
    assert_eq!(m.start(Some("small"), None, None, Map::new(), false, false, None, None)["code"], "nothing");
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
    assert_eq!(m.start(None, None, None, Map::new(), false, false, Some("DE-HB"), None)["code"], "nothing");
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
        assert_eq!(s.resume()["state"], "waiting");
    }
    assert_eq!(wait(&s, &["done", "failed"])["state"], "done");
    // cancel a fresh one: nothing kept, nothing switched
    let env2 = Env::new("cancel");
    env2.srv.files.lock().unwrap().cut_after.insert("Lastbook-small.zip".into(), vec![500_000; 3]);
    let s2 = env2.setup(Mode::FirstRun, None, None);
    s2.start(Some("small"), None, None, Map::new(), false, false, None, None);
    std::thread::sleep(Duration::from_millis(50));
    let j = s2.cancel();
    assert_eq!(j["error"]["code"], "cancelled");
    assert!(!env2.parent.join("LastbookData.setup/downloads").exists());
    assert!(startup(&env2.settings, None).0.is_none());
    // a choice that cannot fit: the numbers come back, nothing starts
    let mut env3 = Env::new("space");
    env3.manifest["packs"][0]["installed_bytes"] = json!(u64::MAX / 4);
    let s3 = env3.setup(Mode::FirstRun, None, None);
    let r = s3.start(Some("small"), None, None, Map::new(), false, false, None, None);
    assert_eq!(r["code"], "no_space");
    assert!(r["need_bytes"].as_u64().unwrap() > r["free_bytes"].as_u64().unwrap());
    assert_eq!(s3.handle("POST", "start", &json!({"pack": "small"})).0, 507);
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
    assert_eq!(s.handle("POST", "start", &json!({"model": "nope"})).1["code"], "unavailable");
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
    assert_eq!(s.handle("POST", "finish", &json!({})), (409, json!({"error": "Setup has not finished yet.", "code": "not_done"})));
    assert_eq!(s.handle("POST", "start", &json!({})).1["code"], "nothing");
    assert_eq!(s.handle("POST", "start", &json!({"pack": "medium"})).1["code"], "unavailable");
    assert_eq!(s.handle("POST", "start", &json!({"pack": "small", "map": "CA"})).1["code"], "unavailable");
    assert_eq!(s.handle("POST", "use", &json!({"path": "/nowhere/at/all"})).1["code"], "missing");
    assert_eq!(s.handle("POST", "pick", &json!({})).1, json!({"path": null, "cancelled": true}));
    assert_eq!(s.handle("POST", "delete-old", &json!({"path": "/etc"})).1["code"], "unknown");
    assert!(path_too_deep(Path::new(&format!("C:\\{}", "x".repeat(70))), true));
    assert!(!path_too_deep(Path::new("C:\\Users\\a\\Documents\\LastbookData"), true));
}
