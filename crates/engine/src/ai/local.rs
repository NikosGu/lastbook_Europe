//! The local AI on this computer: which model file, whether it is the one published, whether it fits, and the runner
//! process (lastbook-llm, which speaks only through pipes) that answers. Ported from the original Lastbook's llm_local.py,
//! with a catalogue of models instead of one, and a third answer besides "fits" and "too big": "from the disk", for a
//! mixture of experts larger than the memory, offered only as the person's explicit choice.
//!
//! Nothing here opens a socket. The model is a file setup downloaded or the person already had; Lastbook finds it in the
//! open library's `models/` folder (or a folder named by LASTBOOK_MODEL_DIR), checks its SHA-256 once, asks llama.cpp
//! how it fits, and starts the runner when someone asks a question. The runner stops after IDLE_STOP unused, when turned
//! off, and when the app quits (it leaves when its standard input closes).
//!
//! States: off, absent, wrong, checking (pct), measuring, too_big, from_disk (fits only from the disk: waiting for the
//! person's choice), stopped (ready to start), loading (pct), ready, error.
//!
//! What it says (a state's message, why a model fits or not, why it cannot answer) is a code with params (CODES), which the
//! interface words in the person's language (web/locales: chat.code.<code>); the English sentence stays beside them, for logs
//! and for an interface that does not know the code.

use super::format::{Family, Seg};
use super::gguf;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Tokens the model holds: the instructions, the sources, the conversation and the answer.
pub const CTX: u32 = 16384;
const START_WAIT: Duration = Duration::from_secs(600);
/// Below this, reading a model's experts from the disk is too slow to be worth offering (a spinning disk, a slow USB stick).
const DISK_MB_S_MIN: f64 = 150.0;

/// What the local AI can say, each a code the interface words in the person's language (web/locales: chat.code.<code>).
pub const CODES: [&str; 29] = [
    // a state's message, and why the model cannot answer now (ensure_running, generate)
    "no_runtime", "unfinished", "wrong_size", "wrong_contents", "cannot_read", "unknown_model", "cannot_measure", "not_set_up",
    "cannot_start", "did_not_start", "start_timeout", "start_failed", "not_running", "busy", "stopped_unexpectedly",
    // the runner's own errors
    "too_long", "load", "context", "model_file", "failed",
    // why a model fits this computer or not (judge, judge_disk, and setup's measure)
    "from_disk", "too_big", "too_big_graphics", "disk_too_slow", "disk_speed_unknown", "measured_fits", "from_disk_ssd",
    // a button the local AI cannot act on (action)
    "no_model", "unknown_action",
];

/// The system's reasons the interface words (web/locales: chat.code.kind.<kind>), after the sentence of cannot_read or
/// cannot_start: their params.kind, an error's kind as kind_code() writes it. Any other kind is said without its reason.
pub const KINDS: [&str; 6] = ["not_found", "permission_denied", "out_of_memory", "timed_out", "read_only_filesystem", "storage_full"];

/// The sentences without params: code, English.
const FIXED: [(&str, &str); 18] = [
    ("no_runtime", "This copy of Lastbook Europe has no local AI runtime."),
    ("unknown_model", "Lastbook Europe does not know how to ask this model questions."),
    ("cannot_measure", "The local AI program could not read the model file."),
    ("not_set_up", "The local AI is not set up on this computer."),
    ("did_not_start", "The local AI did not start."),
    ("start_timeout", "The local AI took too long to start."),
    ("start_failed", "The local AI could not start. Look again after closing other programs."),
    ("not_running", "The local AI is not running."),
    ("busy", "The local AI is busy."),
    ("stopped_unexpectedly", "The local AI stopped unexpectedly. Ask again to restart it."),
    ("too_long", "The question and its sources were too long for the local AI. Start a new chat or ask a shorter question."),
    ("load", "The local AI could not load the model file."),
    ("context", "There was not enough free memory to start the local AI. Close other programs and ask again."),
    ("model_file", "The local AI could not read the model file."),
    ("failed", "The local AI failed."),
    ("too_big_graphics", "The model does not fit this computer’s graphics memory together with its own memory."),
    ("disk_speed_unknown", "The model is larger than this computer’s memory, and the disk it is on reads too slowly (unknown speed) to run it from there."),
    ("measured_fits", "Measured on this computer: it fits."),
];

/// A sentence the local AI says: its code and params, and the English beside them.
#[derive(Clone, Debug, PartialEq)]
pub struct Said {
    pub code: String,
    pub params: Value,
    pub message: String,
}

impl Said {
    /// Puts it into an answer: its English as `english`, beside "code" and "params".
    fn put(self, out: &mut Value, english: &str) {
        out[english] = json!(self.message);
        out["code"] = json!(self.code);
        out["params"] = self.params;
    }

    /// One put into an answer before.
    fn of(v: &Value, english: &str) -> Option<Said> {
        Some(said(v["code"].as_str()?, v["params"].clone(), v[english].as_str()?))
    }
}

fn said(code: &str, params: Value, message: impl Into<String>) -> Said {
    Said { code: code.to_string(), params, message: message.into() }
}

/// A sentence without params, by its code.
fn fixed(code: &str) -> Said {
    let english = FIXED.iter().find(|(c, _)| *c == code).map(|(_, e)| *e);
    debug_assert!(english.is_some(), "{code} is not one of FIXED");
    said(code, json!({}), english.unwrap_or("The local AI failed."))
}

thread_local! {
    /// The last sentence ensure_running or generate answered this thread with.
    static TOLD: RefCell<Option<Said>> = const { RefCell::new(None) };
}

/// A failure's English, for the caller; its code is kept for told().
fn tell(s: Said) -> String {
    let message = s.message.clone();
    TOLD.with(|t| *t.borrow_mut() = Some(s));
    message
}

/// The code and params of `text`: a sentence ensure_running or generate just answered this thread with, or one of the
/// sentences without params (the assistant says some of them too). None for any other text. The chat (ai/chat.rs) adds
/// them to its error event, on the thread that asked.
pub fn told(text: &str) -> Option<Said> {
    TOLD.with(|t| t.borrow().clone()).filter(|s| s.message == text).or_else(|| FIXED.iter().find(|(_, e)| *e == text).map(|(c, e)| said(c, json!({}), *e)))
}

