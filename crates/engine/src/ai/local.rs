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

use super::format::{Family, Seg};
use super::gguf;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
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

/// Does the model fit this computer? From the runner's fit line and the model file's header.
/// fits: in memory. from_disk: only from the disk (a mixture of experts larger than the memory). too_big: neither.
pub fn judge(fit: &Value, info: Option<&gguf::Info>) -> Value {
    let total = fit["ram_total"].as_u64().unwrap_or(0);
    let file = fit["file_bytes"].as_u64().unwrap_or(0);
    let moe = info.is_some_and(|i| i.is_moe()) || fit["experts_on_cpu"].as_bool().unwrap_or(false);
    let mut out = json!({"status": fit["status"], "ram_total": total, "file_bytes": file, "placement": fit["placement"],
                         "n_gpu_layers": fit["n_gpu_layers"], "needs": file + (5 << 29)});
    let (verdict, why) = if fit["from_disk"].as_bool().unwrap_or(false) {
        if moe {
            ("from_disk", format!("The model is {}, more than this computer’s memory ({}). It can still run from the disk, much more slowly.", gb(file), gb(total)))
        } else {
            ("too_big", format!("The model needs about {} of this computer’s memory, plus room for everything else, and this computer has {}.", gb(file), gb(total)))
        }
    } else if fit["status"] != "fits" {
        ("too_big", "The model does not fit this computer’s graphics memory together with its own memory.".to_string())
    } else {
        ("fits", String::new())
    };
    out["verdict"] = json!(verdict);
    out["fits"] = json!(verdict == "fits");
    if !why.is_empty() {
        out["why"] = json!(why);
    }
    out
}

pub fn gb(n: u64) -> String {
    format!("{:.1} GB", n as f64 / 1e9)
}

