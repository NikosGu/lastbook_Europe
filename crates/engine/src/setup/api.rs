//! /api/setup/*: the engine side of first-run setup and of Library → Library and local AI. Contract: docs/SETUP-API.md.
//! Ported from the original Lastbook's engine/setup/api.py; the JSON the window receives is the same.
//!
//! A job is a list of items (pack, map, addresses, model) worked through on one thread: each file is used from this
//! computer when it is already here and its SHA-256 matches, else downloaded; then checked, unpacked and staged. A new
//! library is switched to when the app starts again (apply_pending), so nothing is renamed while it is open. The job
//! lives in the app's own settings folder, never in the work folder beside a library: what is in that folder is not
//! obeyed.

use super::download::{self, Control};
use super::hardware;
use super::install::{self, DATA_NAME};
use super::manifest::Manifest;
use super::net::{Allow, Tls};
use super::{gb, SetupError};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const WORK_SUFFIX: &str = ".setup";
const RECOMMEND_MARGIN: u64 = 5_000_000_000; // beyond what the install needs, so the disk is not left full
pub const ACTIVE: [&str; 5] = ["waiting", "downloading", "verifying", "unpacking", "activating"];
pub const WINDOWS_MAX_PATH: usize = 259;
pub const LONGEST_INSIDE: usize = 190; // the longest file path inside LastbookData in any pack (measured for Medium and All)

/// What setup's answers say when they refuse ({error, code, params}): one code per sentence, which the interface words in
/// the person's language (web/locales: setup.code.<code>), with its params (sizes in params named *_bytes, names as
/// strings); the English sentence stays beside them, for logs and for an interface that does not know the code. busy and
/// not_done are answered with 409, no_space with 507, the rest with 400.
pub const CODES: [&str; 28] = ["busy", "pack_unavailable", "no_ai_offered", "model_unavailable", "map_unavailable", "addresses_unavailable",
                               "choose_library", "nothing_to_add", "cannot_make_folder", "cannot_save_there", "too_deep", "not_library_there",
                               "no_space", "cannot_save_progress", "nothing_to_repair", "repair_unknown", "missing", "not_library",
                               "library_read_only", "wrong_size", "pack_not_in_version", "wrong_size_model", "unknown_file", "not_done",
                               "no_ai_to_remove", "cannot_remove_model", "not_moved_aside", "not_old_library"];

/// Why setup suggests what it does (recommend.why_codes, a list of {code, params} beside the English recommend.why), each
/// worded in web/locales as setup.rec.<code>. The local AI's own reasons (hardware::AI_CODES) can follow them.
pub const WHY_CODES: [&str; 7] = ["none_ready", "none_fits", "other_disk", "other_disk_network", "start_small", "default_folder", "ai_no_room"];

/// The codes of a job's error ({code, message, params}, on the job and on the item that stopped), with the key the
/// interface's error box (web/setup.js errorBox) says each one with. The params name what the sentence names.
pub const JOB_CODES: [(&str, &str); 8] = [("no_space", "setup.job.no_space"), ("network", "setup.job.network"), ("checksum", "setup.job.checksum"),
                                          ("found_mismatch", "setup.job.mismatch"), ("unpack", "setup.job.unpack"), ("activate", "setup.job.activate"),
                                          ("folder", "setup.job.folder"), ("cancelled", "setup.job.cancelled")];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    FirstRun,
    Manage,
}

impl Mode {
    fn as_str(self) -> &'static str {
        match self {
            Mode::FirstRun => "first-run",
            Mode::Manage => "manage",
        }
    }
}

/// What setup needs from the local AI, once it is there.
pub trait AiHooks: Send + Sync {
    /// The AI's own measure of a model file on this computer: {fits, needs_bytes, estimate: false, why}, or None.
    fn measure(&self, model: &Value, path: &Path) -> Option<Value>;
    /// Setup has just checked this file's SHA-256: the AI need not read it again.
    fn mark_checked(&self, path: &Path, sha256: &str);
    /// Stop the model process (before its file is removed).
    fn stop(&self, why: &str);
    /// Look for the model files again.
    fn rediscover(&self);
}

pub struct Options {
    pub mode: Mode,
    pub settings_dir: PathBuf,
    pub default_parent: PathBuf,
    pub current: Option<PathBuf>,
    pub manifest: Manifest,
    pub allow: Allow,
    pub tls: Option<Tls>,
    pub default_why: Option<String>,
    pub on_finish: Option<Box<dyn Fn(PathBuf) + Send + Sync>>,
    pub pick_folder: Option<Box<dyn Fn(bool) -> Option<PathBuf> + Send + Sync>>,
    pub look_for_libraries: Option<Box<dyn Fn() -> Vec<PathBuf> + Send + Sync>>,
    pub ai: Option<Arc<dyn AiHooks>>,
    /// Waits between network retries (tests make them short).
    pub backoff: Option<Vec<f64>>,
    /// Refuse a folder too deep for the library's longest file name on Windows (tests in Windows' long temporary folders turn it off).
    pub windows_paths: bool,
    /// (free, total) bytes of the disk holding a path; tests give a disk of their own, so the machine's free space does not matter.
    pub disk_space: Option<Arc<dyn Fn(&Path) -> (u64, u64) + Send + Sync>>,
    /// The library remembered from last time, when it cannot be reached (its disk unplugged, its network share away).
    pub missing: Option<PathBuf>,
    /// The app's own folder for AI models on this computer: a model can live there instead of beside the library, which
    /// is the better place when the library is on a network share or an external disk.
    pub models_here: Option<PathBuf>,
}

impl Options {
    pub fn new(mode: Mode, settings_dir: PathBuf, default_parent: PathBuf, current: Option<PathBuf>) -> Options {
        Options { mode, settings_dir, default_parent, current, manifest: Manifest::product(), allow: Allow::Product, tls: None, default_why: None,
                  on_finish: None, pick_folder: None, look_for_libraries: None, ai: None, backoff: None, windows_paths: cfg!(windows), disk_space: None, missing: None, models_here: None }
    }
}

struct Inner {
    o: Options,
    job: Mutex<Option<Value>>,
    control: Mutex<Option<Arc<Control>>>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    action: Mutex<()>,
    sources: Mutex<HashMap<String, String>>,
    rejected: Mutex<HashMap<String, (u64, i128)>>,
    ai_measure: Mutex<HashMap<String, Value>>,
    measuring: Mutex<HashSet<String>>,
    deleting: Mutex<HashSet<String>>,
}

#[derive(Clone)]
pub struct Setup(Arc<Inner>);

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

fn u(v: &Value) -> u64 {
    v.as_u64().unwrap_or(0)
}

/// An answer that refuses: the English sentence, its code (CODES) and the params the sentence has.
fn err(code: &str, params: Value, message: impl Into<String>) -> Value {
    json!({"error": message.into(), "code": code, "params": params})
}

/// The name of a file or folder, as a sentence names it.
fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// The AI's own measure of a model file here, with setup's reasons beside the AI's English: it fits, it can run only from
/// the disk (much more slowly), or it does not fit.
fn measured(m: &Value) -> Value {
    let mut m = m.clone();
    let code = if m["fits"].as_bool().unwrap_or(false) {
        "measured_fits"
    } else if m["from_disk"].as_bool().unwrap_or(false) {
        "measured_from_disk"
    } else {
        "measured_too_big"
    };
    m["why_codes"] = json!([hardware::reason(code, json!({}))]);
    m
}

fn names_of(entry: Option<&Value>) -> Vec<String> {
    entry.map(|e| e["files"].as_array().into_iter().flatten().map(|f| s(&f["name"]).to_string()).collect()).unwrap_or_default()
}

fn mtime_ns(p: &Path) -> i128 {
    std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos() as i128).unwrap_or(0)
}

pub fn now() -> String {
    let secs = install::now_secs() as i64;
    let (y, m, d) = install::civil_from_days(secs.div_euclid(86400));
    let t = secs.rem_euclid(86400);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}", t / 3600, t % 3600 / 60, t % 60)
}

/// Setup's work folder for a library: <its name>.setup beside it.
pub fn work_for(target: &Path) -> PathBuf {
    target.with_file_name(format!("{}{WORK_SUFFIX}", target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()))
}

pub fn job_file(settings_dir: &Path) -> PathBuf {
    settings_dir.join("setup").join("job.json")
}

pub fn pending_file(settings_dir: &Path) -> PathBuf {
    settings_dir.join("setup").join("pending.json")
}

/// On Windows the library reads its files by ordinary paths, so <target>\<longest file> must fit in WINDOWS_MAX_PATH.
pub fn path_too_deep(target: &Path, windows: bool) -> bool {
    windows && target.to_string_lossy().chars().count() + 1 + LONGEST_INSIDE > WINDOWS_MAX_PATH
}

/// Room left free beyond what an install needs, so the system keeps working: in all, 5% of the disk, at least 1 GB and
/// at most 5 GB. The install's own need already holds install::SPACE_MARGIN, so only the rest is added here. (Two
/// margins added together turned down a 43 MB library on a disk with 7 GB free.)
fn margin(total: u64) -> u64 {
    (total / 20).clamp(1_000_000_000, RECOMMEND_MARGIN).saturating_sub(install::SPACE_MARGIN)
}

/// Room a region map or address index needs: what is still to download plus its unpacked size.
fn map_need(mp: Option<&Value>, have: &HashSet<String>) -> u64 {
    let Some(mp) = mp else { return 0 };
    mp["files"].as_array().into_iter().flatten().filter(|f| !have.contains(s(&f["name"]))).map(|f| u(&f["size"])).sum::<u64>() + u(&mp["installed_bytes"])
}

fn sha256_file(path: &Path, control: &Control, progress: &mut dyn FnMut(u64, u64)) -> Result<String, SetupError> {
    let mut f = std::fs::File::open(path)
        .map_err(|e| download::failed_with("network", json!({"name": file_name(path)}), format!("Lastbook could not read {} ({e}).", path.display())))?;
    let total = f.metadata().map(|m| m.len()).unwrap_or(0);
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 8 * 1024 * 1024];
    let mut done = 0u64;
    loop {
        control.check()?;
        let n = f.read(&mut buf).map_err(|e| SetupError::failed("network", e.to_string()))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        done += n as u64;
        progress(done, total);
    }
    Ok(hex::encode(h.finalize()))
}

/// Each item's source: 'found' when all its files are already on this computer (plan.sources), else 'download'.
fn tag_sources(job: &mut Value) {
    let plan = job["plan"].clone();
    for it in job["items"].as_array_mut().into_iter().flatten() {
        let names: Vec<String> = match s(&it["kind"]) {
            "pack" => names_of(plan.get("pack").filter(|v| !v.is_null())),
            "map" => names_of(plan.get("map").filter(|v| !v.is_null())),
            "addresses" => names_of(plan.get("addresses").filter(|v| !v.is_null())),
            "model" => plan["model"]["file"].as_str().map(|f| vec![f.to_string()]).unwrap_or_default(),
            _ => vec![],
        };
        let found = !names.is_empty() && names.iter().all(|n| plan["sources"].get(n).is_some());
        it["source"] = json!(if found { "found" } else { "download" });
    }
}

fn item(id: &str, kind: &str, title: &str, total: u64, state: &str) -> Value {
    json!({"id": id, "kind": kind, "title": title, "state": state, "bytes_done": 0, "bytes_total": total, "rate_bps": 0, "eta_s": null, "error": null})
}