/// An error's kind as a code: permission_denied, not_found…
fn kind_code(kind: std::io::ErrorKind) -> String {
    let mut out = String::new();
    for c in format!("{kind:?}").chars() {
        if c.is_ascii_uppercase() && !out.is_empty() {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// A model the catalogue offers: from the setup manifest's `models`.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelEntry {
    pub id: String,
    pub title: String,
    pub file: String,
    pub size: u64,
    pub sha256: String,
}

impl ModelEntry {
    pub fn from_manifest(v: &Value) -> Option<ModelEntry> {
        let s = |k: &str| v[k].as_str().map(str::to_string);
        let file = s("file")?;
        if file.contains(['/', '\\']) || !file.ends_with(".gguf") {
            return None;
        }
        Some(ModelEntry { id: s("id")?, title: s("title").unwrap_or_else(|| file.clone()), file, size: v["size"].as_u64()?, sha256: s("sha256")?.to_lowercase() })
    }
}

/// The id under which a model file the person chose (not one of the catalogue's) is remembered.
pub const CUSTOM: &str = "custom";

/// Tokens a model is run with: CTX, or the number it was trained on when that is fewer (but not under 2048).
pub fn ctx_of(info: &gguf::Info) -> u32 {
    match info.context_length {
        0 => CTX,
        n => (n.min(CTX as u64) as u32).max(2048),
    }
}

/// Does the model fit this computer? From the runner's fit line and the model file's header.
/// fits: in memory. from_disk: only from the disk (a mixture of experts larger than the memory). too_big: neither.
/// Why, unless it fits: `why` (English) with its "code" and "params".
pub fn judge(fit: &Value, info: Option<&gguf::Info>) -> Value {
    let total = fit["ram_total"].as_u64().unwrap_or(0);
    let file = fit["file_bytes"].as_u64().unwrap_or(0);
    let moe = info.is_some_and(|i| i.is_moe()) || fit["experts_on_cpu"].as_bool().unwrap_or(false);
    let mut out = json!({"status": fit["status"], "ram_total": total, "file_bytes": file, "placement": fit["placement"],
                         "n_gpu_layers": fit["n_gpu_layers"], "needs": file + (5 << 29)});
    let sizes = json!({"file_bytes": file, "ram_bytes": total});
    let (verdict, why) = if fit["from_disk"].as_bool().unwrap_or(false) {
        if moe {
            ("from_disk", Some(said("from_disk", sizes, format!("The model is {}, more than this computer’s memory ({}). It can still run from the disk, much more slowly.", gb(file), gb(total)))))
        } else {
            ("too_big", Some(said("too_big", sizes, format!("The model needs about {} of this computer’s memory, plus room for everything else, and this computer has {}.", gb(file), gb(total)))))
        }
    } else if fit["status"] != "fits" {
        ("too_big", Some(fixed("too_big_graphics")))
    } else {
        ("fits", None)
    };
    out["verdict"] = json!(verdict);
    out["fits"] = json!(verdict == "fits");
    if let Some(why) = why {
        why.put(&mut out, "why");
    }
    out
}

/// Whether files can be made in a library's derived/ folder (not on a read-only share or stick).
fn writable(data: &Path) -> bool {
    let d = data.join("derived");
    let probe = d.join(format!(".write-check-{}", std::process::id()));
    let ok = std::fs::create_dir_all(&d).is_ok() && std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// judge()'s answer once the disk's reading speed is known (MB/s; None if it could not be measured), for a model that can
/// run only from the disk: on a disk too slow, it is too big.
pub fn judge_disk(mut judged: Value, speed: Option<f64>) -> Value {
    if judged["verdict"] == "from_disk" && speed.is_none_or(|s| s < DISK_MB_S_MIN) {
        judged["verdict"] = json!("too_big");
        let why = match speed {
            Some(s) => said("disk_too_slow", json!({"disk_mb_s": s.round() as u64}),
                            format!("The model is larger than this computer’s memory, and the disk it is on reads too slowly ({s:.0} MB/s) to run it from there.")),
            None => fixed("disk_speed_unknown"),
        };
        why.put(&mut judged, "why");
    }
    judged
}

/// judge()'s answer as setup shows it: {fits, from_disk, needs_bytes, estimate: false, why, code, params}.
pub fn for_setup(j: &Value) -> Value {
    let why = match j["verdict"].as_str() {
        Some("fits") => Some(fixed("measured_fits")),
        Some("from_disk") => Some(said("from_disk_ssd", j["params"].clone(), format!("{} Only as your choice, on an SSD.", j["why"].as_str().unwrap_or("")))),
        _ => Said::of(j, "why"),
    };
    let mut out = json!({"fits": j["verdict"] == "fits", "from_disk": j["verdict"] == "from_disk", "needs_bytes": j["needs"], "estimate": false, "why": ""});
    if let Some(why) = why {
        why.put(&mut out, "why");
    }
    out
}

pub fn gb(n: u64) -> String {
    format!("{:.1} GB", n as f64 / 1e9)
}

/// What the person reads when the runner reports an error: by its code (too_long, load, context, model_file), else failed,
/// with the runner's own English if it sent some.
fn plain_error(ev: &Value) -> Said {
    match ev["code"].as_str() {
        Some(c @ ("too_long" | "load" | "context" | "model_file")) => fixed(c),
        _ => match ev["msg"].as_str() {
            Some(m) => said("failed", json!({}), m),
            None => fixed("failed"),
        },
    }
}

/// The runner: beside this program (in a package, and in a build folder), or LASTBOOK_LLM.
pub fn runner_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("LASTBOOK_LLM").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    let exe = std::env::current_exe().ok()?;
    let name = if cfg!(windows) { "lastbook-llm.exe" } else { "lastbook-llm" };
    exe.ancestors().skip(1).take(2).map(|d| d.join(name)).find(|p| p.is_file()) // a test runs from target/debug/deps
}

/// Reading speed of the model file in MB/s: 32 reads of 1 MiB spread over the file, with the system's cache bypassed
/// where it allows (macOS), as the experts would be read while answering.
pub fn disk_speed(path: &Path) -> Option<f64> {
    let mut f = std::fs::File::open(path).ok()?;
    #[cfg(target_os = "macos")]
    unsafe {
        use std::os::unix::io::AsRawFd;
        libc::fcntl(f.as_raw_fd(), libc::F_NOCACHE, 1);
    }
    let size = f.metadata().ok()?.len();
    const PIECE: u64 = 1 << 20;
    if size < PIECE * 4 {
        return None;
    }
    let mut buf = vec![0u8; PIECE as usize];
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15 ^ size;
    let t = Instant::now();
    let mut read = 0u64;
    for _ in 0..32 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let at = (x % (size - PIECE)) & !4095;
        f.seek(SeekFrom::Start(at)).ok()?;
        f.read_exact(&mut buf).ok()?;
        read += PIECE;
    }
    Some(read as f64 / 1e6 / t.elapsed().as_secs_f64().max(1e-6))
}

struct Proc {
    serial: u64,
    child: Child,
    stdin: ChildStdin,
}

struct St {
    state: String,
    pct: Option<u64>,
    /// The state's message, if it has one.
    said: Option<Said>,
    model: Option<ModelEntry>,
    path: Option<PathBuf>,
    family: Option<Family>,
    fit: Option<Value>,
    ready: Option<Value>,
    disk_mb_s: Option<f64>,
    proc: Option<Proc>,
    events: Option<Receiver<Value>>,
    spawned: Instant,
    last_used: Instant,
    next_id: u64,
    failed_loads: u32,
    found: Vec<(ModelEntry, PathBuf)>,
    /// Tokens the model is run with: CTX, or less for a model trained on fewer.
    ctx: u32,
}

struct Inner {
    data: PathBuf,
    catalogue: Box<dyn Fn() -> Vec<Value> + Send + Sync>,
    runner: Option<PathBuf>,
    st: Mutex<St>,
    saved: Mutex<Value>,
    gen_lock: Mutex<()>,
    started: AtomicBool,
    serial: AtomicU64,
    idle_stop: Duration,
    start_wait: Duration,
    /// More folders to look in for model files: the app's own on this computer.
    more_places: Mutex<Vec<PathBuf>>,
    /// A folder of this computer's own for what the AI writes when the library's folder cannot be written.
    own: Mutex<Option<PathBuf>>,
}

/// One model process at a time, for one library.
#[derive(Clone)]
pub struct Local(Arc<Inner>);

impl Local {
    /// data: the open library. catalogue: the models setup offers (the manifest's `models`).
    pub fn new(data: PathBuf, catalogue: Box<dyn Fn() -> Vec<Value> + Send + Sync>, runner: Option<PathBuf>) -> Local {
        let saved = crate::setup::install::read_json(&data.join("derived").join("llm").join("state.json")).filter(Value::is_object).unwrap_or_else(|| json!({}));
        let idle = std::env::var("LASTBOOK_AI_IDLE_S").ok().and_then(|v| v.parse().ok()).unwrap_or(15 * 60);
        Local(Arc::new(Inner {
            data,
            catalogue,
            runner,
            st: Mutex::new(St { state: "absent".into(), pct: None, said: None, model: None, path: None, family: None, fit: None,
                                ready: None, disk_mb_s: None, proc: None, events: None, spawned: Instant::now(), last_used: Instant::now(),
                                next_id: 0, failed_loads: 0, found: Vec::new(), ctx: CTX }),
            saved: Mutex::new(saved),
            gen_lock: Mutex::new(()),
            started: AtomicBool::new(false),
            serial: AtomicU64::new(0),
            idle_stop: Duration::from_secs(idle),
            start_wait: START_WAIT,
            more_places: Mutex::new(Vec::new()),
            own: Mutex::new(None),
        }))
    }

    /// A folder of this computer's own (in the app's data folder) for what the AI writes, its choices and its log, when the
    /// library's folder cannot be written: a library on a read-only network share or USB stick keeps its local AI.
    pub fn with_own_folder(self, dir: PathBuf) -> Local {
        let state = dir.join("derived").join("llm").join("state.json");
        let own = crate::setup::install::read_json(&state).filter(Value::is_object);
        if !writable(&self.0.data) {
            if let Some(s) = own {
                *self.0.saved.lock().unwrap_or_else(|e| e.into_inner()) = s;
            }
        } else if let Some(own) = own.filter(|o| o["model"] == CUSTOM && o["custom"].is_object()) {
            // a model file chosen in the first setup, before there was a library (setup's AI kept it in this folder)
            let carried = {
                let mut saved = self.0.saved.lock().unwrap_or_else(|e| e.into_inner());
                let free = saved["model"].is_null();
                if free {
                    saved["model"] = own["model"].clone();
                    saved["custom"] = own["custom"].clone();
                }
                free
            };
            if carried {
                self.save();
            }
        }
        *self.0.own.lock().unwrap_or_else(|e| e.into_inner()) = Some(dir);
        self
    }

    /// Where the AI writes: beside the library, or in its own folder when the library's cannot be written.
    fn write_base(&self) -> PathBuf {
        match self.0.own.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            Some(own) if !writable(&self.0.data) => own,
            _ => self.0.data.clone(),
        }
    }

    fn st(&self) -> std::sync::MutexGuard<'_, St> {
        self.0.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn save(&self) {
        let p = self.write_base().join("derived").join("llm").join("state.json");
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        if let Err(e) = crate::setup::install::write_json(&p, &self.0.saved.lock().unwrap()) {
            eprintln!("local AI: could not save its state: {e}");
        }
    }

    fn saved(&self, key: &str) -> Value {
        self.0.saved.lock().unwrap()[key].clone()
    }

    fn set_saved(&self, key: &str, v: Value) {
        self.0.saved.lock().unwrap()[key] = v;
        self.save();
    }

    pub fn enabled(&self) -> bool {
        self.saved("enabled").as_bool().unwrap_or(true)
    }

    /// The folders searched for model files: LASTBOOK_MODEL_DIR, the library's models/ (wherever the library is: this
    /// computer, an external disk, a network share), the library itself, and the app's own model folder on this computer.
    pub fn places(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = std::env::var_os("LASTBOOK_MODEL_DIR").map(|v| std::env::split_paths(&v).collect()).unwrap_or_default();
        out.push(self.0.data.join("models"));
        out.push(self.0.data.clone());
        out.extend(self.0.more_places.lock().unwrap().iter().cloned());
        out
    }

    /// Another folder to look in for model files (the app's own on this computer).
    pub fn add_place(&self, folder: PathBuf) {
        self.0.more_places.lock().unwrap().push(folder);
    }

    /// Answering, or able to start when asked.
    pub fn available(&self) -> bool {
        matches!(self.st().state.as_str(), "ready" | "stopped" | "loading")
    }

    pub fn family(&self) -> Option<Family> {
        self.st().family.clone()
    }

    /// Tokens the model holds: the prompt and the answer together.
    pub fn ctx(&self) -> u32 {
        self.st().ctx
    }

    /// A model file the person already has, wherever it is (another program's models folder, a download): used where it is,
    /// not copied. It needs to be a model one can talk to (Family::of): the instructions-following kind, with its chat
    /// template. Ok: what it is; Err: (code, English sentence).
    pub fn use_file(&self, path: &Path) -> Result<Value, (String, String)> {
        let info = gguf::read(path).map_err(|e| ("not_a_model".to_string(), format!("That is not a model file Lastbook Europe can read ({e}).")))?;
        let family = Family::of(&info).ok_or_else(|| ("unknown_model".to_string(), "Lastbook Europe does not know how to ask this model questions: it has no chat template (a base model, or not a language model).".to_string()))?;
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let title = if info.name.trim().is_empty() { path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() } else { info.name.trim().to_string() };
        self.set_saved("custom", json!({"path": path.to_string_lossy(), "size": size, "title": title}));
        self.set_saved("model", json!(CUSTOM));
        self.set_saved("enabled", json!(true));
        self.stop("another model chosen");
        {
            let mut st = self.st();
            (st.state, st.said) = ("absent".into(), None);
        }
        self.rediscover();
        Ok(json!({"title": title, "size": size, "family": family.id(), "tools": family.tools(), "path": path.to_string_lossy()}))
    }

    /// The state for the interface. A state with a message carries its "code" and "params"; `why` (how the model fits, unless
    /// it fits) carries "why_code" and "why_params".
    pub fn status(&self) -> Value {
        let st = self.st();
        let fit = st.fit.clone().unwrap_or(Value::Null);
        let mut out = json!({"name": st.model.as_ref().map(|m| m.title.clone()), "model": st.model.as_ref().map(|m| m.id.clone()),
               "file": st.model.as_ref().map(|m| m.file.clone()), "size": st.model.as_ref().map(|m| m.size),
               "state": st.state, "pct": st.pct, "message": st.said.as_ref().map(|s| s.message.as_str()).unwrap_or(""),
               "found_at": st.path.as_deref().map(folder_name),
               "enabled": self.enabled(), "runtime": self.0.runner.is_some(), "needs_bytes": fit["needs"], "ram_total": fit["ram_total"],
               "placement": fit["placement"], "verdict": fit["verdict"], "why": fit["why"], "why_code": fit["code"], "why_params": fit["params"],
               "disk_mb_s": st.disk_mb_s.map(|v| v.round()),
               "from_disk_chosen": st.model.as_ref().is_some_and(|m| self.from_disk_chosen(&m.id)),
               "load_s": st.ready.as_ref().and_then(|r| r["load_s"].as_f64()),
               "models": st.found.iter().map(|(m, _)| json!({"id": m.id, "title": m.title, "size": m.size})).collect::<Vec<_>>(),
               "places": self.places().iter().take(6).map(|p| folder_name(p)).collect::<Vec<_>>()});
        if let Some(s) = &st.said {
            out["code"] = json!(s.code);
            out["params"] = s.params.clone();
        }
        out
    }

    fn from_disk_chosen(&self, id: &str) -> bool {
        self.saved("from_disk")[id].as_bool().unwrap_or(false)
    }

    /// Finding, checking and measuring run on their own thread; an idle watch stops the model when unused.
    pub fn start_background(&self) {
        if self.0.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = self.clone();
        std::thread::Builder::new().name("local-ai-find".into()).spawn(move || me.discover()).ok();
        let me = self.clone();
        std::thread::Builder::new().name("local-ai-idle".into()).spawn(move || loop {
            std::thread::sleep((me.0.idle_stop / 4).min(Duration::from_secs(30)).max(Duration::from_millis(200)));
            let idle = {
                let st = me.st();
                st.state == "ready" && me.0.gen_lock.try_lock().is_ok() && st.last_used.elapsed() > me.0.idle_stop
            };
            if idle {
                me.stop("not used for a while");
            }
        }).ok();
    }

    /// Find a model file, check it if it has not been checked as it is now, measure how it fits. Safe to call again.
    pub fn discover(&self) {
        {
            let mut st = self.st();
            if matches!(st.state.as_str(), "checking" | "measuring" | "loading" | "ready") {
                return;
            }
            if !self.enabled() {
                st.state = "off".into();
                st.said = None;
                return;
            }
            if self.0.runner.is_none() {
                st.state = "error".into();
                st.said = Some(fixed("no_runtime"));
                return;
            }
            (st.state, st.pct, st.said) = ("absent".into(), None, None);
        }
        let catalogue: Vec<ModelEntry> = (self.0.catalogue)().iter().filter_map(ModelEntry::from_manifest).collect();
        let (mut found, mut wrong): (Vec<(ModelEntry, PathBuf)>, Option<(PathBuf, u64, u64)>) = (Vec::new(), None);
        for folder in self.places() {
            for m in &catalogue {
                let p = folder.join(&m.file);
                let Ok(md) = std::fs::metadata(&p) else { continue };
                if !md.is_file() || found.iter().any(|(f, _)| f.id == m.id) {
                    continue;
                }
                if md.len() == m.size {
                    found.push((m.clone(), p));
                } else if wrong.is_none() {
                    wrong = Some((p, md.len(), m.size));
                }
            }
        }
        // a file the person chose, where it is (Local::use_file): offered as long as it is there, at its size
        let custom = self.saved("custom");
        if let (Some(p), Some(size)) = (custom["path"].as_str(), custom["size"].as_u64()) {
            let p = PathBuf::from(p);
            if std::fs::metadata(&p).is_ok_and(|m| m.is_file() && m.len() == size) {
                let file = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let title = custom["title"].as_str().map(str::to_string).unwrap_or_else(|| file.clone());
                found.push((ModelEntry { id: CUSTOM.into(), title, file, size, sha256: String::new() }, p));
            }
        }
        // the one chosen before, else the largest here: the person can choose another
        found.sort_by_key(|(m, _)| std::cmp::Reverse(m.size));
        let chosen = self.saved("model").as_str().and_then(|id| found.iter().position(|(m, _)| m.id == id)).unwrap_or(0);
        self.st().found = found.clone();
        let Some((model, path)) = found.get(chosen).cloned() else {
            let mut st = self.st();
            if let Some((p, size, want)) = wrong {
                st.state = "wrong".into();
                let short = size < want;
                st.said = Some(said(if short { "unfinished" } else { "wrong_size" }, json!({"folder": folder_param(&p), "size_bytes": size, "expected_bytes": want}),
                                    format!("The file in {} is {} of {}. {}", folder_name(&p), gb(size), gb(want),
                                            if short { "The download may not have finished." } else { "It is not the file Lastbook Europe expects." })));
            } else {
                st.state = "absent".into();
            }
            return;
        };
        let stamp = stamp(&path);
        // a catalogue model is checked against its SHA-256 once; a file the person chose has none to check against
        if !model.sha256.is_empty() && self.saved("checked")[&model.id] != stamp {
            if !self.check(&model, &path) {
                return;
            }
            let mut checked = self.saved("checked");
            if !checked.is_object() {
                checked = json!({});
            }
            checked[&model.id] = stamp;
            self.set_saved("checked", checked);
        }
        let info = gguf::read(&path).ok();
        {
            let mut st = self.st();
            st.family = info.as_ref().and_then(Family::of);
            st.ctx = info.as_ref().map(ctx_of).unwrap_or(CTX);
            if st.family.is_none() {
                st.state = "error".into();
                st.said = Some(fixed("unknown_model"));
                return;
            }
            st.model = Some(model.clone());
            st.path = Some(path.clone());
        }
        self.measure(&model, &path, info.as_ref());
    }

    /// Setup has just checked this file's SHA-256 while downloading it: it need not be read again.
    pub fn mark_checked(&self, path: &Path, sha256: &str) {
        let catalogue: Vec<ModelEntry> = (self.0.catalogue)().iter().filter_map(ModelEntry::from_manifest).collect();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        if let Some(m) = catalogue.iter().find(|m| m.file == name && m.size == size && m.sha256 == sha256.to_lowercase()) {
            let mut checked = self.saved("checked");
            if !checked.is_object() {
                checked = json!({});
            }
            checked[&m.id] = stamp(path);
            self.set_saved("checked", checked);
        }
    }

    fn check(&self, model: &ModelEntry, path: &Path) -> bool {
        {
            let mut st = self.st();
            (st.state, st.pct, st.said) = ("checking".into(), Some(0), None);
        }
        let result = (|| -> std::io::Result<String> {
            let mut f = std::fs::File::open(path)?;
            let mut h = Sha256::new();
            let mut buf = vec![0u8; 8 << 20];
            let mut done = 0u64;
            loop {
                let n = f.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
                done += n as u64;
                self.st().pct = Some(done * 100 / model.size.max(1));
            }
            Ok(hex::encode(h.finalize()))
        })();
        let mut st = self.st();
        match result {
            Ok(d) if d == model.sha256 => true,
            Ok(_) => {
                (st.state, st.pct) = ("wrong".into(), None);
                st.said = Some(said("wrong_contents", json!({"folder": folder_param(path)}),
                                    format!("The file in {} has the right size but different contents. Download it again.", folder_name(path))));
                false
            }
            Err(e) => {
                (st.state, st.pct) = ("wrong".into(), None);
                st.said = Some(said("cannot_read", json!({"kind": kind_code(e.kind())}), format!("Lastbook Europe could not read the model file ({}).", e.kind())));
                false
            }
        }
    }

    fn argv(&self, path: &Path, extra: &[&str]) -> Command {
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(8);
        let threads = (threads / 2).clamp(4, 16); // about the number of performance cores; more only contend
        let mut c = Command::new(self.0.runner.as_ref().expect("runner"));
        let ctx = gguf::read(path).map(|i| ctx_of(&i)).unwrap_or(CTX);
        c.arg("--model").arg(path).args(["--ctx", &ctx.to_string(), "--threads", &threads.to_string()]).args(extra);
        quiet(&mut c);
        c
    }

    /// llama.cpp's fit decides where each part goes; then: in memory, from the disk, or too big.
    fn measure(&self, model: &ModelEntry, path: &Path, info: Option<&gguf::Info>) {
        {
            let mut st = self.st();
            (st.state, st.pct, st.said) = ("measuring".into(), None, None);
        }
        let out = self.argv(path, &["--measure"]).stdin(Stdio::null()).stderr(Stdio::piped()).output();
        let fit = out.as_ref().ok().and_then(|o| {
            String::from_utf8_lossy(&o.stdout).lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).find(|v| v["ev"] == "fit")
        });
        let Some(fit) = fit else {
            if let Ok(o) = &out {
                eprintln!("local AI: measuring gave no fit: {}", String::from_utf8_lossy(&o.stdout));
            }
            let mut st = self.st();
            st.state = "error".into();
            st.said = Some(fixed("cannot_measure"));
            return;
        };
        let judged = judge(&fit, info);
        let speed = if judged["verdict"] == "from_disk" { disk_speed(path) } else { None };
        self.measured(model, judge_disk(judged, speed), speed);
    }

    /// What measuring found, as the state: stopped (ready to start), from_disk (waiting for the person's choice), too_big.
    fn measured(&self, model: &ModelEntry, judged: Value, speed: Option<f64>) {
        eprintln!("local AI: fit {judged}");
        let mut st = self.st();
        st.disk_mb_s = speed;
        st.state = match judged["verdict"].as_str() {
            Some("fits") => "stopped".into(),
            Some("from_disk") if self.from_disk_chosen(&model.id) => "stopped".into(),
            Some("from_disk") => "from_disk".into(),
            _ => "too_big".into(),
        };
        st.said = if st.state == "too_big" { Said::of(&judged, "why") } else { None };
        st.fit = Some(judged);
    }

    /// Starts the model if it is stopped and waits until it is ready: None, or a sentence saying why not (its code: told()).
    pub fn ensure_running(&self, on_wait: &mut dyn FnMut(Option<u64>), cancelled: &dyn Fn() -> bool) -> Option<String> {
        {
            let mut st = self.st();
            if st.state == "ready" && st.proc.is_some() {
                st.last_used = Instant::now();
                return None;
            }
            if !matches!(st.state.as_str(), "stopped" | "loading" | "error") || st.path.is_none() || st.fit.is_none() {
                return Some(tell(st.said.clone().unwrap_or_else(|| fixed("not_set_up"))));
            }
            if st.state == "error" && !st.fit.as_ref().is_some_and(|f| f["verdict"] == "fits" || f["verdict"] == "from_disk") {
                return Some(st.said.clone().map(tell).unwrap_or_default());
            }
            if st.state != "loading" {
                if let Err(e) = self.spawn(&mut st) {
                    let s = said("cannot_start", json!({"kind": kind_code(e.kind())}), format!("The local AI could not start ({e})."));
                    st.state = "error".into();
                    st.said = Some(s.clone());
                    return Some(tell(s));
                }
            }
        }
        let t0 = Instant::now();
        while t0.elapsed() < self.0.start_wait {
            {
                let mut st = self.st();
                if st.state == "ready" {
                    st.last_used = Instant::now();
                    return None;
                }
                if st.state != "loading" {
                    return Some(tell(st.said.clone().unwrap_or_else(|| fixed("did_not_start"))));
                }
            }
            if cancelled() {
                return Some("stopped".into());
            }
            on_wait(self.st().pct);
            std::thread::sleep(Duration::from_millis(250));
        }
        self.stop("took too long to start");
        Some(tell(fixed("start_timeout")))
    }

    fn spawn(&self, st: &mut St) -> std::io::Result<()> {
        let path = st.path.clone().expect("path");
        let logs = self.write_base().join("derived").join("logs");
        std::fs::create_dir_all(&logs)?;
        let mut log = std::fs::OpenOptions::new().create(true).append(true).open(logs.join("local-ai.log"))?;
        let _ = writeln!(log, "\n--- {} start", crate::setup::api::now());
        // a model on a network share or an external disk is read once into memory: mapped, every page it needs later would
        // travel again (and a share can go away); from the disk, it has to stay mapped
        let mapped = crate::setup::volumes::kind(&path) == "local" || st.fit.as_ref().is_some_and(|f| f["verdict"] == "from_disk");
        let mut child = self.argv(&path, if mapped { &[] } else { &["--no-mmap"] }).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::from(log)).spawn()?;
        tie_to_this_process(&mut child);
        let stdout = child.stdout.take().expect("stdout");
        let stdin = child.stdin.take().expect("stdin");
        let serial = self.0.serial.fetch_add(1, Ordering::SeqCst) + 1;
        let (tx, rx) = channel();
        (st.state, st.pct, st.said) = ("loading".into(), Some(0), None);
        st.proc = Some(Proc { serial, child, stdin });
        st.events = Some(rx);
        st.spawned = Instant::now();
        let me = self.clone();
        std::thread::Builder::new().name("local-ai-read".into()).spawn(move || me.read(serial, stdout, tx))?;
        Ok(())
    }

    fn read(&self, serial: u64, stdout: std::process::ChildStdout, tx: Sender<Value>) {
        let mine = |st: &St| st.proc.as_ref().is_some_and(|p| p.serial == serial);
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            let Ok(ev) = serde_json::from_str::<Value>(&line) else { continue };
            match ev["ev"].as_str() {
                Some("progress") => {
                    let mut st = self.st();
                    if mine(&st) {
                        st.pct = ev["p"].as_u64();
                    }
                }
                Some("ready") => {
                    let mut st = self.st();
                    if mine(&st) {
                        (st.state, st.pct, st.failed_loads) = ("ready".into(), None, 0);
                        let mut info = ev.clone();
                        info["load_s"] = json!((st.spawned.elapsed().as_secs_f64() * 10.0).round() / 10.0);
                        eprintln!("local AI: ready {info}");
                        st.ready = Some(info);
                        st.last_used = Instant::now();
                    }
                }
                Some("fit") => {}
                Some("error") if ev["id"].is_null() => {
                    let mut st = self.st();
                    if mine(&st) && st.state == "loading" {
                        st.state = "error".into();
                        st.said = Some(plain_error(&ev));
                    }
                    let _ = tx.send(ev);
                }
                _ => {
                    let _ = tx.send(ev);
                }
            }
        }
        let mut st = self.st();
        let code = if mine(&st) { st.proc.as_mut().and_then(|p| p.child.wait().ok()).and_then(|s| s.code()) } else { None };
        let _ = tx.send(json!({"ev": "exit", "code": code}));
        if mine(&st) {
            st.proc = None;
            eprintln!("local AI: the program ended ({code:?})");
            if st.state == "ready" {
                (st.state, st.said) = ("stopped".into(), None); // the next question starts it again
            } else if st.state == "loading" || st.state == "error" {
                st.failed_loads += 1;
                if st.failed_loads < 2 && st.state == "loading" {
                    (st.state, st.said) = ("stopped".into(), None); // once may be chance
                } else {
                    st.state = "error".into();
                    if st.said.is_none() {
                        st.said = Some(fixed("start_failed"));
                    }
                }
            }
        }
    }

    pub fn stop(&self, why: &str) {
        let proc = {
            let mut st = self.st();
            let proc = st.proc.take();
            if matches!(st.state.as_str(), "ready" | "loading" | "error") && st.path.is_some() && st.fit.as_ref().is_some_and(|f| f["verdict"] != "too_big") {
                (st.state, st.pct, st.said) = ("stopped".into(), None, None);
            }
            proc
        };
        if let Some(mut p) = proc {
            eprintln!("local AI: stopping ({why})");
            let _ = p.stdin.write_all(b"{\"op\":\"quit\"}\n");
            drop(p.stdin);
            let t = Instant::now();
            while t.elapsed() < Duration::from_secs(10) {
                if let Ok(Some(_)) = p.child.try_wait() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = p.child.kill();
            let _ = p.child.wait();
        }
    }

    /// The local AI page's buttons: on, off, stop (Stop it now: it starts again when asked), rescan (Look again),
    /// from_disk_on / from_disk_off (the person's choice to run a model too large for the memory from the disk), and use:<id>
    /// (another model found here). The status after it, or what refused it: {error, code, params}.
    pub fn action(&self, action: &str) -> Value {
        let refused = |s: Said| {
            let mut out = json!({});
            s.put(&mut out, "error");
            out
        };
        match action {
            "on" => {
                self.set_saved("enabled", json!(true));
                if self.st().state == "off" {
                    self.st().state = "absent".into();
                }
                self.rediscover();
            }
            "off" => {
                self.set_saved("enabled", json!(false));
                self.stop("turned off");
                let mut st = self.st();
                (st.state, st.said) = ("off".into(), None);
            }
            "stop" => self.stop("stopped from About"),
            "rescan" => self.rediscover(),
            "from_disk_on" | "from_disk_off" => {
                let Some(id) = self.st().model.as_ref().map(|m| m.id.clone()) else { return refused(said("no_model", json!({}), "no model")) };
                let mut choice = self.saved("from_disk");
                if !choice.is_object() {
                    choice = json!({});
                }
                choice[&id] = json!(action == "from_disk_on");
                self.set_saved("from_disk", choice);
                let verdict = self.st().fit.as_ref().map(|f| f["verdict"].clone()).unwrap_or(Value::Null);
                if verdict == "from_disk" {
                    if action == "from_disk_on" {
                        self.st().state = "stopped".into();
                    } else {
                        self.stop("from the disk turned off");
                        self.st().state = "from_disk".into();
                    }
                }
            }
            a if a.starts_with("use:") => {
                self.set_saved("model", json!(&a[4..]));
                self.stop("another model chosen");
                {
                    let mut st = self.st();
                    (st.state, st.said) = ("absent".into(), None);
                }
                self.rediscover();
            }
            // a page newer or older than this engine: its action, at most 64 characters, for a log
            _ => return refused(said("unknown_action", json!({"action": action.chars().take(64).collect::<String>()}), "unknown action")),
        }
        self.status()
    }

    pub fn rediscover(&self) {
        let me = self.clone();
        std::thread::Builder::new().name("local-ai-find".into()).spawn(move || me.discover()).ok();
    }

    /// One answer, streamed: on_text(piece) for each piece. The runner's "done" event, or a sentence saying what failed (its
    /// code: told()).
    pub fn generate(&self, segments: &[Seg], max_tokens: u32, cancelled: &dyn Fn() -> bool, on_text: &mut dyn FnMut(&str)) -> Result<Value, String> {
        let _one = self.0.gen_lock.lock().unwrap_or_else(|e| e.into_inner()); // the model holds one conversation
        let (rid, events, family) = {
            let mut st = self.st();
            if st.proc.is_none() || st.state != "ready" {
                return Err(tell(st.said.clone().unwrap_or_else(|| fixed("not_running"))));
            }
            st.next_id += 1;
            st.last_used = Instant::now();
            (st.next_id, st.events.take(), st.family.clone())
        };
        let Some(events) = events else { return Err(tell(fixed("busy"))) };
        while events.try_recv().is_ok() {} // anything left from an answer that was stopped
        let stops = family.as_ref().map(|f| f.stops()).unwrap_or_default();
        let req = json!({"op": "gen", "id": rid, "segments": segments.iter().map(|(t, s)| json!([t, s])).collect::<Vec<_>>(),
                         "max_tokens": max_tokens, "temp": 0.2, "top_k": 64, "top_p": 0.95, "min_p": 0.05, "seed": 1234 + rid, "stop": stops});
        let result = (|| {
            self.send(&req).map_err(tell)?;
            let mut sent_cancel = false;
            loop {
                if cancelled() && !sent_cancel {
                    sent_cancel = true;
                    let _ = self.send(&json!({"op": "cancel"}));
                }
                let ev = match events.recv_timeout(Duration::from_millis(200)) {
                    Ok(ev) => ev,
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => return Err(tell(fixed("stopped_unexpectedly"))),
                };
                match ev["ev"].as_str() {
                    Some("exit") => return Err(tell(fixed("stopped_unexpectedly"))),
                    _ if ev["id"].as_u64() != Some(rid) => continue,
                    Some("tok") => on_text(ev["t"].as_str().unwrap_or("")),
                    Some("done") => {
                        // the runner's numbers (words read and written, the time each took), for the log on this computer
                        eprintln!("local AI: done {ev}");
                        return Ok(ev);
                    }
                    Some("error") => return Err(tell(plain_error(&ev))),
                    _ => {}
                }
            }
        })();
        let mut st = self.st();
        st.last_used = Instant::now();
        if st.proc.is_some() {
            st.events = Some(events);
        }
        result
    }

    fn send(&self, v: &Value) -> Result<(), Said> {
        let mut st = self.st();
        let p = st.proc.as_mut().ok_or_else(|| fixed("not_running"))?;
        let mut line = serde_json::to_vec(v).unwrap_or_default();
        line.push(b'\n');
        p.stdin.write_all(&line).and_then(|_| p.stdin.flush()).map_err(|_| fixed("stopped_unexpectedly"))
    }
}