/// What the person reads when the runner reports an error.
fn plain_error(ev: &Value) -> String {
    match ev["code"].as_str() {
        Some("too_long") => "The question and its sources were too long for the local AI. Start a new chat or ask a shorter question.".into(),
        Some("load") => "The local AI could not load the model file.".into(),
        Some("context") => "There was not enough free memory to start the local AI. Close other programs and ask again.".into(),
        Some("model_file") => "The local AI could not read the model file.".into(),
        _ => ev["msg"].as_str().map(str::to_string).unwrap_or_else(|| "The local AI failed.".into()),
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
    message: String,
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
            st: Mutex::new(St { state: "absent".into(), pct: None, message: String::new(), model: None, path: None, family: None, fit: None,
                                ready: None, disk_mb_s: None, proc: None, events: None, spawned: Instant::now(), last_used: Instant::now(),
                                next_id: 0, failed_loads: 0, found: Vec::new() }),
            saved: Mutex::new(saved),
            gen_lock: Mutex::new(()),
            started: AtomicBool::new(false),
            serial: AtomicU64::new(0),
            idle_stop: Duration::from_secs(idle),
        }))
    }

    fn st(&self) -> std::sync::MutexGuard<'_, St> {
        self.0.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn save(&self) {
        let p = self.0.data.join("derived").join("llm").join("state.json");
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

    /// The folders searched for model files: LASTBOOK_MODEL_DIR, the library's models/, the library itself.
    pub fn places(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = std::env::var_os("LASTBOOK_MODEL_DIR").map(|v| std::env::split_paths(&v).collect()).unwrap_or_default();
        out.push(self.0.data.join("models"));
        out.push(self.0.data.clone());
        out
    }

    /// Answering, or able to start when asked.
    pub fn available(&self) -> bool {
        matches!(self.st().state.as_str(), "ready" | "stopped" | "loading")
    }

    pub fn family(&self) -> Option<Family> {
        self.st().family
    }

    pub fn status(&self) -> Value {
        let st = self.st();
        let fit = st.fit.clone().unwrap_or(Value::Null);
        json!({"name": st.model.as_ref().map(|m| m.title.clone()), "model": st.model.as_ref().map(|m| m.id.clone()),
               "file": st.model.as_ref().map(|m| m.file.clone()), "size": st.model.as_ref().map(|m| m.size),
               "state": st.state, "pct": st.pct, "message": st.message, "found_at": st.path.as_deref().map(folder_name),
               "enabled": self.enabled(), "runtime": self.0.runner.is_some(), "needs_bytes": fit["needs"], "ram_total": fit["ram_total"],
               "placement": fit["placement"], "verdict": fit["verdict"], "why": fit["why"], "disk_mb_s": st.disk_mb_s.map(|v| v.round()),
               "from_disk_chosen": st.model.as_ref().is_some_and(|m| self.from_disk_chosen(&m.id)),
               "load_s": st.ready.as_ref().and_then(|r| r["load_s"].as_f64()),
               "models": st.found.iter().map(|(m, _)| json!({"id": m.id, "title": m.title, "size": m.size})).collect::<Vec<_>>(),
               "places": self.places().iter().take(6).map(|p| folder_name(p)).collect::<Vec<_>>()})
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
                st.message.clear();
                return;
            }
            if self.0.runner.is_none() {
                st.state = "error".into();
                st.message = "This copy of Lastbook has no local AI runtime.".into();
                return;
            }
            (st.state, st.pct, st.message) = ("absent".into(), None, String::new());
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
        // the one chosen before, else the largest here: the person can choose another
        found.sort_by_key(|(m, _)| std::cmp::Reverse(m.size));
        let chosen = self.saved("model").as_str().and_then(|id| found.iter().position(|(m, _)| m.id == id)).unwrap_or(0);
        self.st().found = found.clone();
        let Some((model, path)) = found.get(chosen).cloned() else {
            let mut st = self.st();
            if let Some((p, size, want)) = wrong {
                st.state = "wrong".into();
                st.message = format!("The file in {} is {} of {}. {}", folder_name(&p), gb(size), gb(want),
                                     if size < want { "The download may not have finished." } else { "It is not the file Lastbook expects." });
            } else {
                st.state = "absent".into();
            }
            return;
        };
        let stamp = stamp(&path);
        if self.saved("checked")[&model.id] != stamp {
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
            if st.family.is_none() {
                st.state = "error".into();
                st.message = "Lastbook does not know how to ask this model questions.".into();
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
            (st.state, st.pct, st.message) = ("checking".into(), Some(0), String::new());
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
                st.message = format!("The file in {} has the right size but different contents. Download it again.", folder_name(path));
                false
            }
            Err(e) => {
                (st.state, st.pct) = ("wrong".into(), None);
                st.message = format!("Lastbook could not read the model file ({}).", e.kind());
                false
            }
        }
    }

    fn argv(&self, path: &Path, extra: &[&str]) -> Command {
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(8);
        let threads = (threads / 2).clamp(4, 16); // about the number of performance cores; more only contend
        let mut c = Command::new(self.0.runner.as_ref().expect("runner"));
        c.arg("--model").arg(path).args(["--ctx", &CTX.to_string(), "--threads", &threads.to_string()]).args(extra);
        quiet(&mut c);
        c
    }

    /// llama.cpp's fit decides where each part goes; then: in memory, from the disk, or too big.
    fn measure(&self, model: &ModelEntry, path: &Path, info: Option<&gguf::Info>) {
        {
            let mut st = self.st();
            (st.state, st.pct, st.message) = ("measuring".into(), None, String::new());
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
            st.message = "The local AI program could not read the model file.".into();
            return;
        };
        let mut judged = judge(&fit, info);
        let mut speed = None;
        if judged["verdict"] == "from_disk" {
            speed = disk_speed(path);
            if speed.is_none_or(|s| s < DISK_MB_S_MIN) {
                judged["verdict"] = json!("too_big");
                judged["why"] = json!(format!("The model is larger than this computer’s memory, and the disk it is on reads too slowly ({}) to run it from there.",
                                              speed.map(|s| format!("{s:.0} MB/s")).unwrap_or_else(|| "unknown speed".into())));
            }
        }
        eprintln!("local AI: fit {judged}");
        let mut st = self.st();
        st.disk_mb_s = speed;
        st.state = match judged["verdict"].as_str() {
            Some("fits") => "stopped".into(),
            Some("from_disk") if self.from_disk_chosen(&model.id) => "stopped".into(),
            Some("from_disk") => "from_disk".into(),
            _ => "too_big".into(),
        };
        st.message = if st.state == "too_big" { judged["why"].as_str().unwrap_or("").to_string() } else { String::new() };
        st.fit = Some(judged);
    }

    /// Starts the model if it is stopped and waits until it is ready: None, or a sentence saying why not.
    pub fn ensure_running(&self, on_wait: &mut dyn FnMut(Option<u64>), cancelled: &dyn Fn() -> bool) -> Option<String> {
        {
            let mut st = self.st();
            if st.state == "ready" && st.proc.is_some() {
                st.last_used = Instant::now();
                return None;
            }
            if !matches!(st.state.as_str(), "stopped" | "loading" | "error") || st.path.is_none() || st.fit.is_none() {
                return Some(if st.message.is_empty() { "The local AI is not set up on this computer.".into() } else { st.message.clone() });
            }
            if st.state == "error" && !st.fit.as_ref().is_some_and(|f| f["verdict"] == "fits" || f["verdict"] == "from_disk") {
                return Some(st.message.clone());
            }
            if st.state != "loading" {
                if let Err(e) = self.spawn(&mut st) {
                    st.state = "error".into();
                    st.message = format!("The local AI could not start ({e}).");
                    return Some(st.message.clone());
                }
            }
        }
        let t0 = Instant::now();
        while t0.elapsed() < START_WAIT {
            {
                let mut st = self.st();
                if st.state == "ready" {
                    st.last_used = Instant::now();
                    return None;
                }
                if st.state != "loading" {
                    return Some(if st.message.is_empty() { "The local AI did not start.".into() } else { st.message.clone() });
                }
            }
            if cancelled() {
                return Some("stopped".into());
            }
            on_wait(self.st().pct);
            std::thread::sleep(Duration::from_millis(250));
        }
        self.stop("took too long to start");
        Some("The local AI took too long to start.".into())
    }

    fn spawn(&self, st: &mut St) -> std::io::Result<()> {
        let path = st.path.clone().expect("path");
        let logs = self.0.data.join("derived").join("logs");
        std::fs::create_dir_all(&logs)?;
        let mut log = std::fs::OpenOptions::new().create(true).append(true).open(logs.join("local-ai.log"))?;
        let _ = writeln!(log, "\n--- {} start", crate::setup::api::now());
        let mut child = self.argv(&path, &[]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::from(log)).spawn()?;
        tie_to_this_process(&mut child);
        let stdout = child.stdout.take().expect("stdout");
        let stdin = child.stdin.take().expect("stdin");
        let serial = self.0.serial.fetch_add(1, Ordering::SeqCst) + 1;
        let (tx, rx) = channel();
        (st.state, st.pct, st.message) = ("loading".into(), Some(0), String::new());
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
                        st.message = plain_error(&ev);
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
                (st.state, st.message) = ("stopped".into(), String::new()); // the next question starts it again
            } else if st.state == "loading" || st.state == "error" {
                st.failed_loads += 1;
                if st.failed_loads < 2 && st.state == "loading" {
                    (st.state, st.message) = ("stopped".into(), String::new()); // once may be chance
                } else {
                    st.state = "error".into();
                    if st.message.is_empty() {
                        st.message = "The local AI could not start. Look again after closing other programs.".into();
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
                (st.state, st.pct, st.message) = ("stopped".into(), None, String::new());
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

    /// The local AI page's buttons: on, off, rescan (Look again), from_disk_on / from_disk_off (the person's choice to run
    /// a model too large for the memory from the disk), and use:<id> (another model found here).
    pub fn action(&self, action: &str) -> Value {
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
                (st.state, st.message) = ("off".into(), String::new());
            }
            "rescan" => self.rediscover(),
            "from_disk_on" | "from_disk_off" => {
                let Some(id) = self.st().model.as_ref().map(|m| m.id.clone()) else { return json!({"error": "no model"}) };
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
                self.st().state = "absent".into();
                self.rediscover();
            }
            _ => return json!({"error": "unknown action"}),
        }
        self.status()
    }

    pub fn rediscover(&self) {
        let me = self.clone();
        std::thread::Builder::new().name("local-ai-find".into()).spawn(move || me.discover()).ok();
    }

    /// One answer, streamed: on_text(piece) for each piece. The runner's "done" event, or a sentence saying what failed.
    pub fn generate(&self, segments: &[Seg], max_tokens: u32, cancelled: &dyn Fn() -> bool, on_text: &mut dyn FnMut(&str)) -> Result<Value, String> {
        let _one = self.0.gen_lock.lock().unwrap_or_else(|e| e.into_inner()); // the model holds one conversation
        let (rid, events, family) = {
            let mut st = self.st();
            if st.proc.is_none() || st.state != "ready" {
                return Err(if st.message.is_empty() { "The local AI is not running.".into() } else { st.message.clone() });
            }
            st.next_id += 1;
            st.last_used = Instant::now();
            (st.next_id, st.events.take(), st.family)
        };
        let Some(events) = events else { return Err("The local AI is busy.".into()) };
        while events.try_recv().is_ok() {} // anything left from an answer that was stopped
        let stops = family.map(|f| f.stops()).unwrap_or_default();
        let req = json!({"op": "gen", "id": rid, "segments": segments.iter().map(|(t, s)| json!([t, s])).collect::<Vec<_>>(),
                         "max_tokens": max_tokens, "temp": 0.2, "top_k": 64, "top_p": 0.95, "min_p": 0.05, "seed": 1234 + rid, "stop": stops});
        let result = (|| {
            self.send(&req)?;
            let mut sent_cancel = false;
            loop {
                if cancelled() && !sent_cancel {
                    sent_cancel = true;
                    let _ = self.send(&json!({"op": "cancel"}));
                }
                let ev = match events.recv_timeout(Duration::from_millis(200)) {
                    Ok(ev) => ev,
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => return Err("The local AI stopped unexpectedly. Ask again to restart it.".to_string()),
                };
                match ev["ev"].as_str() {
                    Some("exit") => return Err("The local AI stopped unexpectedly. Ask again to restart it.".into()),
                    _ if ev["id"].as_u64() != Some(rid) => continue,
                    Some("tok") => on_text(ev["t"].as_str().unwrap_or("")),
                    Some("done") => return Ok(ev),
                    Some("error") => return Err(plain_error(&ev)),
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

    fn send(&self, v: &Value) -> Result<(), String> {
        let mut st = self.st();
        let p = st.proc.as_mut().ok_or("The local AI is not running.")?;
        let mut line = serde_json::to_vec(v).unwrap_or_default();
        line.push(b'\n');
        p.stdin.write_all(&line).and_then(|_| p.stdin.flush()).map_err(|_| "The local AI stopped unexpectedly. Ask again to restart it.".to_string())
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
    let folder = if p.extension().is_some_and(|e| e == "gguf") { p.parent().unwrap_or(p) } else { p };
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    if let Some(rel) = home.as_ref().and_then(|h| folder.strip_prefix(h).ok()) {
        let r = rel.to_string_lossy();
        return if r.is_empty() { "your home folder".into() } else { r.into_owned() };
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
            eprintln!("local AI: could not tie the program to Lastbook");
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

    #[test]
    fn three_answers() {
        let moe = gguf::Info { arch: "qwen35moe".into(), expert_count: 256, ..Default::default() };
        let dense = gguf::Info { arch: "qwen35".into(), ..Default::default() };
        let fit = |from_disk: bool, status: &str| json!({"status": status, "ram_total": 17_179_869_184u64, "file_bytes": 37_812_647_552u64, "from_disk": from_disk, "placement": "disk_cpu"});
        assert_eq!(judge(&fit(false, "fits"), Some(&dense))["verdict"], "fits");
        assert_eq!(judge(&fit(true, "fits"), Some(&moe))["verdict"], "from_disk");
        let j = judge(&fit(true, "fits"), Some(&dense));
        assert_eq!(j["verdict"], "too_big");
        assert!(j["why"].as_str().unwrap().contains("37.8 GB"));
        assert_eq!(judge(&fit(false, "does_not_fit"), Some(&moe))["verdict"], "too_big");
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
        let segs = Family::Qwen35.render(&msgs, &[]);
        let mut text = String::new();
        let done = l.generate(&segs, 8, &|| false, &mut |t| text.push_str(t)).unwrap();
        assert_eq!(done["ev"], "done");
        assert!(done["n_gen"].as_u64().unwrap() >= 1);
        l.stop("test over");
        assert_eq!(l.status()["state"], "stopped");
        let _ = std::fs::remove_dir_all(d);
    }
}