/// Progress of one item, with rate and time left.
struct Tracker {
    t: Instant,
    at: u64,
    rate: f64,
    state: String,
}

impl Tracker {
    fn new(it: &Value) -> Tracker {
        Tracker { t: Instant::now(), at: u(&it["bytes_done"]), rate: 0.0, state: s(&it["state"]).to_string() }
    }

    fn progress(&mut self, it: &mut Value, done: u64, total: u64, state: &str) {
        if !state.is_empty() && self.state != state {
            self.state = state.to_string();
            it["state"] = json!(state);
            self.t = Instant::now();
            self.at = done;
            self.rate = 0.0;
        }
        it["bytes_done"] = json!(done);
        it["bytes_total"] = json!(total);
        let el = self.t.elapsed().as_secs_f64();
        if el >= 1.0 {
            let inst = done.saturating_sub(self.at) as f64 / el;
            self.rate = if self.rate == 0.0 { inst } else { 0.6 * self.rate + 0.4 * inst };
            self.t = Instant::now();
            self.at = done;
            it["rate_bps"] = json!(self.rate as u64);
            it["eta_s"] = if self.rate > 0.0 { json!(((total.saturating_sub(done)) as f64 / self.rate).round() as u64) } else { Value::Null };
        }
    }
}

impl Setup {
    pub fn new(o: Options) -> Setup {
        let me = Setup(Arc::new(Inner {
            o,
            job: Mutex::new(None),
            control: Mutex::new(None),
            thread: Mutex::new(None),
            action: Mutex::new(()),
            sources: Mutex::new(HashMap::new()),
            rejected: Mutex::new(HashMap::new()),
            ai_measure: Mutex::new(HashMap::new()),
            measuring: Mutex::new(HashSet::new()),
            deleting: Mutex::new(HashSet::new()),
        }));
        let job = me.load_job();
        *me.0.job.lock().unwrap() = job;
        me
    }

    fn o(&self) -> &Options {
        &self.0.o
    }

    fn disk_of(&self, p: &Path) -> (u64, u64) {
        match &self.o().disk_space {
            Some(f) => f(p),
            None => hardware::disk_of(p),
        }
    }

    pub fn manifest(&self) -> &Manifest {
        &self.0.o.manifest
    }

    // --------------------------------------------------------------------------------------------- saved state --
    fn settings(&self) -> Value {
        install::read_json(&self.o().settings_dir.join("setup.json")).filter(|v| v.is_object()).unwrap_or_else(|| json!({}))
    }

    fn save_settings(&self, d: &Value) {
        let _ = std::fs::create_dir_all(&self.o().settings_dir);
        if let Err(e) = install::write_json(&self.o().settings_dir.join("setup.json"), d) {
            eprintln!("setup: could not save setup.json: {e}");
        }
    }

    /// The saved job, from the app's own settings folder only, with its plan rebuilt from the manifest. Anything that does
    /// not check out is no job: it never stops the app from starting.
    fn load_job(&self) -> Option<Value> {
        let raw = install::read_json(&job_file(&self.o().settings_dir))?;
        let mut job = match self.checked_job(raw) {
            Some(j) => j,
            None => {
                eprintln!("setup: the saved job was not used");
                return None;
            }
        };
        if ACTIVE.contains(&s(&job["state"])) {
            // this program stopped while it ran: carry on only when the person says so
            job["state"] = json!("paused");
            let plan = job["plan"].clone();
            for it in job["items"].as_array_mut().into_iter().flatten() {
                let was = s(&it["state"]).to_string();
                if ACTIVE.contains(&was.as_str()) {
                    it["state"] = json!("paused");
                    if was != "waiting" {
                        it["paused_from"] = json!(was);
                    }
                    let (d, t) = self.done_after_restart(&plan, it, &was);
                    it["bytes_done"] = json!(d);
                    it["bytes_total"] = json!(t);
                }
                it["rate_bps"] = json!(0);
                it["eta_s"] = Value::Null;
            }
        }
        Some(job)
    }

    /// A job as this program would have written it, or None. Nothing from disk is used as a path or a file name unless it
    /// is what the plan's ids and the parent folder give.
    fn checked_job(&self, mut job: Value) -> Option<Value> {
        let plan = job.get("plan").filter(|p| p.is_object())?.clone();
        let items = job.get("items").and_then(|i| i.as_array()).filter(|a| a.iter().all(|i| i.is_object()))?.clone();
        let state = s(&job["state"]);
        if !(ACTIVE.contains(&state) || ["paused", "done", "failed"].contains(&state)) {
            return None;
        }
        if let Some(lib) = plan["use_library"].as_str() {
            let lib = PathBuf::from(lib);
            if !install::is_library(&lib) {
                return None;
            }
            job["plan"] = json!({"use_library": lib.to_string_lossy(), "work": work_for(&lib).to_string_lossy(), "target": lib.to_string_lossy()});
            return Some(job);
        }
        let parent = PathBuf::from(plan["parent"].as_str()?);
        let target = PathBuf::from(plan["target"].as_str()?);
        let current = self.o().current.as_deref();
        if target.parent() != Some(parent.as_path()) || (target.file_name().map(|n| n.to_string_lossy() != DATA_NAME).unwrap_or(true) && Some(target.as_path()) != current) {
            return None; // a library setup installs is called LastbookData; only the open one may have another name
        }
        let work = work_for(&target);
        let id = |key: &str| plan[key]["id"].as_str().map(str::to_string);
        let pack = if plan["pack"].is_null() { None } else { Some(self.manifest().pack(&id("pack")?)?) };
        let mp = if plan["map"].is_null() { None } else { Some(self.manifest().map(&id("map")?)?) };
        let model = if plan["model"].is_null() { None } else { Some(self.manifest().model(Some(&id("model")?))?) };
        let addresses = if plan["addresses"].is_null() { None } else { Some(self.manifest().address_index(&id("addresses")?)?) };
        let mut wanted: HashSet<(String, String)> = HashSet::new();
        for (k, e) in [("pack", &pack), ("map", &mp), ("model", &model), ("addresses", &addresses)] {
            if let Some(e) = e {
                wanted.insert((k.into(), s(&e["id"]).into()));
            }
        }
        let have: HashSet<(String, String)> = items.iter().map(|i| (s(&i["kind"]).to_string(), s(&i["id"]).to_string())).collect();
        if have != wanted {
            return None;
        }
        let mut names = names_of(pack.as_ref());
        names.extend(names_of(mp.as_ref()));
        names.extend(names_of(addresses.as_ref()));
        if let Some(m) = &model {
            names.push(s(&m["file"]).to_string());
        }
        let mut sources = Map::new();
        for (n, v) in plan["sources"].as_object().cloned().unwrap_or_default() {
            if let Some(p) = v.as_str() {
                let pp = Path::new(p);
                if names.contains(&n) && pp.file_name().map(|f| f.to_string_lossy() == n).unwrap_or(false) && pp.is_file() {
                    sources.insert(n, json!(p));
                }
            }
        }
        let switch = plan["switch"].as_bool().unwrap_or(false) && pack.is_some();
        let model_dir = match (&self.o().models_here, plan["model_dir"].as_str()) {
            (Some(here), Some(d)) if Path::new(d) == here.as_path() => json!(d), // only the app's own folder, never a path from disk
            _ => Value::Null,
        };
        let mut clean = json!({"pack": pack, "model": model, "map": mp, "addresses": addresses, "parent": parent.to_string_lossy(),
                               "target": target.to_string_lossy(), "work": work.to_string_lossy(), "sources": sources,
                               "explicit": plan["explicit"].as_bool().unwrap_or(false), "retried": {}, "switch": switch, "model_dir": model_dir,
                               "repair": plan["repair"].as_bool().unwrap_or(false)});
        if let Some(d) = plan["damaged"].as_array() {
            clean["damaged"] = json!(d.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>());
        }
        if !plan["map_new"].is_null() {
            clean["map_new"] = json!(work.join("unpack-map").join(DATA_NAME).to_string_lossy());
        }
        if let (Some(m), true) = (&model, !plan["model_file"].is_null()) {
            let lib = if switch { target.clone() } else { current.map(Path::to_path_buf).unwrap_or(target.clone()) };
            clean["model_file"] = json!(lib.join("models").join(s(&m["file"])).to_string_lossy());
            if !plan["model_sha256"].is_null() {
                clean["model_sha256"] = m["sha256"].clone();
            }
        }
        job["plan"] = clean;
        Some(job)
    }

    /// (bytes done, bytes total) for an item found mid-run at startup, from what is on disk for the step it was in.
    fn done_after_restart(&self, plan: &Value, it: &Value, was: &str) -> (u64, u64) {
        let work = PathBuf::from(s(&plan["work"]));
        if was == "unpacking" {
            for (kind, sub) in [("map", "unpack-map"), ("addresses", "unpack-addresses")] {
                if s(&it["kind"]) == kind && !plan[kind].is_null() {
                    let total = u(&plan[kind]["installed_bytes"]);
                    return (install::tree_size(&work.join(sub)).min(total), total);
                }
            }
            if !plan["pack"].is_null() {
                let total = u(&plan["pack"]["installed_bytes"]);
                return (install::tree_size(&work.join("unpack")).min(total), total);
            }
        }
        if s(&it["source"]) == "found" {
            return (u(&it["bytes_total"]), u(&it["bytes_total"]));
        }
        (Self::on_disk(plan, it), u(&it["bytes_total"]))
    }

    fn on_disk(plan: &Value, it: &Value) -> u64 {
        let kind = s(&it["kind"]);
        let files: Vec<String> = if ["pack", "map", "addresses"].contains(&kind) && !plan[kind].is_null() {
            names_of(Some(&plan[kind]))
        } else {
            plan["model"]["file"].as_str().map(|f| vec![f.to_string()]).unwrap_or_default()
        };
        let dl = Path::new(s(&plan["work"])).join("downloads");
        files.iter().flat_map(|n| [dl.join(n), dl.join(format!("{n}.part"))]).filter_map(|p| std::fs::metadata(p).ok()).map(|m| m.len()).sum()
    }

    fn save_job(&self, job: &Value) -> std::io::Result<()> {
        let f = job_file(&self.o().settings_dir);
        std::fs::create_dir_all(f.parent().unwrap())?;
        install::write_json(&f, job)
    }

    fn with_job<R>(&self, f: impl FnOnce(&mut Value) -> R) -> Option<R> {
        let mut g = self.0.job.lock().unwrap();
        let job = g.as_mut()?;
        let r = f(job);
        if let Err(e) = self.save_job(job) {
            eprintln!("setup: could not save the job: {e}");
        }
        Some(r)
    }

    // ------------------------------------------------------------------------------------------------- the state --
    pub fn current(&self) -> Option<PathBuf> {
        self.o().current.clone()
    }

    /// Ids of the manifest's models whose file is here, at its size.
    pub fn models_here(&self) -> Vec<String> {
        let Some(cur) = &self.o().current else { return vec![] };
        let folders: Vec<PathBuf> = std::iter::once(cur.join("models")).chain(self.o().models_here.clone()).collect();
        self.manifest().models().iter().filter(|m| {
            folders.iter().any(|f| std::fs::metadata(f.join(s(&m["file"]))).map(|md| md.len() == u(&m["size"])).unwrap_or(false))
        }).map(|m| s(&m["id"]).to_string()).collect()
    }