/// What setup needs from the local AI: a measure of a model file on this computer, the files it already checked, and
/// stopping the model before its file is removed.
impl crate::setup::api::AiHooks for Local {
    fn measure(&self, _model: &Value, path: &Path) -> Option<Value> {
        self.0.runner.as_ref()?;
        let out = self.argv(path, &["--measure"]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
        let fit = String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).find(|v| v["ev"] == "fit")?;
        Some(for_setup(&judge(&fit, gguf::read(path).ok().as_ref())))
    }

    fn mark_checked(&self, path: &Path, sha256: &str) {
        Local::mark_checked(self, path, sha256)
    }

    fn stop(&self, why: &str) {
        Local::stop(self, why)
    }

    fn rediscover(&self) {
        Local::rediscover(self)
    }

    fn use_model_file(&self, path: &Path) -> Result<Value, (String, String)> {
        Local::use_file(self, path)
    }

    fn chosen_file(&self) -> Option<Value> {
        let custom = self.saved("custom");
        (self.saved("model") == CUSTOM && custom.is_object()).then_some(custom)
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(mut p) = self.st.lock().ok().and_then(|mut s| s.proc.take()) {
            let _ = p.stdin.write_all(b"{\"op\":\"quit\"}\n");
            let _ = p.child.kill();
        }
    }
}