    /// Where a new model goes by default: on this computer when the library is on a network share or an external disk (a
    /// model read over the network or from a stick is slow, and may be unplugged), else beside the library.
    pub fn default_model_place(&self, library: &Path) -> &'static str {
        if self.o().models_here.is_some() && super::volumes::kind(library) != "local" {
            "computer"
        } else {
            "library"
        }
    }

    pub fn current_info(&self) -> Value {
        let Some(cur) = &self.o().current else { return Value::Null };
        let meta = install::read_json(&cur.join("pack.json")).unwrap_or(Value::Null);
        let pack = meta["name"].as_str().map(str::to_string);
        let revision = meta["content_revision"].as_str().unwrap_or("").to_string();
        let models = self.models_here();
        json!({"folder": cur.to_string_lossy(), "disk_kind": super::volumes::kind(cur), "pack": pack, "content_revision": revision, "map_bytes": install::map_bytes(cur),
               "model": models.first(), "models": models, "map": install::map_name(cur), "address_indexes": self.address_indexes(),
               "old": self.settings()["old"].clone().as_array().cloned().unwrap_or_default()})
    }

    pub fn address_indexes(&self) -> Vec<String> {
        let Some(cur) = &self.o().current else { return vec![] };
        let mut out = Vec::new();
        for e in self.manifest().current()["address_indexes"].as_array().cloned().unwrap_or_default() {
            let id = s(&e["id"]);
            let path = cur.join("derived").join("geo").join("addresses").join(format!("{id}.sqlite"));
            let Some(receipt) = install::read_json(&path.with_extension("json")) else { continue };
            let expected: Map<String, Value> = e["files"].as_array().into_iter().flatten().map(|f| (s(&f["name"]).to_string(), f["sha256"].clone())).collect();
            if std::fs::metadata(&path).map(|m| m.len() == u(&e["installed_bytes"])).unwrap_or(false) && receipt["files"] == Value::Object(expected) {
                out.push(id.to_string());
            }
        }
        out
    }

    /// Files in app-owned storage, the open library, or chosen by the person. Offered when the size matches; the SHA-256
    /// is checked when one is used.
    pub fn found(&self) -> Vec<Value> {
        let mut out = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        if let Some(look) = &self.o().look_for_libraries {
            for lib in look() {
                let key = lib.to_string_lossy().to_string();
                if !seen.contains(&key) && Some(&lib) != self.o().current.as_ref() && install::is_library(&lib) {
                    seen.insert(key);
                    out.push(json!({"kind": "library", "path": lib.to_string_lossy(), "name": lib.file_name().map(|n| n.to_string_lossy().into_owned())}));
                }
            }
        }
        let m = self.manifest().current();
        let mut files: HashMap<String, (String, String, u64)> = HashMap::new();
        for p in m["packs"].as_array().into_iter().flatten() {
            for f in p["files"].as_array().into_iter().flatten() {
                files.insert(s(&f["name"]).into(), ("pack".into(), s(&p["id"]).into(), u(&f["size"])));
            }
        }
        for model in m["models"].as_array().into_iter().flatten().filter(|x| x["available"].as_bool().unwrap_or(false)) {
            files.insert(s(&model["file"]).into(), ("model".into(), s(&model["id"]).into(), u(&model["size"])));
        }
        for (kind, key) in [("map", "maps"), ("addresses", "address_indexes")] {
            for x in m[key].as_array().into_iter().flatten() {
                for f in x["files"].as_array().into_iter().flatten() {
                    files.insert(s(&f["name"]).into(), (kind.into(), s(&x["id"]).into(), u(&f["size"])));
                }
            }
        }
        let mut places = vec![self.o().default_parent.clone()];
        if let Some(cur) = &self.o().current {
            places.push(cur.join("models"));
            places.push(cur.clone());
        }
        // models kept on this computer, and a folder named for them
        places.extend(self.o().models_here.clone());
        places.extend(std::env::var_os("LASTBOOK_MODEL_DIR").map(|v| std::env::split_paths(&v).collect::<Vec<_>>()).unwrap_or_default());
        let mut candidates: Vec<PathBuf> = self.0.sources.lock().unwrap().values().map(PathBuf::from).collect();
        for place in places {
            for e in std::fs::read_dir(&place).into_iter().flatten().flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if files.contains_key(&name) || (name.starts_with("Lastbook-") && name.ends_with(".zip")) {
                    candidates.push(e.path());
                }
            }
        }
        let rejected = self.0.rejected.lock().unwrap().clone();
        for c in candidates {
            let key = c.canonicalize().unwrap_or(c.clone()).to_string_lossy().to_string();
            if seen.contains(&key) || !c.is_file() {
                continue;
            }
            seen.insert(key);
            let name = c.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let (kind, id, size) = files.get(&name).cloned().map(|(k, i, s)| (k, Some(i), Some(s))).unwrap_or(("pack".into(), None, None));
            let len = std::fs::metadata(&c).map(|m| m.len()).unwrap_or(0);
            let is_rejected = rejected.get(&c.to_string_lossy().to_string()) == Some(&(len, mtime_ns(&c)));
            out.push(json!({"kind": kind, "path": c.to_string_lossy(), "name": name, "id": id, "size": len,
                            "size_ok": size == Some(len) && !is_rejected, "rejected": is_rejected}));
        }
        out
    }

    fn model_path_here(&self, model_id: &str) -> Option<PathBuf> {
        self.found().into_iter().find(|f| s(&f["kind"]) == "model" && s(&f["id"]) == model_id && f["size_ok"].as_bool().unwrap_or(false)).map(|f| PathBuf::from(s(&f["path"])))
    }

    /// Does this model fit this computer: the AI's own measure once its file is here, else an estimate from memory.
    pub fn model_fit(&self, hw: &Value, model: &Value) -> Value {
        let id = s(&model["id"]).to_string();
        if let Some(m) = self.0.ai_measure.lock().unwrap().get(&id) {
            return measured(m);
        }
        if let (Some(path), Some(ai)) = (self.model_path_here(&id), self.o().ai.clone()) {
            if self.0.measuring.lock().unwrap().insert(id.clone()) {
                let me = self.clone();
                let model = model.clone();
                std::thread::spawn(move || {
                    if let Some(r) = ai.measure(&model, &path) {
                        me.0.ai_measure.lock().unwrap().insert(s(&model["id"]).to_string(), r);
                    }
                });
            }
        }
        let gpus = hw["gpus"].as_array().cloned().unwrap_or_default();
        hardware::estimate_ai(hw["ram_bytes"].as_u64(), &gpus, hw["unified_memory"].as_bool().unwrap_or(false), u(&model["memory_bytes"]))
    }

    /// The model setup suggests: the largest that fits this computer, else None.
    fn best_model(&self, fits: &Map<String, Value>) -> Option<Value> {
        let mut ms: Vec<Value> = self.manifest().models().into_iter().filter(|m| m["available"].as_bool().unwrap_or(false)).collect();
        ms.sort_by_key(|m| std::cmp::Reverse(u(&m["memory_bytes"])));
        ms.into_iter().find(|m| fits.get(s(&m["id"])).is_some_and(|f| f["fits"].as_bool().unwrap_or(false)))
    }

    pub fn hardware(&self) -> Value {
        // the open library's folder first: one entry per disk, and the window finds a library's disk by its path
        let mut extra: Vec<PathBuf> = self.o().current.as_ref().and_then(|c| c.parent()).map(Path::to_path_buf).into_iter().collect();
        extra.push(self.o().default_parent.clone());
        let mut hw = hardware::detect(&extra);
        let mut fits = Map::new();
        for m in self.manifest().models().iter().filter(|m| m["available"].as_bool().unwrap_or(false)) {
            fits.insert(s(&m["id"]).to_string(), self.model_fit(&hw, m));
        }
        let shown = self.best_model(&fits).or_else(|| {
            // nothing fits: the smallest one says why
            let mut ms = self.manifest().models();
            ms.sort_by_key(|m| u(&m["memory_bytes"]));
            ms.into_iter().find(|m| m["available"].as_bool().unwrap_or(false))
        });
        hw["ai"] = match &shown {
            Some(m) => {
                let mut f = fits[s(&m["id"])].clone();
                f["model"] = m["id"].clone();
                f
            }
            None => json!({"fits": false, "needs_bytes": null, "estimate": true, "why": "No local AI is offered for this version.",
                           "why_codes": [hardware::reason("none_offered", json!({}))]}),
        };
        hw["ai_models"] = Value::Object(fits);
        hw
    }

    pub fn recommend(&self, hw: &Value) -> Value {
        let m = self.manifest().current();
        let available: Vec<Value> = m["packs"].as_array().cloned().unwrap_or_default().into_iter().filter(|p| p["available"].as_bool().unwrap_or(false)).collect();
        // the smallest library first: it gets a person started soonest, and the larger ones stay one tap away
        let mut packs: Vec<&Value> = available.iter().collect();
        packs.sort_by_key(|p| u(&p["installed_bytes"]));
        packs.truncate(1);
        let parent = self.o().current.as_ref().and_then(|c| c.parent().map(Path::to_path_buf)).unwrap_or(self.o().default_parent.clone());
        let (free, total) = self.disk_of(&parent);
        let fits: Map<String, Value> = hw["ai_models"].as_object().cloned().unwrap_or_default();
        let model = self.best_model(&fits);
        let none = HashSet::new();
        let need: Map<String, Value> = available.iter().map(|p| (s(&p["id"]).to_string(), json!(install::need_bytes(Some(p), None, &none)))).collect();
        let map_needs: Map<String, Value> = m["maps"].as_array().into_iter().flatten().filter(|x| x["available"].as_bool().unwrap_or(false))
            .map(|x| (s(&x["id"]).to_string(), json!(map_need(Some(x), &none)))).collect();
        let address_need: Map<String, Value> = m["address_indexes"].as_array().into_iter().flatten().filter(|x| x["available"].as_bool().unwrap_or(false))
            .map(|x| (s(&x["id"]).to_string(), json!(map_need(Some(x), &none)))).collect();
        if packs.is_empty() {
            return json!({"pack": null, "ai": false, "model": null, "folder": parent.to_string_lossy(), "need_bytes": need, "map_need_bytes": map_needs,
                          "address_need_bytes": address_need, "why": "No library is ready to download yet.", "why_codes": [hardware::reason("none_ready", json!({}))]});
        }
        let mut choice: Option<&Value> = None;
        let mut folder = parent.clone();
        let mut elsewhere: Option<Value> = None;
        for p in &packs {
            if free >= u(&need[s(&p["id"])]) + margin(total) {
                choice = Some(p);
                break;
            }
        }
        if choice.is_none() {
            'outer: for p in &packs {
                for d in hw["disks"].as_array().into_iter().flatten() {
                    let dp = PathBuf::from(s(&d["path"]));
                    if !hardware::same_disk(&dp, &parent) && u(&d["free_bytes"]) >= u(&need[s(&p["id"])]) + margin(u(&d["total_bytes"])) {
                        choice = Some(p);
                        folder = dp;
                        elsewhere = Some(d.clone());
                        break 'outer;
                    }
                }
            }
        }
        let Some(choice) = choice else {
            let small = packs.last().unwrap();
            let need_small = u(&need[s(&small["id"])]);
            return json!({"pack": null, "ai": false, "model": null, "folder": parent.to_string_lossy(), "need_bytes": need, "map_need_bytes": map_needs,
                          "address_need_bytes": address_need,
                          "why": format!("The smallest library, {}, needs {} free, with room left for the system, and this computer has {} free.",
                                         s(&small["title"]), gb(need_small + margin(total)), gb(free)),
                          "why_codes": [hardware::reason("none_fits", json!({"pack": small["id"], "need_bytes": need_small + margin(total), "free_bytes": free}))]});
        };
        let (mut free2, mut margin2) = (free, margin(total));
        if let Some(d) = &elsewhere {
            free2 = u(&d["free_bytes"]);
            margin2 = margin(u(&d["total_bytes"]));
        }
        let left = free2.saturating_sub(u(&need[s(&choice["id"])]));
        let ai = model.as_ref().is_some_and(|m| left >= u(&m["size"]) + margin2);
        let mut codes = Vec::new();
        let mut why = match &elsewhere {
            Some(d) => {
                let name = d["name"].as_str().map(str::to_string).unwrap_or_else(|| s(&d["path"]).to_string());
                let params = json!({"free_bytes": free, "pack": choice["id"], "disk": name});
                if s(&d["kind"]) == "network" {     // a NAS: the network, not a cable, keeps it there
                    codes.push(hardware::reason("other_disk_network", params));
                    format!("This computer’s own disk has {} free, not enough for any library, so {} goes on {name}, a network share. Keep this computer on that network: Lastbook reads the library from it.", gb(free), s(&choice["title"]))
                } else {
                    codes.push(hardware::reason("other_disk", params));
                    format!("This computer’s own disk has {} free, not enough for any library, so {} goes on {name}. Keep that disk connected: Lastbook reads the library from it.", gb(free), s(&choice["title"]))
                }
            }
            None => {
                codes.push(hardware::reason("start_small", json!({"pack": choice["id"]})));
                format!("{} is selected to get started. You can choose a larger library.", s(&choice["title"]))
            }
        };
        if let (Some(dw), None, None) = (&self.o().default_why, &elsewhere, &self.o().current) {
            // the app's note on its default folder (appdirs::default_choice: the user folder's path is too long on Windows)
            why = format!("{dw} {why}");
            codes.insert(0, hardware::reason("default_folder", json!({"folder": self.o().default_parent.to_string_lossy()})));
        }
        if model.is_some() && !ai {
            why.push_str(" The local AI fits this computer’s memory, but not on the disk as well.");
            codes.push(hardware::reason("ai_no_room", json!({})));
        } else if model.is_none() && !self.manifest().models().is_empty() {
            why.push(' ');
            why.push_str(s(&hw["ai"]["why"]));
            codes.extend(hw["ai"]["why_codes"].as_array().cloned().unwrap_or_default());
        }
        json!({"pack": choice["id"], "ai": ai, "model": if ai { model.map(|m| m["id"].clone()) } else { None }, "folder": folder.to_string_lossy(),
               "model_place": self.default_model_place(&folder),
               "need_bytes": need, "map_need_bytes": map_needs, "address_need_bytes": address_need, "why": why, "why_codes": codes})
    }

    pub fn state(&self) -> Value {
        self.manifest().fetch_once();
        let hw = self.hardware();
        let rec = self.recommend(&hw);
        let missing = self.o().missing.as_ref().map(|m| json!({"folder": m.to_string_lossy(), "here": m.exists()}));
        json!({"mode": self.o().mode.as_str(), "hardware": hw, "manifest": self.manifest().public(), "current": self.current_info(),
               "found": self.found(), "recommend": rec, "job": self.public_job(), "missing": missing})
    }

    pub fn public_job(&self) -> Value {
        match self.0.job.lock().unwrap().as_ref() {
            Some(j) => {
                let mut j = j.clone();
                j.as_object_mut().map(|o| o.remove("plan"));
                j
            }
            None => Value::Null,
        }
    }

    // ----------------------------------------------------------------------------------------------- the actions --
    fn parent_for(&self, folder: Option<&str>) -> PathBuf {
        match folder {
            Some(f) if !f.is_empty() => {
                let p = expand(f);
                if p.file_name().map(|n| n == DATA_NAME).unwrap_or(false) {
                    p.parent().map(Path::to_path_buf).unwrap_or(p)
                } else {
                    p
                }
            }
            _ => self.o().current.as_ref().and_then(|c| c.parent().map(Path::to_path_buf)).unwrap_or(self.o().default_parent.clone()),
        }
    }

    fn busy(&self) -> bool {
        self.0.job.lock().unwrap().as_ref().is_some_and(|j| ACTIVE.contains(&s(&j["state"])) || s(&j["state"]) == "paused")
    }

    /// pack: a pack id. model: a model id, or "" for the one setup suggests. map/address: region codes.
    #[allow(clippy::too_many_arguments)]
    pub fn start(&self, pack_id: Option<&str>, model_id: Option<&str>, folder: Option<&str>, sources: Map<String, Value>, explicit: bool,
                 repair: bool, map_id: Option<&str>, address_id: Option<&str>) -> Value {
        self.start_with(pack_id, model_id, folder, sources, explicit, repair, map_id, address_id, None)
    }

    /// As start, and model_place: "library" (beside it) or "computer" (the app's own folder on this computer); None for
    /// the default (default_model_place).
    #[allow(clippy::too_many_arguments)]
    pub fn start_with(&self, pack_id: Option<&str>, model_id: Option<&str>, folder: Option<&str>, sources: Map<String, Value>, explicit: bool,
                      repair: bool, map_id: Option<&str>, address_id: Option<&str>, model_place: Option<&str>) -> Value {
        if repair {
            return self.start_repair(model_id.is_some());
        }
        let _g = self.0.action.lock().unwrap();
        if self.busy() {
            return err("busy", json!({}), "A download is already under way. Pause or cancel it first.");
        }
        let mut pack = match pack_id {
            Some(id) => match self.manifest().pack(id) {
                Some(p) if p["available"].as_bool().unwrap_or(false) => Some(p),
                _ => return err("pack_unavailable", json!({}), "That library is not available yet."),
            },
            None => None,
        };
        let cur = self.current_info();
        let current = self.o().current.clone();
        let mut folder = folder.map(str::to_string);
        if let (Some(f), Some(c)) = (&folder, &current) {
            if install::same_path(&expand(f), c) {
                folder = None; // the window sends the shown folder, which in manage mode is the open library itself
            }
        }
        if let Some(p) = &pack {
            let same_revision = s(&p["revision"]).is_empty() || p["revision"] == cur["content_revision"];
            if self.o().mode == Mode::Manage && cur["pack"] == p["id"] && same_revision && folder.is_none() {
                pack = None; // the same library again: only the AI or a map is wanted
            }
        }
        let mut model = match model_id {
            None => None,
            Some("") => {
                let hw = self.hardware();
                let rec = self.recommend(&hw);
                match rec["model"].as_str().and_then(|id| self.manifest().model(Some(id))).or_else(|| self.manifest().model(None)) {
                    Some(m) => Some(m),
                    None => return err("no_ai_offered", json!({}), "No local AI is offered for this version."),
                }
            }
            Some(id) => match self.manifest().model(Some(id)) {
                Some(m) => Some(m),
                None => return err("model_unavailable", json!({}), "That local AI is not offered for this version."),
            },
        };
        let mut mp = match map_id {
            Some(id) => match self.manifest().map(id) {
                Some(m) if m["available"].as_bool().unwrap_or(false) => Some(m),
                _ => return err("map_unavailable", json!({}), "That region’s map is not available yet."),
            },
            None => None,
        };
        let requested_address = address_id.is_some();
        let mut address_key = address_id.map(str::to_string)
            .or(map_id.map(str::to_string))
            .or(pack.as_ref().and_then(|p| p["map"].as_str().map(str::to_string)))
            .or(cur["map"].as_str().map(str::to_string));
        if matches!(address_key.as_deref(), Some("base") | Some("europe")) {
            address_key = cur["address_indexes"].get(0).and_then(|v| v.as_str()).map(str::to_string);
        }
        let mut addresses = address_key.as_deref().and_then(|id| self.manifest().address_index(id));
        if !addresses.as_ref().is_some_and(|a| a["available"].as_bool().unwrap_or(false)) {
            if requested_address {
                return err("addresses_unavailable", json!({}), "That region’s address search is not available yet.");
            }
            addresses = None;
        }
        let parent0 = self.parent_for(folder.as_deref());
        let target = match (&current, &folder) {
            (Some(c), None) => c.clone(),
            _ => parent0.join(DATA_NAME),
        };
        let parent = target.parent().map(Path::to_path_buf).unwrap_or(parent0);
        if let Some(a) = &addresses {
            if cur["address_indexes"].as_array().into_iter().flatten().any(|v| v == &a["id"]) && Some(&target) == current.as_ref() {
                addresses = None;
            }
        }
        if let Some(m) = &model {
            if cur["models"].as_array().into_iter().flatten().any(|v| v == &m["id"]) && Some(&target) == current.as_ref() {
                model = None; // already here, and an upgrade in place carries it over
            }
        }
        if let Some(m) = &mp {
            if pack.is_none() && cur["map"] == m["id"] {
                mp = None; // this region's map is the one here already
            }
        }
        if self.o().mode == Mode::FirstRun && pack.is_none() {
            return err("choose_library", json!({}), "Choose a library first.");
        }
        if pack.is_none() && model.is_none() && mp.is_none() && addresses.is_none() {
            return err("nothing_to_add", json!({}), "Nothing to add: this library and the local AI are already here.");
        }
        if let Err(e) = std::fs::create_dir_all(&parent) {
            return err("cannot_make_folder", json!({}), format!("Lastbook cannot make a folder there ({e})."));
        }
        if tempfile_in(&parent).is_err() {
            return err("cannot_save_there", json!({}), "Lastbook cannot save in that folder. Choose another.");
        }
        if path_too_deep(&target, self.o().windows_paths) {
            let room = WINDOWS_MAX_PATH - LONGEST_INSIDE - 1 - DATA_NAME.len() - 1;
            return err("too_deep", json!({"room": room}), format!("That folder is too deep for Windows: the library’s longest file name would not fit. Choose a folder whose path is at most {room} characters long, such as Documents."));
        }
        if self.o().mode == Mode::FirstRun && target.exists() && !install::is_library(&target) && std::fs::read_dir(&target).map(|mut d| d.next().is_some()).unwrap_or(false) {
            return err("not_library_there", json!({"name": DATA_NAME}), format!("There is already a folder called {DATA_NAME} there that is not a library. Choose another place."));
        }
        let work = work_for(&target);
        let mut srcs: Map<String, Value> = self.0.sources.lock().unwrap().iter().map(|(k, v)| (k.clone(), json!(v))).collect();
        srcs.extend(sources);
        for f in self.found() {
            let name = s(&f["name"]).to_string();
            if f["size_ok"].as_bool().unwrap_or(false) && !srcs.contains_key(&name) && ["pack", "model", "map", "addresses"].contains(&s(&f["kind"])) {
                srcs.insert(name, f["path"].clone());
            }
        }
        let mut names = names_of(pack.as_ref());
        if let Some(m) = &model {
            names.push(s(&m["file"]).to_string());
        }
        names.extend(names_of(mp.as_ref()));
        names.extend(names_of(addresses.as_ref()));
        let have: HashSet<String> = srcs.keys().cloned().collect();
        let mut need = install::need_bytes(pack.as_ref(), model.as_ref(), &have) as i128 + map_need(mp.as_ref(), &have) as i128 + map_need(addresses.as_ref(), &have) as i128;
        let preserve_map = pack.as_ref().is_some_and(|p| cur["pack"] == p["id"]) && Some(&target) == current.as_ref() && mp.is_none()
            && !matches!(cur["map"].as_str(), None | Some("base"));
        if preserve_map {
            need += u(&cur["map_bytes"]) as i128;
        }
        for name in &names {
            if have.contains(name) {
                continue;
            }
            for done in [work.join("downloads").join(format!("{name}.part")), work.join("downloads").join(name)] {
                if let Ok(m) = std::fs::metadata(&done) {
                    need -= m.len() as i128; // what an earlier run already downloaded is on the disk already
                }
            }
        }
        let need = need.max(0) as u64;
        let (free, _) = self.disk_of(&parent);
        if free < need {
            let mut out = err("no_space", json!({"need_bytes": need, "free_bytes": free}),
                              format!("This needs {} free on that disk, and it has {}. Free some space or choose another disk.", gb(need), gb(free)));
            out["need_bytes"] = json!(need); // as before, for a window that reads them here
            out["free_bytes"] = json!(free);
            return out;
        }
        let mut items = Vec::new();
        if let Some(p) = &pack {
            items.push(item(s(&p["id"]), "pack", s(&p["title"]), u(&p["download_bytes"]), "waiting"));
        }
        if let Some(m) = &mp {
            items.push(item(s(&m["id"]), "map", s(&m["title"]), u(&m["download_bytes"]), "waiting"));
        }
        if let Some(a) = &addresses {
            items.push(item(s(&a["id"]), "addresses", &format!("{} address search", s(&a["title"])), u(&a["download_bytes"]), "waiting"));
        }
        if let Some(m) = &model {
            items.push(item(s(&m["id"]), "model", s(&m["title"]), u(&m["size"]), "waiting"));
        }
        let sources: Map<String, Value> = srcs.into_iter().filter(|(k, _)| names.contains(k)).collect();
        let place = model_place.filter(|p| *p == "computer" || *p == "library").unwrap_or_else(|| self.default_model_place(&parent));
        let model_dir = match (&model, place, &self.o().models_here) {
            (Some(_), "computer", Some(dir)) => json!(dir.to_string_lossy()),
            _ => Value::Null,
        };
        let mut job = json!({"id": uuid::Uuid::new_v4().simple().to_string()[..12].to_string(), "state": "waiting", "items": items, "error": null,
                             "started": now(), "folder": target.to_string_lossy(),
                             "plan": {"pack": pack, "model": model, "map": mp, "addresses": addresses, "parent": parent.to_string_lossy(),
                                      "target": target.to_string_lossy(), "work": work.to_string_lossy(), "sources": sources, "explicit": explicit,
                                      "retried": {}, "switch": pack.is_some(), "model_dir": model_dir}});
        let mut st = self.settings();
        st["work"] = json!(work.to_string_lossy());
        self.save_settings(&st);
        tag_sources(&mut job);
        if let Err(e) = self.save_job(&job) {
            return err("cannot_save_progress", json!({}), format!("Setup could not save its progress ({e})."));
        }
        *self.0.job.lock().unwrap() = Some(job);
        self.run();
        self.public_job()
    }

    /// Re-check the open library's files (pack-manifest.tsv) and the model's SHA-256. What is wrong is fetched again: the
    /// pack is downloaded and switched to as in an upgrade, the model replaced in place. Nothing is downloaded if all is well.
    fn start_repair(&self, ai: bool) -> Value {
        let _g = self.0.action.lock().unwrap();
        if self.busy() {
            return err("busy", json!({}), "A download is already under way. Pause or cancel it first.");
        }
        let cur = self.current_info();
        let Some(current) = self.o().current.clone().filter(|_| self.o().mode == Mode::Manage) else {
            return err("nothing_to_repair", json!({}), "There is no library to repair yet.");
        };
        let pack = cur["pack"].as_str().and_then(|id| self.manifest().pack(id));
        let Some(pack) = pack.filter(|p| p["available"].as_bool().unwrap_or(false)) else {
            return err("repair_unknown", json!({}), "Lastbook cannot tell which library this is, so it cannot repair it. Download a library instead.");
        };
        let model = if ai || !cur["models"].as_array().map(|a| a.is_empty()).unwrap_or(true) {
            cur["model"].as_str().and_then(|id| self.manifest().model(Some(id))).or_else(|| ai.then(|| self.manifest().model(None)).flatten())
        } else {
            None
        };
        let parent = current.parent().map(Path::to_path_buf).unwrap_or_default();
        let work = work_for(&current);
        let mut srcs = Map::new();
        for f in self.found() {
            if f["size_ok"].as_bool().unwrap_or(false) && ["pack", "model"].contains(&s(&f["kind"])) {
                srcs.insert(s(&f["name"]).to_string(), f["path"].clone());
            }
        }
        let mut names = names_of(Some(&pack));
        if let Some(m) = &model {
            names.push(s(&m["file"]).to_string());
        }
        let mut items = vec![item(s(&pack["id"]), "pack", s(&pack["title"]), u(&pack["installed_bytes"]), "waiting")];
        if let Some(m) = &model {
            items.push(item(s(&m["id"]), "model", s(&m["title"]), u(&m["size"]), "waiting"));
        }
        let sources: Map<String, Value> = srcs.into_iter().filter(|(k, _)| names.contains(k)).collect();
        let mut job = json!({"id": uuid::Uuid::new_v4().simple().to_string()[..12].to_string(), "state": "waiting", "items": items, "error": null,
                             "started": now(), "folder": current.to_string_lossy(), "repair": true,
                             "plan": {"pack": pack, "model": model, "map": null, "addresses": null, "parent": parent.to_string_lossy(),
                                      "target": current.to_string_lossy(), "work": work.to_string_lossy(), "sources": sources, "explicit": false,
                                      "retried": {}, "switch": true, "repair": true}});
        let mut st = self.settings();
        st["work"] = json!(work.to_string_lossy());
        self.save_settings(&st);
        tag_sources(&mut job);
        let _ = self.save_job(&job);
        *self.0.job.lock().unwrap() = Some(job);
        self.run();
        self.public_job()
    }

    fn run(&self) {
        let control = Control::new();
        *self.0.control.lock().unwrap() = Some(control.clone());
        let me = self.clone();
        let t = std::thread::Builder::new().name("setup-job".into()).spawn(move || me.work(&control)).expect("thread");
        *self.0.thread.lock().unwrap() = Some(t);
    }

    fn join(&self) {
        let t = self.0.thread.lock().unwrap().take();
        if let Some(t) = t {
            if t.thread().id() != std::thread::current().id() {
                let _ = t.join();
            }
        }
    }

    pub fn pause(&self) -> Value {
        if let Some(c) = self.0.control.lock().unwrap().as_ref() {
            if self.0.job.lock().unwrap().as_ref().is_some_and(|j| ACTIVE.contains(&s(&j["state"]))) {
                c.stop("paused");
            }
        }
        self.join();
        self.public_job()
    }

    pub fn resume(&self) -> Value {
        let go = self.with_job(|j| {
            if s(&j["state"]) != "paused" {
                return false;
            }
            j["state"] = json!("waiting");
            for it in j["items"].as_array_mut().into_iter().flatten() {
                it.as_object_mut().map(|o| o.remove("paused_from"));
            }
            true
        });
        if go == Some(true) {
            self.run();
        }
        self.public_job()
    }

    pub fn retry(&self) -> Value {
        let go = self.with_job(|j| {
            if s(&j["state"]) != "failed" {
                return false;
            }
            j["state"] = json!("waiting");
            j["error"] = Value::Null;
            j["plan"]["retried"] = json!({});
            for it in j["items"].as_array_mut().into_iter().flatten() {
                if s(&it["state"]) != "done" {
                    it["state"] = json!("waiting");
                    it["error"] = Value::Null;
                }
            }
            true
        });
        if go == Some(true) {
            self.run();
        }
        self.public_job()
    }

    pub fn cancel(&self) -> Value {
        if self.0.job.lock().unwrap().is_none() {
            return Value::Null;
        }
        if let Some(c) = self.0.control.lock().unwrap().as_ref() {
            if self.0.job.lock().unwrap().as_ref().is_some_and(|j| ACTIVE.contains(&s(&j["state"]))) {
                c.stop("cancelled");
            }
        }
        self.join();
        let work = self.0.job.lock().unwrap().as_ref().map(|j| (s(&j["state"]).to_string(), j["plan"]["switch"].as_bool().unwrap_or(false), PathBuf::from(s(&j["plan"]["work"]))));
        if let Some((state, switch, work)) = work {
            if state != "done" || switch {
                for sub in ["downloads", "unpack", "unpack-map", "unpack-addresses"] {
                    let _ = std::fs::remove_dir_all(work.join(sub));
                }
                let _ = std::fs::remove_file(pending_file(&self.o().settings_dir));
                self.with_job(|j| {
                    j["state"] = json!("failed");
                    j["error"] = json!({"code": "cancelled", "message": "Cancelled. Nothing was changed.", "params": {}});
                    for it in j["items"].as_array_mut().into_iter().flatten() {
                        if s(&it["state"]) != "done" {
                            it["state"] = json!("failed");
                        }
                    }
                });
            }
        }
        self.public_job()
    }

    pub fn use_path(&self, path: &str) -> Value {
        let p = expand(path);
        if path.trim().is_empty() || !p.exists() {
            return err("missing", json!({}), "Lastbook cannot find that file or folder.");
        }
        if p.is_dir() {
            let lib = if install::is_library(&p) { p.clone() } else { p.join(DATA_NAME) };
            if !install::is_library(&lib) {
                let m = self.manifest().current();
                let mut known: HashSet<String> = m["packs"].as_array().into_iter().flatten().flat_map(|pk| names_of(Some(pk))).collect();
                for model in self.manifest().models() {
                    known.insert(s(&model["file"]).to_string());
                }
                let mut inside: Vec<PathBuf> = std::fs::read_dir(&p).into_iter().flatten().flatten().map(|e| e.path())
                    .filter(|c| c.is_file() && c.file_name().is_some_and(|n| known.contains(&n.to_string_lossy().to_string()))).collect();
                inside.sort();
                if let Some(first) = inside.first() {
                    return self.use_path(&first.to_string_lossy()); // the person explicitly chose this folder
                }
                let name = file_name(&p);
                return err("not_library", json!({"name": name}), format!("“{name}” is not a Lastbook library. Choose the folder called {DATA_NAME}."));
            }
            if tempfile_in(&lib).is_err() {
                let name = file_name(&lib);
                return err("library_read_only", json!({"name": name}), format!("Lastbook cannot save in “{name}”. Move it somewhere you can save to, then choose it there."));
            }
            let _g = self.0.action.lock().unwrap();
            if self.0.job.lock().unwrap().as_ref().is_some_and(|j| ACTIVE.contains(&s(&j["state"]))) {
                return err("busy", json!({}), "A download is under way. Pause or cancel it first.");
            }
            let job = json!({"id": uuid::Uuid::new_v4().simple().to_string()[..12].to_string(), "state": "done", "error": null, "started": now(),
                             "folder": lib.to_string_lossy(),
                             "items": [{"id": "library", "kind": "pack", "title": lib.file_name().unwrap().to_string_lossy(), "state": "done",
                                        "bytes_done": 0, "bytes_total": 0, "rate_bps": 0, "eta_s": null, "error": null}],
                             "plan": {"use_library": lib.to_string_lossy(), "work": work_for(&lib).to_string_lossy(), "target": lib.to_string_lossy()}});
            *self.0.job.lock().unwrap() = Some(job);
            return self.public_job();
        }
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
        let m = self.manifest().current();
        for pk in m["packs"].as_array().into_iter().flatten() {
            for f in pk["files"].as_array().into_iter().flatten() {
                if s(&f["name"]) == name {
                    if size != u(&f["size"]) {
                        return err("wrong_size", json!({"name": name, "size_bytes": size, "expected_bytes": u(&f["size"])}),
                                   format!("That file is {}, not the {} of the published {name}. The download may not have finished.", gb(size), gb(u(&f["size"]))));
                    }
                    if !pk["available"].as_bool().unwrap_or(false) {
                        return err("pack_not_in_version", json!({}), "That library is not available in this version.");
                    }
                    self.0.sources.lock().unwrap().insert(name.clone(), p.to_string_lossy().to_string());
                    let mut src = Map::new();
                    src.insert(name.clone(), json!(p.to_string_lossy()));
                    return self.start(Some(s(&pk["id"])), None, None, src, true, false, None, None);
                }
            }
        }
        for (key, kind) in [("maps", "map"), ("address_indexes", "addresses")] {
            for x in m[key].as_array().into_iter().flatten() {
                for f in x["files"].as_array().into_iter().flatten() {
                    if s(&f["name"]) != name {
                        continue;
                    }
                    if size != u(&f["size"]) {
                        return err("wrong_size", json!({"name": name, "size_bytes": size, "expected_bytes": u(&f["size"])}),
                                   format!("That file is {}, not the {} of the published {name}. The download may not have finished.", gb(size), gb(u(&f["size"]))));
                    }
                    self.0.sources.lock().unwrap().insert(name.clone(), p.to_string_lossy().to_string());
                    if self.o().mode == Mode::Manage {
                        let mut src = Map::new();
                        src.insert(name.clone(), json!(p.to_string_lossy()));
                        let id = s(&x["id"]);
                        return if kind == "map" {
                            self.start(None, None, None, src, true, false, Some(id), None)
                        } else {
                            self.start(None, None, None, src, true, false, None, Some(id))
                        };
                    }
                    let j = self.public_job();
                    return if j.is_null() { json!({"job": null, "noted": true}) } else { j };
                }
            }
        }
        for model in self.manifest().models() {
            if s(&model["file"]) == name {
                if size != u(&model["size"]) {
                    return err("wrong_size_model", json!({"size_bytes": size, "expected_bytes": u(&model["size"])}),
                               format!("That file is {}, not the {} of the model. The download may not have finished.", gb(size), gb(u(&model["size"]))));
                }
                self.0.sources.lock().unwrap().insert(name.clone(), p.to_string_lossy().to_string());
                if self.o().mode == Mode::Manage {
                    let mut src = Map::new();
                    src.insert(name, json!(p.to_string_lossy()));
                    return self.start(None, Some(s(&model["id"])), None, src, true, false, None, None);
                }
                let j = self.public_job();
                return if j.is_null() { json!({"job": null, "noted": true}) } else { j };
            }
        }
        err("unknown_file", json!({}), "That is not a file Lastbook knows. Choose a Lastbook library zip or the model file.")
    }

    pub fn finish(&self) -> Value {
        let folder = {
            let g = self.0.job.lock().unwrap();
            match g.as_ref() {
                Some(j) if s(&j["state"]) == "done" => j["plan"]["use_library"].as_str().or(j["plan"]["target"].as_str()).unwrap_or("").to_string(),
                _ => return err("not_done", json!({}), "Setup has not finished yet."),
            }
        };
        let me = self.clone();
        if me.o().on_finish.is_none() {
            return json!({"ok": true, "restart": true, "folder": folder});
        }
        let f = folder.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400)); // after this answer has reached the window
            if let Some(cb) = &me.o().on_finish {
                cb(PathBuf::from(f));
            }
        });
        json!({"ok": true, "folder": folder})
    }

    pub fn pick(&self, files: bool) -> Value {
        let path = self.o().pick_folder.as_ref().and_then(|p| p(files));
        match path {
            None => json!({"path": null, "cancelled": true}),
            Some(p) => {
                let (free, _) = self.disk_of(&p);
                json!({"path": p.to_string_lossy(), "free_bytes": free, "disk": hardware::disk_name(&p), "kind": super::volumes::kind(&p)})
            }
        }
    }

    /// Manage mode: the model files go from the open library and from this computer's own folder for models (the one named,
    /// or every one). The library is not touched.
    pub fn remove_ai(&self, model_id: Option<&str>) -> Value {
        let Some(cur) = self.o().current.clone().filter(|_| self.o().mode == Mode::Manage) else {
            return err("no_ai_to_remove", json!({}), "There is no local AI in this library to remove.");
        };
        if self.0.job.lock().unwrap().as_ref().is_some_and(|j| ACTIVE.contains(&s(&j["state"]))) {
            return err("busy", json!({}), "A download is under way. Pause or cancel it first.");
        }
        let folders: Vec<PathBuf> = std::iter::once(cur.join("models")).chain(self.o().models_here.clone()).collect();
        let targets: Vec<PathBuf> = self.manifest().models().iter().filter(|m| model_id.is_none_or(|id| s(&m["id"]) == id))
            .flat_map(|m| folders.iter().map(|f| f.join(s(&m["file"]))).collect::<Vec<_>>()).filter(|p| p.is_file()).collect();
        if targets.is_empty() {
            return err("no_ai_to_remove", json!({}), "There is no local AI in this library to remove.");
        }
        if let Some(ai) = &self.o().ai {
            ai.stop("removed");
        }
        let mut freed = 0;
        for p in targets {
            let n = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            if let Err(e) = std::fs::remove_file(&p) {
                return err("cannot_remove_model", json!({}), format!("Lastbook could not remove the model file ({e}). Quit anything using it, then try again."));
            }
            freed += n;
        }
        self.0.ai_measure.lock().unwrap().clear();
        if let Some(ai) = &self.o().ai {
            ai.rediscover();
        }
        json!({"ok": true, "freed_bytes": freed})
    }

    pub fn delete_old(&self, path: &str) -> Value {
        let st = self.settings();
        let olds: Vec<String> = st["old"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).collect();
        if !olds.contains(&path.to_string()) {
            return err("not_moved_aside", json!({}), "Lastbook only deletes an old library it moved aside itself.");
        }
        let p = PathBuf::from(path);
        if !p.file_name().is_some_and(|n| n.to_string_lossy().contains(".old-")) {
            return err("not_old_library", json!({}), "That is not an old library.");
        }
        self.0.deleting.lock().unwrap().insert(path.to_string());
        let me = self.clone();
        let path = path.to_string();
        std::thread::spawn(move || {
            let _ = std::fs::remove_dir_all(&p);
            let mut st = me.settings();
            let left: Vec<Value> = st["old"].as_array().into_iter().flatten().filter(|o| o.as_str() != Some(&path) || Path::new(o.as_str().unwrap_or("")).exists()).cloned().collect();
            st["old"] = Value::Array(left);
            me.save_settings(&st);
            me.0.deleting.lock().unwrap().remove(&path);
        });
        json!({"ok": true, "deleting": self.0.deleting.lock().unwrap().iter().next()})
    }

    // ------------------------------------------------------------------------------------------------ the work --
    fn set(&self, kind: &str, id: &str, state: &str, extra: &[(&str, Value)]) {
        self.with_job(|j| {
            if let Some(it) = j["items"].as_array_mut().and_then(|a| a.iter_mut().find(|i| s(&i["kind"]) == kind && s(&i["id"]) == id)) {
                it["state"] = json!(state);
                for (k, v) in extra {
                    it[*k] = v.clone();
                }
            }
            j["state"] = json!(state);
        });
    }

    /// The job stops: {code, message, params} (and anything more the failure carries) on the job and on its item.
    fn fail(&self, cur: Option<(String, String)>, e: &SetupError) {
        let mut errv = json!({"code": e.code(), "message": e.message(), "params": {}});
        if let SetupError::Failed { extra, .. } = e {
            for (k, v) in extra {
                errv[k] = v.clone();
            }
        }
        self.with_job(|j| {
            if let Some((kind, id)) = &cur {
                if let Some(it) = j["items"].as_array_mut().and_then(|a| a.iter_mut().find(|i| s(&i["kind"]) == kind && s(&i["id"]) == id)) {
                    it["state"] = json!("failed");
                    it["error"] = errv.clone();
                    it["rate_bps"] = json!(0);
                    it["eta_s"] = Value::Null;
                }
            }
            j["state"] = json!("failed");
            j["error"] = errv.clone();
        });
        eprintln!("setup: failed ({}): {}", e.code(), e.message());
    }

    fn progress_fn(&self, kind: &str, id: &str, offset: u64) -> impl FnMut(u64, u64, &str) + Send + 'static {
        let me = self.clone();
        let (kind, id) = (kind.to_string(), id.to_string());
        let mut tracker: Option<Tracker> = None;
        move |done, _total, phase| {
            let mut g = me.0.job.lock().unwrap();
            if let Some(it) = g.as_mut().and_then(|j| j["items"].as_array_mut()).and_then(|a| a.iter_mut().find(|i| s(&i["kind"]) == kind && s(&i["id"]) == id)) {
                let t = tracker.get_or_insert_with(|| Tracker::new(it));
                let total = u(&it["bytes_total"]);
                t.progress(it, offset + done, total, phase);
            }
        }
    }

    fn plan(&self) -> Value {
        self.0.job.lock().unwrap().as_ref().map(|j| j["plan"].clone()).unwrap_or(Value::Null)
    }

    fn set_plan(&self, f: impl FnOnce(&mut Value)) {
        self.with_job(|j| f(&mut j["plan"]));
    }

    fn work(&self, control: &Control) {
        let plan = self.plan();
        let work = PathBuf::from(s(&plan["work"]));
        let items: Vec<(String, String, String)> = self.0.job.lock().unwrap().as_ref().map(|j| j["items"].as_array().into_iter().flatten()
            .map(|i| (s(&i["kind"]).to_string(), s(&i["id"]).to_string(), s(&i["state"]).to_string())).collect()).unwrap_or_default();
        let mut cur: Option<(String, String)> = None;
        let result = (|| -> Result<(), SetupError> {
            std::fs::create_dir_all(work.join("downloads")).map_err(|e| SetupError::failed("folder", e.to_string()))?;
            for (kind, id, state) in &items {
                if state == "done" {
                    continue;
                }
                cur = Some((kind.clone(), id.clone()));
                match kind.as_str() {
                    "pack" => self.do_pack(kind, id, &work, control)?,
                    "map" => self.do_map(kind, id, &work, control)?,
                    "addresses" => self.do_addresses(kind, id, &work, control)?,
                    _ => self.do_model(kind, id, &work, control)?,
                }
                self.with_job(|j| {      // the item alone: the job is done only once finish_job has staged the switch
                    if let Some(it) = j["items"].as_array_mut().and_then(|a| a.iter_mut().find(|i| s(&i["kind"]) == kind.as_str() && s(&i["id"]) == id.as_str())) {
                        it["state"] = json!("done");
                        it["rate_bps"] = json!(0);
                        it["eta_s"] = Value::Null;
                    }
                });
            }
            cur = None;
            self.finish_job(&work)
        })();
        match result {
            Ok(()) => {}
            Err(SetupError::Stopped(why)) => {
                if why == "paused" {
                    self.with_job(|j| {
                        if let Some((kind, id)) = &cur {
                            if let Some(it) = j["items"].as_array_mut().and_then(|a| a.iter_mut().find(|i| s(&i["kind"]) == kind && s(&i["id"]) == id)) {
                                it["state"] = json!("paused");
                                it["rate_bps"] = json!(0);
                                it["eta_s"] = Value::Null;
                            }
                        }
                        j["state"] = json!("paused");
                    });
                }
            }
            Err(e) => self.fail(cur, &e),
        }
    }

    /// The file, checked against its SHA-256: one already on this computer, or a download. (its path, whether setup made it)
    fn get_file(&self, kind: &str, id: &str, f: &Value, work: &Path, control: &Control, offset: u64) -> Result<(PathBuf, bool), SetupError> {
        let name = s(&f["name"]).to_string();
        let plan = self.plan();
        let src = plan["sources"][&name].as_str().map(PathBuf::from);
        if let Some(src) = &src {
            if src.is_file() && plan["verified"].as_array().into_iter().flatten().any(|v| s(v) == name) {
                return Ok((src.clone(), false));
            }
        }
        let explicit_use = plan["explicit"].as_bool().unwrap_or(false);
        if let Some(src) = src.filter(|p| p.is_file()) {
            self.set(kind, id, "verifying", &[]);
            let mut prog = self.progress_fn(kind, id, offset);
            let digest = sha256_file(&src, control, &mut |d, t| prog(d, t, "verifying"))?;
            if digest == s(&f["sha256"]) {
                self.set_plan(|p| {
                    if !p["verified"].is_array() {
                        p["verified"] = json!([]);
                    }
                    p["verified"].as_array_mut().unwrap().push(json!(name));
                });
                return Ok((src, false));
            }
            let len = std::fs::metadata(&src).map(|m| m.len()).unwrap_or(0);
            self.0.rejected.lock().unwrap().insert(src.to_string_lossy().to_string(), (len, mtime_ns(&src)));
            self.set_plan(|p| {
                if let Some(o) = p["sources"].as_object_mut() {
                    o.remove(&name); // a retry downloads it instead; the file itself is left where it is, untouched
                }
                p["explicit"] = json!(false);
            });
            self.with_job(|j| {
                if let Some(it) = j["items"].as_array_mut().and_then(|a| a.iter_mut().find(|i| s(&i["kind"]) == kind && s(&i["id"]) == id)) {
                    it["source"] = json!("download");
                }
            });
            if explicit_use {
                let folder = src.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let mut extra = Map::new();
                extra.insert("file".into(), json!(src.to_string_lossy()));
                extra.insert("params".into(), json!({"name": name, "folder": folder}));
                return Err(SetupError::Failed { code: "found_mismatch".into(), message: format!("The {name} in {folder} doesn’t match the published file, so it was not used. It was left as it is. Download it instead?"), extra });
            }
            eprintln!("setup: {} does not match; downloading instead", src.display());
        }
        let dest = work.join("downloads").join(&name);
        if !dest.exists() {
            let still = u(&f["size"]).saturating_sub(std::fs::metadata(work.join("downloads").join(format!("{name}.part"))).map(|m| m.len()).unwrap_or(0));
            let (free, _) = self.disk_of(work);
            if free < still + 512 * 1024 * 1024 {
                let mut extra = Map::new();
                extra.insert("need_bytes".into(), json!(still));
                extra.insert("free_bytes".into(), json!(free));
                extra.insert("params".into(), json!({"name": name, "need_bytes": still, "free_bytes": free}));
                return Err(SetupError::Failed { code: "no_space".into(), message: format!("Downloading {name} needs {} free, and the disk has {}. Free some space, then try again.", gb(still), gb(free)), extra });
            }
        }
        loop {
            self.set(kind, id, "downloading", &[]);
            let job_control = self.0.control.lock().unwrap().clone().unwrap_or_else(Control::new);
            let mut d = download::Download::new(s(&f["url"]), &dest, u(&f["size"]), s(&f["sha256"]), job_control, Box::new(self.progress_fn(kind, id, offset)),
                                                self.o().allow.clone(), self.o().tls.clone());
            if let Some(b) = &self.o().backoff {
                d.backoff = b.clone();
            }
            match d.run() {
                Ok(p) => return Ok((p, true)),
                Err(SetupError::BadChecksum(_)) => {
                    let tries = self.plan()["retried"][&name].as_u64().unwrap_or(0);
                    if tries >= 1 {
                        return Err(download::failed_with("checksum", json!({"name": name}),
                                                         format!("{name} did not match its published checksum twice. The file on the server may be damaged; try again later.")));
                    }
                    self.set_plan(|p| p["retried"][&name] = json!(tries + 1));
                    eprintln!("setup: {name} failed its checksum; downloading it once more");
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Files of the open library that are missing or differ from its pack-manifest.tsv (path, bytes, sha256).
    fn check_library(&self, kind: &str, id: &str, lib: &Path, control: &Control) -> Result<Vec<String>, SetupError> {
        let Ok(text) = std::fs::read_to_string(lib.join("pack-manifest.tsv")) else { return Ok(vec!["pack-manifest.tsv".into()]) };
        let mut rows = Vec::new();
        for l in text.lines().skip(1).filter(|l| !l.trim().is_empty()) {
            let parts: Vec<&str> = l.split('\t').collect();
            match (parts.first(), parts.get(1).and_then(|n| n.parse::<u64>().ok()), parts.get(2)) {
                (Some(p), Some(n), Some(h)) => rows.push((p.to_string(), n, h.to_string())),
                _ => return Ok(vec!["pack-manifest.tsv".into()]),
            }
        }
        if install::map_set(lib).is_some() {
            rows.retain(|r| !install::map_member(&r.0, false)); // a region map replaced the pack's own: checked against map-set.json
            if let Err(e) = install::check_map(lib) {
                eprintln!("setup: repair: {}", e.message());
            }
        }
        let total: u64 = rows.iter().map(|r| r.1).sum::<u64>().max(1);
        self.set(kind, id, "verifying", &[("bytes_done", json!(0)), ("bytes_total", json!(total))]);
        let mut prog = self.progress_fn(kind, id, 0);
        let (mut done, mut bad) = (0u64, Vec::new());
        for (rel, size, digest) in rows {
            let p = lib.join(&rel);
            let unsafe_rel = rel.split('/').any(|x| x == "..") || Path::new(&rel).is_absolute();
            if unsafe_rel || !p.is_file() || std::fs::metadata(&p).map(|m| m.len()).ok() != Some(size) {
                bad.push(rel);
                done += size;
                prog(done, total, "verifying");
                continue;
            }
            let base = done;
            let got = sha256_file(&p, control, &mut |d, _| prog(base + d, total, "verifying"))?;
            if got != digest {
                bad.push(rel);
            }
            done = base + size;
        }
        Ok(bad)
    }

    fn do_pack(&self, kind: &str, id: &str, work: &Path, control: &Control) -> Result<(), SetupError> {
        let plan = self.plan();
        let pack = plan["pack"].clone();
        if plan["repair"].as_bool().unwrap_or(false) && plan["damaged"].is_null() {
            let bad = self.check_library(kind, id, Path::new(s(&plan["target"])), control)?;
            eprintln!("setup: repair found {} damaged or missing files", bad.len());
            let none = bad.is_empty();
            self.set_plan(|p| {
                p["damaged"] = json!(bad);
                if none {
                    p["switch"] = json!(false);
                }
            });
            if none {
                return Ok(());
            }
            let have: HashSet<String> = plan["sources"].as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
            let need = install::need_bytes(Some(&pack), None, &have);
            let (free, _) = self.disk_of(work.parent().unwrap_or(work));
            if free < need {
                return Err(download::failed_with("no_space", json!({"need_bytes": need, "free_bytes": free}),
                                                 format!("Some files need fetching again. That needs {} free, and the disk has {}.", gb(need), gb(free))));
            }
            self.with_job(|j| {
                if let Some(it) = j["items"].as_array_mut().and_then(|a| a.iter_mut().find(|i| s(&i["kind"]) == kind)) {
                    it["bytes_total"] = pack["download_bytes"].clone();
                    it["bytes_done"] = json!(0);
                }
            });
        }
        let staged = work.join("unpack");
        let mut got = Vec::new();
        let mut offset = 0;
        for f in pack["files"].as_array().cloned().unwrap_or_default() {
            got.push(self.get_file(kind, id, &f, work, control, offset)?);
            offset += u(&f["size"]);
        }
        let installed = pack["installed_bytes"].as_u64();
        self.set(kind, id, "unpacking", &[("bytes_done", json!(0)), ("bytes_total", json!(installed))]);
        let mut total = 0;
        for (path, _) in &got {
            total += install::check_zip(path, installed, None)?.1; // every member checked before any is written
        }
        let still = total.saturating_sub(install::tree_size(&staged));
        let (free, _) = self.disk_of(work);
        if free < still + 512 * 1024 * 1024 {
            return Err(download::failed_with("no_space", json!({"need_bytes": still, "free_bytes": free}),
                                             format!("Unpacking needs {} free, and the disk has {}. Free some space, then try again.", gb(still), gb(free))));
        }
        let mut prog = self.progress_fn(kind, id, 0);
        let mut lib = PathBuf::new();
        for (path, _) in &got {
            lib = install::unpack(path, &staged, installed, control, &mut |d, t| prog(d, t, "unpacking"), None, None)?;
        }
        if !s(&pack["revision"]).is_empty() {
            let meta = install::read_json(&lib.join("pack.json")).unwrap_or(Value::Null);
            if meta["content_revision"] != pack["revision"] {
                return Err(SetupError::refused("unpack", "The library content version does not match this download."));
            }
        }
        let current = self.current_info();
        if !plan["repair"].as_bool().unwrap_or(false) && plan["map"].is_null() && Some(PathBuf::from(s(&plan["target"]))) == self.o().current
            && current["pack"] == pack["id"] && !matches!(current["map"].as_str(), None | Some("base")) {
            install::copy_map(self.o().current.as_ref().unwrap(), &lib).map_err(|e| SetupError::refused("unpack", e.to_string()))?;
        }
        let files: Vec<Value> = pack["files"].as_array().into_iter().flatten().map(|f| f["name"].clone()).collect();
        let _ = install::write_json(&lib.join("setup-pack.json"), &json!({"pack": pack["id"], "installed": now(), "files": files}));
        for (path, ours) in got {
            if ours {
                let _ = std::fs::remove_file(path);
            }
        }
        Ok(())
    }

    /// A region map: download, check, unpack (map files only) into unpack-map/. With a pack, it replaces the staged
    /// library's own map set at once; on its own it is staged and switched in when the app restarts.
    fn do_map(&self, kind: &str, id: &str, work: &Path, control: &Control) -> Result<(), SetupError> {
        let plan = self.plan();
        let mp = plan["map"].clone();
        let mut got = Vec::new();
        let mut offset = 0;
        for f in mp["files"].as_array().cloned().unwrap_or_default() {
            got.push(self.get_file(kind, id, &f, work, control, offset)?);
            offset += u(&f["size"]);
        }
        let installed = mp["installed_bytes"].as_u64();
        self.set(kind, id, "unpacking", &[("bytes_done", json!(0)), ("bytes_total", json!(installed))]);
        let staged = work.join("unpack-map");
        let allowed: &dyn Fn(&str, bool) -> bool = &install::map_member;
        for (path, _) in &got {
            install::check_zip(path, installed, Some(allowed))?;
        }
        let mut prog = self.progress_fn(kind, id, 0);
        let mut lib = PathBuf::new();
        for (path, _) in &got {
            lib = install::unpack(path, &staged, installed, control, &mut |d, t| prog(d, t, "unpacking"), Some(allowed), None)?;
        }
        if install::map_name(&lib) != s(&mp["id"]) {
            let found = install::map_name(&lib);
            return Err(download::failed_with("unpack", json!({"found": found, "expected": mp["id"]}), format!("The map file says it is {found}, not {}.", s(&mp["id"]))));
        }
        if plan["switch"].as_bool().unwrap_or(false) {
            install::replace_map_in(&work.join("unpack").join(DATA_NAME), &lib)?;
            let _ = std::fs::remove_dir_all(&staged);
        } else {
            let l = lib.to_string_lossy().to_string();
            self.set_plan(|p| p["map_new"] = json!(l));
        }
        for (path, ours) in got {
            if ours {
                let _ = std::fs::remove_file(path);
            }
        }
        Ok(())
    }

    fn do_addresses(&self, kind: &str, id: &str, work: &Path, control: &Control) -> Result<(), SetupError> {
        let plan = self.plan();
        let entry = plan["addresses"].clone();
        let relative = format!("derived/geo/addresses/{}.sqlite", s(&entry["id"]));
        let dirs = ["", "derived", "derived/geo", "derived/geo/addresses"];
        let rel2 = relative.clone();
        let allowed = move |name: &str, is_dir: bool| if is_dir { dirs.contains(&name) } else { name == rel2 };
        let installed = u(&entry["installed_bytes"]);
        let rel3 = relative.clone();
        let verify = move |lib: &Path| -> Result<(), SetupError> {
            let p = lib.join(&rel3);
            if !p.is_file() || std::fs::metadata(&p).map(|m| m.len()).ok() != Some(installed) {
                return Err(SetupError::refused("unpack", "The address index is missing or the wrong size."));
            }
            Ok(())
        };
        let mut got = Vec::new();
        let mut offset = 0;
        for f in entry["files"].as_array().cloned().unwrap_or_default() {
            got.push(self.get_file(kind, id, &f, work, control, offset)?);
            offset += u(&f["size"]);
        }
        for (path, _) in &got {
            install::check_zip(path, Some(installed), Some(&allowed))?;
        }
        self.set(kind, id, "unpacking", &[("bytes_done", json!(0)), ("bytes_total", json!(installed))]);
        let mut prog = self.progress_fn(kind, id, 0);
        let mut lib = PathBuf::new();
        for (path, _) in &got {
            lib = install::unpack(path, &work.join("unpack-addresses"), Some(installed), control, &mut |d, t| prog(d, t, "unpacking"), Some(&allowed), Some(&verify))?;
        }
        let target = if plan["switch"].as_bool().unwrap_or(false) { work.join("unpack").join(DATA_NAME) } else { PathBuf::from(s(&plan["target"])) };
        let destination = target.join(&relative);
        std::fs::create_dir_all(destination.parent().unwrap()).map_err(|e| SetupError::refused("activate", e.to_string()))?;
        self.set(kind, id, "activating", &[]);
        std::fs::rename(lib.join(&relative), &destination).map_err(|e| SetupError::refused("activate", e.to_string()))?;
        let files: Map<String, Value> = entry["files"].as_array().into_iter().flatten().map(|f| (s(&f["name"]).to_string(), f["sha256"].clone())).collect();
        let _ = install::write_json(&destination.with_extension("json"), &json!({"files": files}));
        for (path, ours) in got {
            if ours {
                let _ = std::fs::remove_file(path);
            }
        }
        Ok(())
    }

    fn do_model(&self, kind: &str, id: &str, work: &Path, control: &Control) -> Result<(), SetupError> {
        let plan = self.plan();
        let m = plan["model"].clone();
        let f = json!({"name": m["file"], "url": m["url"], "size": m["size"], "sha256": m["sha256"]});
        let (path, ours) = self.get_file(kind, id, &f, work, control, 0)?;
        let switch = plan["switch"].as_bool().unwrap_or(false);
        let lib = if switch { work.join("unpack").join(DATA_NAME) } else { self.o().current.clone().unwrap_or(PathBuf::from(s(&plan["target"]))) };
        let on_computer = plan["model_dir"].as_str().map(PathBuf::from); // this computer's own folder, whatever the library does
        let dest = on_computer.clone().unwrap_or_else(|| lib.join("models")).join(s(&m["file"]));
        std::fs::create_dir_all(dest.parent().unwrap()).map_err(|e| SetupError::refused("activate", e.to_string()))?;
        self.set(kind, id, "activating", &[]);
        if ours || hardware::same_disk(&path, &dest) {
            std::fs::rename(&path, &dest).map_err(|e| SetupError::refused("activate", e.to_string()))?;
        } else {
            let tmp = dest.with_file_name(format!("{}.partial", s(&m["file"])));
            std::fs::copy(&path, &tmp).and_then(|_| std::fs::rename(&tmp, &dest)).map_err(|e| SetupError::refused("activate", e.to_string()))?;
        }
        let model_file = if switch && on_computer.is_none() { PathBuf::from(s(&plan["target"])).join("models").join(s(&m["file"])) } else { dest.clone() };
        let sha = m["sha256"].clone();
        self.set_plan(|p| {
            p["model_file"] = json!(model_file.to_string_lossy());
            p["model_sha256"] = sha; // what get_file just matched
        });
        if (!switch || on_computer.is_some()) && self.o().current.is_some() {
            if let Some(ai) = &self.o().ai {
                ai.mark_checked(&dest, s(&m["sha256"]));
                ai.rediscover();
            }
        }
        Ok(())
    }

    fn finish_job(&self, work: &Path) -> Result<(), SetupError> {
        let plan = self.plan();
        let pend = pending_file(&self.o().settings_dir);
        if plan["switch"].as_bool().unwrap_or(false) {
            let lib = work.join("unpack").join(DATA_NAME);
            if !install::is_library(&lib) {
                return Err(SetupError::refused("activate", "The new library is not complete."));
            }
            install::stage_switch(&pend, &lib, Path::new(s(&plan["target"])), plan["model_file"].as_str(), None, plan["model_sha256"].as_str())
                .map_err(|e| SetupError::refused("activate", e.to_string()))?;
        } else if let Some(new) = plan["map_new"].as_str() {
            install::stage_switch(&pend, Path::new(new), Path::new(s(&plan["target"])), None, Some("map"), None).map_err(|e| SetupError::refused("activate", e.to_string()))?;
        }
        self.with_job(|j| {
            j["state"] = json!("done");
            for it in j["items"].as_array_mut().into_iter().flatten() {
                it["rate_bps"] = json!(0);
                it["eta_s"] = Value::Null;
            }
        });
        Ok(())
    }

    /// Answers one /api/setup/ request: (HTTP status, JSON).
    pub fn handle(&self, method: &str, name: &str, body: &Value) -> (u16, Value) {
        let opt = |k: &str| body[k].as_str().filter(|v| !v.is_empty());
        let out = match (method, name) {
            ("GET" | "HEAD", "state") => self.state(),
            ("GET" | "HEAD", "job") => self.public_job(),
            ("POST", "start") => {
                // model: an id; ai: true for the suggested one (as the window sent before the model choice)
                let model = opt("model").map(str::to_string).or(if body["ai"].as_bool().unwrap_or(false) { Some(String::new()) } else { None });
                self.start_with(opt("pack"), model.as_deref(), opt("folder"), Map::new(), false, body["repair"].as_bool().unwrap_or(false), opt("map"),
                                opt("address_id"), opt("model_place"))
            }
            ("POST", "pause") => self.pause(),
            ("POST", "resume") => self.resume(),
            ("POST", "cancel") => self.cancel(),
            ("POST", "retry") => self.retry(),
            ("POST", "use") => self.use_path(body["path"].as_str().unwrap_or("")),
            ("POST", "finish") => self.finish(),
            ("POST", "pick") => self.pick(body["files"].as_bool() == Some(true)),
            ("POST", "delete-old") => self.delete_old(body["path"].as_str().unwrap_or("")),
            ("POST", "remove-ai") => self.remove_ai(opt("model")),
            _ => return (404, json!({"error": "not found"})),
        };
        let status = match (out.get("error"), out["code"].as_str()) {
            (Some(_), Some("busy" | "not_done")) => 409,
            (Some(_), Some("no_space")) => 507,
            (Some(_), Some(_)) => 400,
            _ => 200,
        };
        (status, out)
    }
}

fn expand(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(p)
}

fn tempfile_in(dir: &Path) -> std::io::Result<()> {
    let p = dir.join(format!(".lastbook-write-test-{}", std::process::id()));
    std::fs::write(&p, b"")?;
    std::fs::remove_file(&p)
}

/// The app calls this first: a staged switch is made now, before the engine opens anything. Returns the library to open
/// (or None to look as usual) and a note: {target, old, model_file, error}.
pub fn startup(settings_dir: &Path, only: Option<&Path>) -> (Option<PathBuf>, Option<Map<String, Value>>) {
    let Some(st) = install::read_json(&settings_dir.join("setup.json")) else { return (None, None) };
    let Some(work) = st["work"].as_str().map(PathBuf::from) else { return (None, None) };
    if let Some(only) = only {
        if !install::same_path(&work, &work_for(only)) {
            return (None, None); // opened with --data: a switch staged for another library waits for a normal start
        }
    }
    install::apply_pending(&pending_file(settings_dir), &work)
}

/// The library opened: record the old one for deleting later, tell the AI its model was checked, clear the work folder.
pub fn opened(settings_dir: &Path, note: Option<&Map<String, Value>>, ai: Option<&dyn AiHooks>) {
    let mut st = install::read_json(&settings_dir.join("setup.json")).filter(|v| v.is_object()).unwrap_or_else(|| json!({}));
    let work = st["work"].as_str().map(PathBuf::from);
    if let (Some(note), Some(ai)) = (note, ai) {
        if let (Some(mf), Some(sha)) = (note.get("model_file").and_then(|v| v.as_str()), note.get("model_sha256").and_then(|v| v.as_str())) {
            if Path::new(mf).is_file() {
                ai.mark_checked(Path::new(mf), sha);
            }
        }
    }
    if let Some(old) = note.and_then(|n| n.get("old")).and_then(|v| v.as_str()) {
        let olds = st["old"].as_array().cloned().unwrap_or_default();
        if !olds.iter().any(|o| o.as_str() == Some(old)) {
            let mut v = olds;
            v.push(json!(old));
            st["old"] = Value::Array(v);
        }
    }
    if let Some(work) = work {
        install::confirm(&pending_file(settings_dir));
        let done = install::read_json(&job_file(settings_dir)).is_some_and(|j| s(&j["state"]) == "done");
        if note.is_some() || done {
            let _ = std::fs::remove_dir_all(&work);
            st.as_object_mut().map(|o| o.remove("work"));
            let _ = std::fs::remove_file(job_file(settings_dir));
        }
    }
    let olds: Vec<Value> = st["old"].as_array().into_iter().flatten().filter(|o| Path::new(s(o)).exists()).cloned().collect();
    st["old"] = Value::Array(olds);
    let _ = std::fs::create_dir_all(settings_dir);
    let _ = install::write_json(&settings_dir.join("setup.json"), &st);
}