fn stamp(p: &Path) -> Value {
    let md = std::fs::metadata(p).ok();
    let mtime = md.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos() as u64);
    json!([p.to_string_lossy(), md.map(|m| m.len()), mtime])
}

/// A folder as the person knows it: relative to the home folder when it is inside it.
pub fn folder_name(p: &Path) -> String {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    if home.is_some_and(|h| h == folder_of(p)) {
        return "your home folder".into();
    }
    folder_param(p)
}

/// The folder a model file is in (or the folder itself).
fn folder_of(p: &Path) -> &Path {
    if p.extension().is_some_and(|e| e == "gguf") { p.parent().unwrap_or(p) } else { p }
}

/// A folder as the person knows it, in a sentence's params: as folder_name, and the home folder by its own name (no English
/// words in a param).
fn folder_param(p: &Path) -> String {
    let folder = folder_of(p);
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    if let Some(rel) = home.as_ref().and_then(|h| folder.strip_prefix(h).ok()).filter(|r| !r.as_os_str().is_empty()) {
        return rel.to_string_lossy().into_owned();
    }
    folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| folder.to_string_lossy().into_owned())
}

/// No console window for the program on Windows.
fn quiet(c: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    #[cfg(target_os = "linux")]
    unsafe {
        use std::os::unix::process::CommandExt;
        // the program goes when Lastbook goes, even if Lastbook is killed
        c.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
    let _ = &c;
}

/// Windows: the program goes when Lastbook goes, even if Lastbook is killed (a job object that kills on close). Elsewhere it
/// leaves when its standard input closes, which happens when Lastbook ends for any reason.
fn tie_to_this_process(_child: &mut Child) {
    #[cfg(windows)]
    unsafe {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::*;
        static JOB: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
        let job = *JOB.get_or_init(|| {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(job, JobObjectExtendedLimitInformation, &info as *const _ as *const _, std::mem::size_of_val(&info) as u32);
            job as usize
        });
        if AssignProcessToJobObject(job as _, _child.as_raw_handle() as _) == 0 {
            eprintln!("local AI: could not tie the program to Lastbook Europe");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lb-ai-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("models")).unwrap();
        d
    }

    fn wait_for(l: &Local, states: &[&str]) -> Value {
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(120) {
            let s = l.status();
            if states.contains(&s["state"].as_str().unwrap_or("")) {
                return s;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("never reached {states:?}: {}", l.status());
    }

    fn moe() -> gguf::Info {
        gguf::Info { arch: "qwen35moe".into(), expert_count: 256, ..Default::default() }
    }

    fn dense() -> gguf::Info {
        gguf::Info { arch: "qwen35".into(), ..Default::default() }
    }

    /// The runner's fit line for a 37.8 GB model on a computer with 17.2 GB.
    fn fit(from_disk: bool, status: &str) -> Value {
        json!({"status": status, "ram_total": 17_179_869_184u64, "file_bytes": 37_812_647_552u64, "from_disk": from_disk, "placement": "disk_cpu"})
    }

    fn sizes() -> Value {
        json!({"file_bytes": 37_812_647_552u64, "ram_bytes": 17_179_869_184u64})
    }

    /// (state or verdict, code, params) of an answer.
    fn coded<'a>(v: &'a Value, what: &str, code: &str) -> (&'a str, &'a str, &'a Value) {
        (v[what].as_str().unwrap_or(""), v[code].as_str().unwrap_or(""), &v[code.replace("code", "params")])
    }

    #[test]
    fn three_answers() {
        let j = judge(&fit(false, "fits"), Some(&dense()));
        assert_eq!((j["verdict"].as_str(), j.get("why"), j.get("code")), (Some("fits"), None, None));
        let j = judge(&fit(true, "fits"), Some(&moe()));
        assert_eq!(coded(&j, "verdict", "code"), ("from_disk", "from_disk", &sizes()));
        assert_eq!(j["why"], "The model is 37.8 GB, more than this computer’s memory (17.2 GB). It can still run from the disk, much more slowly.");
        let j = judge(&fit(true, "fits"), Some(&dense()));
        assert_eq!(coded(&j, "verdict", "code"), ("too_big", "too_big", &sizes()));
        assert_eq!(j["why"], "The model needs about 37.8 GB of this computer’s memory, plus room for everything else, and this computer has 17.2 GB.");
        let j = judge(&fit(false, "does_not_fit"), Some(&moe()));
        assert_eq!(coded(&j, "verdict", "code"), ("too_big", "too_big_graphics", &json!({})));
        assert_eq!(j["why"], "The model does not fit this computer’s graphics memory together with its own memory.");
    }

    #[test]
    fn from_the_disk_only_when_it_reads_fast() {
        let from_disk = judge(&fit(true, "fits"), Some(&moe()));
        assert_eq!(judge_disk(from_disk.clone(), Some(2400.0)), from_disk, "fast enough: unchanged");
        let j = judge_disk(from_disk.clone(), Some(87.4));
        assert_eq!(coded(&j, "verdict", "code"), ("too_big", "disk_too_slow", &json!({"disk_mb_s": 87})));
        assert_eq!(j["why"], "The model is larger than this computer’s memory, and the disk it is on reads too slowly (87 MB/s) to run it from there.");
        let j = judge_disk(from_disk, None);
        assert_eq!(coded(&j, "verdict", "code"), ("too_big", "disk_speed_unknown", &json!({})));
        assert!(j["why"].as_str().unwrap().contains("(unknown speed)"));
        let fits = judge(&fit(false, "fits"), Some(&dense()));
        assert_eq!(judge_disk(fits.clone(), None), fits, "a model that fits is not asked about the disk");
    }

    #[test]
    fn setup_reads_the_measure_by_its_code() {
        let needs = 37_812_647_552u64 + (5 << 29);
        assert_eq!(for_setup(&judge(&fit(false, "fits"), Some(&dense()))),
                   json!({"fits": true, "from_disk": false, "needs_bytes": needs, "estimate": false, "why": "Measured on this computer: it fits.", "code": "measured_fits", "params": {}}));
        let m = for_setup(&judge(&fit(true, "fits"), Some(&moe())));
        assert_eq!((m["fits"].as_bool(), m["from_disk"].as_bool()), (Some(false), Some(true)));
        assert_eq!(coded(&m, "why", "code").1, "from_disk_ssd");
        assert_eq!(m["params"], sizes());
        assert_eq!(m["why"], "The model is 37.8 GB, more than this computer’s memory (17.2 GB). It can still run from the disk, much more slowly. Only as your choice, on an SSD.");
        let m = for_setup(&judge(&fit(true, "fits"), Some(&dense())));
        assert_eq!((m["fits"].as_bool(), m["from_disk"].as_bool(), m["code"].as_str(), &m["params"]), (Some(false), Some(false), Some("too_big"), &sizes()));
        assert!(m["why"].as_str().unwrap().starts_with("The model needs about 37.8 GB"));
    }

    #[test]
    fn the_runners_errors_by_their_codes() {
        for (code, english) in [("too_long", "The question and its sources were too long for the local AI. Start a new chat or ask a shorter question."),
                                ("load", "The local AI could not load the model file."),
                                ("context", "There was not enough free memory to start the local AI. Close other programs and ask again."),
                                ("model_file", "The local AI could not read the model file.")] {
            assert_eq!(plain_error(&json!({"ev": "error", "code": code, "msg": "llama: details"})), said(code, json!({}), english));
        }
        assert_eq!(plain_error(&json!({"ev": "error", "code": "ggml", "msg": "ggml: out of something"})), said("failed", json!({}), "ggml: out of something"));
        assert_eq!(plain_error(&json!({"ev": "error"})), said("failed", json!({}), "The local AI failed."));
    }

    #[test]
    fn every_sentence_without_params_is_known_by_its_words() {
        for (code, english) in FIXED {
            assert!(CODES.contains(&code), "{code} is not in CODES");
            assert_eq!(told(english), Some(said(code, json!({}), english)));
        }
        assert_eq!(told("Something else went wrong."), None);
        // every sentence this file makes with fixed() is one of them (a misspelt code would only show when it is said)
        for part in include_str!("local.rs").split(concat!("fixed", "(\"")).skip(1) {
            let code = part.split('"').next().unwrap();
            assert!(FIXED.iter().any(|(c, _)| *c == code), "{code} is not in FIXED");
        }
        assert_eq!(kind_code(std::io::ErrorKind::PermissionDenied), "permission_denied");
        assert_eq!(kind_code(std::io::ErrorKind::NotFound), "not_found");
    }

    #[test]
    fn the_systems_reasons_are_its_kinds() {
        use std::io::{Error, ErrorKind as K};
        // each reason the interface words is the kind_code() of the system's error it stands for
        for (kind, code) in [(K::NotFound, "not_found"), (K::PermissionDenied, "permission_denied"), (K::OutOfMemory, "out_of_memory"), (K::TimedOut, "timed_out"),
                             (K::ReadOnlyFilesystem, "read_only_filesystem"), (K::StorageFull, "storage_full")] {
            assert_eq!(kind_code(Error::from(kind).kind()), code);
            assert!(KINDS.contains(&code), "{code} is not in KINDS");
        }
        // as the system reports them
        #[cfg(unix)]
        for (errno, code) in [(libc::ENOENT, "not_found"), (libc::EACCES, "permission_denied"), (libc::ENOMEM, "out_of_memory"), (libc::ETIMEDOUT, "timed_out"),
                              (libc::EROFS, "read_only_filesystem"), (libc::ENOSPC, "storage_full")] {
            assert_eq!(kind_code(Error::from_raw_os_error(errno).kind()), code);
        }
        // another kind keeps its name, and is said without a reason
        assert_eq!(kind_code(K::Interrupted), "interrupted");
        assert!(!KINDS.contains(&"interrupted"));
    }

    #[test]
    fn the_buttons_it_cannot_act_on_say_why_by_their_codes() {
        let (l, d) = local("buttons", vec![]);
        // no model found yet: nothing to run from the disk
        for action in ["from_disk_on", "from_disk_off"] {
            assert_eq!(l.action(action), json!({"error": "no model", "code": "no_model", "params": {}}), "{action}");
        }
        // a button this engine does not have (a page newer or older than it): its action, for a log
        assert_eq!(l.action("fly"), json!({"error": "unknown action", "code": "unknown_action", "params": {"action": "fly"}}));
        assert_eq!(l.action(&"x".repeat(100))["params"]["action"], "x".repeat(64));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn every_code_has_words_in_the_interface() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/locales/en.json");
        let words: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        for code in CODES {
            assert!(words[format!("chat.code.{code}")].is_string(), "web/locales/en.json has no chat.code.{code}");
        }
        for kind in KINDS {
            assert!(words[format!("chat.code.kind.{kind}")].is_string(), "web/locales/en.json has no chat.code.kind.{kind}");
        }
    }

    /// A local AI on a library of its own, offering these models. Its runner is a path where there is none: nothing is ever
    /// started.
    fn local(name: &str, catalogue: Vec<Value>) -> (Local, PathBuf) {
        let d = tmp(name);
        let l = Local::new(d.clone(), Box::new(move || catalogue.clone()), Some(d.join("no-runner")));
        (l, d)
    }

    fn entry(id: &str, file: &str, bytes: &[u8]) -> Value {
        json!({"id": id, "title": id.to_uppercase(), "file": file, "size": bytes.len(), "sha256": hex::encode(Sha256::digest(bytes))})
    }

    #[test]
    fn each_state_says_its_code() {
        let bytes = b"GGUF but not really a model".to_vec();
        // this copy has no runtime
        let l = Local::new(tmp("noruntime"), Box::new(Vec::new), None);
        l.discover();
        let s = l.status();
        assert_eq!(coded(&s, "state", "code"), ("error", "no_runtime", &json!({})));
        assert_eq!(s["message"], "This copy of Lastbook Europe has no local AI runtime.", "the English stays beside the code");
        // nothing there: no message, no code
        let (l, d) = local("states", vec![entry("tiny", "tiny.gguf", &bytes)]);
        l.discover();
        let s = l.status();
        assert_eq!((s["state"].as_str(), s["message"].as_str(), s.get("code"), s.get("params")), (Some("absent"), Some(""), None, None));
        // a file too short: the download may not have finished
        let file = d.join("models").join("tiny.gguf");
        let folder = folder_param(&file);
        std::fs::write(&file, &bytes[..10]).unwrap();
        l.discover();
        let s = l.status();
        assert_eq!(coded(&s, "state", "code"), ("wrong", "unfinished", &json!({"folder": folder, "size_bytes": 10, "expected_bytes": 27})));
        assert_eq!(s["message"], format!("The file in {} is 0.0 GB of 0.0 GB. The download may not have finished.", folder_name(&file)));
        // too long: not the file Lastbook expects
        std::fs::write(&file, [&bytes[..], b" and more"].concat()).unwrap();
        l.discover();
        let s = l.status();
        assert_eq!(coded(&s, "state", "code"), ("wrong", "wrong_size", &json!({"folder": folder, "size_bytes": 36, "expected_bytes": 27})));
        assert!(s["message"].as_str().unwrap().ends_with("It is not the file Lastbook Europe expects."));
        // the right size, other contents
        std::fs::write(&file, b"GGUF but not really a mode!").unwrap();
        l.discover();
        let s = l.status();
        assert_eq!(coded(&s, "state", "code"), ("wrong", "wrong_contents", &json!({"folder": folder})));
        assert_eq!(s["message"], format!("The file in {} has the right size but different contents. Download it again.", folder_name(&file)));
        // a file Lastbook may not read
        #[cfg(unix)]
        if unsafe { libc::geteuid() } != 0 {
            use std::os::unix::fs::PermissionsExt;
            std::fs::write(&file, &bytes).unwrap();
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
            l.discover();
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
            let s = l.status();
            assert_eq!(coded(&s, "state", "code"), ("wrong", "cannot_read", &json!({"kind": "permission_denied"})));
            assert_eq!(s["message"], "Lastbook Europe could not read the model file (permission denied).");
        }
        // the right file, but not a model Lastbook knows how to ask
        std::fs::write(&file, &bytes).unwrap();
        l.discover();
        let s = l.status();
        assert_eq!(coded(&s, "state", "code"), ("error", "unknown_model", &json!({})));
        assert_eq!(s["message"], "Lastbook Europe does not know how to ask this model questions.");
        // turned off: no message
        let s = l.action("off");
        assert_eq!((s["state"].as_str(), s["message"].as_str(), s.get("code")), (Some("off"), Some(""), None));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn the_states_the_interface_draws() {
        // the `local` of /api/assistant/status for the states the chat page and About draw: two models here, the larger first
        let (big, small) = (b"a larger model file".to_vec(), b"a small one".to_vec());
        let (l, d) = local("draws", vec![entry("big", "big.gguf", &big), entry("small", "small.gguf", &small)]);
        std::fs::write(d.join("models").join("big.gguf"), &big).unwrap();
        std::fs::write(d.join("models").join("small.gguf"), &small).unwrap();
        l.discover();
        let s = l.status();
        assert_eq!(s["models"], json!([{"id": "big", "title": "BIG", "size": 19}, {"id": "small", "title": "SMALL", "size": 11}]));
        // checking, and how far
        {
            let mut st = l.st();
            (st.state, st.pct, st.said) = ("checking".into(), Some(40), None);
        }
        let s = l.status();
        assert_eq!((s["state"].as_str(), s["pct"].as_u64(), s.get("code")), (Some("checking"), Some(40), None));
        // measured: larger than the memory, a mixture of experts, on a fast disk: waiting for the person's choice
        let model = ModelEntry::from_manifest(&entry("big", "big.gguf", &big)).unwrap();
        {
            let mut st = l.st();
            (st.model, st.path) = (Some(model.clone()), Some(d.join("models").join("big.gguf")));
        }
        l.measured(&model, judge_disk(judge(&fit(true, "fits"), Some(&moe())), Some(2400.0)), Some(2400.0));
        let s = l.status();
        assert_eq!((s["state"].as_str(), s.get("code"), s["verdict"].as_str()), (Some("from_disk"), None, Some("from_disk")));
        assert_eq!(coded(&s, "state", "why_code").1, "from_disk");
        assert_eq!((&s["why_params"], s["disk_mb_s"].as_f64(), s["from_disk_chosen"].as_bool()), (&sizes(), Some(2400.0), Some(false)));
        assert_eq!(s["why"], "The model is 37.8 GB, more than this computer’s memory (17.2 GB). It can still run from the disk, much more slowly.");
        // chosen: ready to start, from the disk; not any more: waiting again
        let s = l.action("from_disk_on");
        assert_eq!((s["state"].as_str(), s["from_disk_chosen"].as_bool(), s["why_code"].as_str()), (Some("stopped"), Some(true), Some("from_disk")));
        let s = l.action("from_disk_off");
        assert_eq!((s["state"].as_str(), s["from_disk_chosen"].as_bool()), (Some("from_disk"), Some(false)));
        // too big: the state's message is the why, with its code, in both places
        l.measured(&model, judge_disk(judge(&fit(true, "fits"), Some(&dense())), None), None);
        let s = l.status();
        assert_eq!(coded(&s, "state", "code"), ("too_big", "too_big", &sizes()));
        assert_eq!((s["why_code"].as_str(), &s["why_params"], &s["message"]), (Some("too_big"), &sizes(), &s["why"]));
        // on a disk too slow
        l.measured(&model, judge_disk(judge(&fit(true, "fits"), Some(&moe())), Some(87.0)), Some(87.0));
        let s = l.status();
        assert_eq!(coded(&s, "state", "code"), ("too_big", "disk_too_slow", &json!({"disk_mb_s": 87})));
        // the other model chosen: looked for and checked
        l.action("use:small");
        wait_for(&l, &["error", "wrong"]);
        assert_eq!(l.saved("model"), "small");
        assert!(l.saved("checked")["small"].is_array(), "{}", l.status());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn why_it_cannot_answer_is_told_by_its_code() {
        let bytes = b"GGUF but not really a model".to_vec();
        let (mut l, d) = local("told", vec![entry("tiny", "tiny.gguf", &bytes)]);
        // long beside the 150 ms after which a thread below ends a start: a busy test machine must not make it a timeout
        Arc::get_mut(&mut l.0).unwrap().start_wait = Duration::from_secs(3);
        // nothing set up
        let text = l.ensure_running(&mut |_| {}, &|| false).unwrap();
        assert_eq!(told(&text), Some(fixed("not_set_up")));
        let text = l.generate(&[], 8, &|| false, &mut |_| {}).unwrap_err();
        assert_eq!(told(&text), Some(fixed("not_running")));
        // the state's own message, with its params
        let unfinished = said("unfinished", json!({"folder": "models", "size_bytes": 10, "expected_bytes": 27}), "The file in models is 0.0 GB of 0.0 GB. The download may not have finished.");
        {
            let mut st = l.st();
            (st.state, st.said) = ("wrong".into(), Some(unfinished.clone()));
        }
        assert_eq!(told(&l.ensure_running(&mut |_| {}, &|| false).unwrap()), Some(unfinished.clone()));
        assert_eq!(told(&l.generate(&[], 8, &|| false, &mut |_| {}).unwrap_err()), Some(unfinished));
        // ready to start, but its runner cannot be started: could not start, and the error's kind
        let file = d.join("models").join("tiny.gguf");
        std::fs::write(&file, &bytes).unwrap();
        {
            let mut st = l.st();
            (st.state, st.said, st.path, st.fit) = ("stopped".into(), None, Some(file.clone()), Some(judge(&fit(false, "fits"), Some(&dense()))));
        }
        let text = l.ensure_running(&mut |_| {}, &|| false).unwrap();
        assert!(text.starts_with("The local AI could not start ("), "{text}");
        assert_eq!(told(&text), Some(said("cannot_start", json!({"kind": "not_found"}), text.clone())));
        assert_eq!(coded(&l.status(), "state", "code"), ("error", "cannot_start", &json!({"kind": "not_found"})));
        // starting, and then not: it did not start
        l.st().state = "loading".into();
        let me = l.clone();
        let flip = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            let mut st = me.st();
            (st.state, st.said) = ("stopped".into(), None);
        });
        assert_eq!(told(&l.ensure_running(&mut |_| {}, &|| false).unwrap()), Some(fixed("did_not_start")));
        flip.join().unwrap();
        // starting for too long
        l.st().state = "loading".into();
        assert_eq!(told(&l.ensure_running(&mut |_| {}, &|| false).unwrap()), Some(fixed("start_timeout")));
        assert_eq!(l.status()["state"], "stopped");
        // stopped by the person: an answer that is no sentence
        l.st().state = "loading".into();
        assert_eq!(l.ensure_running(&mut |_| {}, &|| true), Some("stopped".into()));
        let _ = std::fs::remove_dir_all(d);
    }

    /// A stand-in for the runner (a shell script, no model): what it prints is what the runner would say.
    #[cfg(unix)]
    fn script(d: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p = d.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    #[cfg(unix)]
    #[test]
    fn the_runners_errors_are_told_by_their_codes() {
        let bytes = b"GGUF but not really a model".to_vec();
        let d = tmp("stand-in");
        let file = d.join("models").join("tiny.gguf");
        std::fs::write(&file, &bytes).unwrap();
        let ready = |runner: PathBuf| {
            let l = Local::new(d.clone(), Box::new(Vec::new), Some(runner));
            {
                let mut st = l.st();
                (st.state, st.path, st.fit, st.family) = ("stopped".into(), Some(file.clone()), Some(judge(&fit(false, "fits"), Some(&dense()))), Some(Family::Qwen35));
            }
            l
        };
        // too little memory while loading: the state's message, and the answer
        let l = ready(script(&d, "no-memory", r#"echo '{"ev":"error","code":"context","msg":"failed to allocate"}'"#));
        assert_eq!(told(&l.ensure_running(&mut |_| {}, &|| false).unwrap()), Some(fixed("context")));
        assert_eq!(coded(&l.status(), "state", "code"), ("error", "context", &json!({})));
        // a program that ends at once, twice: it did not start, then could not start
        let l = ready(script(&d, "ends", "exit 0"));
        assert_eq!(told(&l.ensure_running(&mut |_| {}, &|| false).unwrap()), Some(fixed("did_not_start")));
        assert_eq!(told(&l.ensure_running(&mut |_| {}, &|| false).unwrap()), Some(fixed("start_failed")));
        assert_eq!(coded(&l.status(), "state", "code"), ("error", "start_failed", &json!({})));
        // ready, then the question is too long for it
        let id = r#"id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')"#;
        let l = ready(script(&d, "too-long", &format!("echo '{{\"ev\":\"ready\"}}'\nread line\n{id}\necho \"{{\\\"ev\\\":\\\"error\\\",\\\"id\\\":$id,\\\"code\\\":\\\"too_long\\\"}}\"\nread line")));
        assert_eq!(l.ensure_running(&mut |_| {}, &|| false), None);
        assert_eq!(told(&l.generate(&[], 8, &|| false, &mut |_| {}).unwrap_err()), Some(fixed("too_long")));
        l.stop("test over");
        // ready, then gone in the middle of an answer
        let l = ready(script(&d, "gone", "echo '{\"ev\":\"ready\"}'\nread line\nexit 0"));
        assert_eq!(l.ensure_running(&mut |_| {}, &|| false), None);
        assert_eq!(told(&l.generate(&[], 8, &|| false, &mut |_| {}).unwrap_err()), Some(fixed("stopped_unexpectedly")));
        let _ = std::fs::remove_dir_all(d);
    }

    #[cfg(unix)]
    #[test]
    fn measuring_says_its_code() {
        use crate::ai::gguf::tests::{header, Kv};
        // a model file's header only (a mixture of experts Lastbook knows how to ask), measured by stand-ins for the runner
        let model = header(&[("general.architecture", Kv::Str("qwen35moe")), ("qwen35moe.expert_count", Kv::Num(256)), ("tokenizer.chat_template", Kv::Str("<|im_start|>"))]);
        let d = tmp("measure");
        std::fs::write(d.join("models").join("m.gguf"), &model).unwrap();
        let catalogue = vec![entry("m", "m.gguf", &model)];
        let measured_by = |name: &str, says: &str| {
            let c = catalogue.clone();
            let l = Local::new(d.clone(), Box::new(move || c.clone()), Some(script(&d, name, says)));
            l.discover();
            l.status()
        };
        // the runner says nothing of how the model fits
        let s = measured_by("silent", "exit 0");
        assert_eq!(coded(&s, "state", "code"), ("error", "cannot_measure", &json!({})));
        assert_eq!(s["message"], "The local AI program could not read the model file.");
        // larger than the memory, on a disk whose speed cannot be measured (a file this small): too big
        let s = measured_by("from-disk", r#"echo '{"ev":"fit","status":"fits","ram_total":17179869184,"file_bytes":37812647552,"from_disk":true,"placement":"disk_cpu"}'"#);
        assert_eq!(coded(&s, "state", "code"), ("too_big", "disk_speed_unknown", &json!({})));
        assert_eq!((s["verdict"].as_str(), s["why_code"].as_str(), &s["message"]), (Some("too_big"), Some("disk_speed_unknown"), &s["why"]));
        // it fits: ready to start, and nothing to say
        let s = measured_by("fits", r#"echo '{"ev":"fit","status":"fits","ram_total":17179869184,"file_bytes":1000,"from_disk":false,"placement":"fit"}'"#);
        assert_eq!((s["state"].as_str(), s.get("code"), &s["why"], &s["why_code"]), (Some("stopped"), None, &Value::Null, &Value::Null));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn finds_checks_and_measures_with_the_real_runner() {
        let Some(runner) = runner_path() else {
            eprintln!("no lastbook-llm beside the tests: build the workspace first");
            return;
        };
        // a file of the right size and digest that is not a model: found, checked, then the runner refuses it
        let d = tmp("fake");
        let bytes = b"GGUF but not really a model".to_vec();
        std::fs::write(d.join("models").join("tiny.gguf"), &bytes).unwrap();
        let cat = vec![json!({"id": "tiny", "title": "Tiny", "file": "tiny.gguf", "size": bytes.len(), "sha256": hex::encode(Sha256::digest(&bytes)), "available": true})];
        let l = Local::new(d.clone(), Box::new(move || cat.clone()), Some(runner.clone()));
        l.discover();
        let s = l.status();
        assert_eq!(s["state"], "error", "{s}");
        assert!(l.saved("checked")["tiny"].is_array(), "the check is remembered");
        // the wrong digest: refused without running anything
        let cat = vec![json!({"id": "tiny", "title": "Tiny", "file": "tiny.gguf", "size": bytes.len(), "sha256": "00".repeat(32)})];
        let d2 = tmp("wrong");
        std::fs::write(d2.join("models").join("tiny.gguf"), &bytes).unwrap();
        let l = Local::new(d2.clone(), Box::new(move || cat.clone()), Some(runner.clone()));
        l.discover();
        assert_eq!(l.status()["state"], "wrong");
        // nothing there, and turned off
        let l = Local::new(tmp("none"), Box::new(Vec::new), Some(runner));
        l.discover();
        assert_eq!(l.status()["state"], "absent");
        assert_eq!(l.action("off")["state"], "off");
        assert_eq!(l.status()["enabled"], false);
        for x in [d, d2] {
            let _ = std::fs::remove_dir_all(x);
        }
    }

    #[test]
    fn answers_with_a_real_model_when_one_is_named() {
        let (Some(model), Some(runner)) = (std::env::var_os("LASTBOOK_TEST_MODEL").map(PathBuf::from), runner_path()) else {
            eprintln!("LASTBOOK_TEST_MODEL is not set: no model to run");
            return;
        };
        let d = tmp("real");
        let size = std::fs::metadata(&model).unwrap().len();
        let mut h = Sha256::new();
        std::io::copy(&mut std::fs::File::open(&model).unwrap(), &mut h).unwrap();
        let file = model.file_name().unwrap().to_string_lossy().into_owned();
        let cat = vec![json!({"id": "test", "title": "Test model", "file": file, "size": size, "sha256": hex::encode(h.finalize())})];
        std::env::set_var("LASTBOOK_MODEL_DIR", model.parent().unwrap());
        let l = Local::new(d.clone(), Box::new(move || cat.clone()), Some(runner));
        l.discover();
        let s = wait_for(&l, &["stopped", "from_disk", "too_big", "error"]);
        assert_eq!(s["state"], "stopped", "{s}");
        assert_eq!(l.family(), Some(Family::Qwen35));
        assert_eq!(l.ensure_running(&mut |_| {}, &|| false), None);
        let msgs = vec![json!({"role": "system", "content": "Be brief."}), json!({"role": "user", "content": "Ciao"})];
        let segs = Family::Qwen35.render(&msgs, &[]).unwrap();
        let mut text = String::new();
        let done = l.generate(&segs, 8, &|| false, &mut |t| text.push_str(t)).unwrap();
        assert_eq!(done["ev"], "done");
        assert!(done["n_gen"].as_u64().unwrap() >= 1);
        l.stop("test over");
        assert_eq!(l.status()["state"], "stopped");
        let _ = std::fs::remove_dir_all(d);
    }
    #[cfg(unix)]
    #[test]
    fn a_library_that_cannot_be_written_keeps_its_choices_on_this_computer() {
        use std::os::unix::fs::PermissionsExt;
        let lib = tmp("readonly-lib");
        let own = tmp("readonly-own");
        std::fs::set_permissions(&lib, std::fs::Permissions::from_mode(0o555)).unwrap(); // as a read-only share or stick
        let l = Local::new(lib.clone(), Box::new(Vec::new), None).with_own_folder(own.clone());
        assert_eq!(l.write_base(), own);
        l.set_saved("enabled", json!(false));
        assert!(own.join("derived/llm/state.json").exists());
        assert!(!lib.join("derived").exists(), "nothing written beside the library");
        // the next start reads its choices back from there
        let again = Local::new(lib.clone(), Box::new(Vec::new), None).with_own_folder(own.clone());
        assert!(!again.enabled());
        // a library that can be written keeps them beside itself
        std::fs::set_permissions(&lib, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(Local::new(lib.clone(), Box::new(Vec::new), None).with_own_folder(own).write_base(), lib);
    }
}
