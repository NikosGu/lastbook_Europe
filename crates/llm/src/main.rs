//! lastbook-llm: runs one local language model for Lastbook Europe. It talks only through its standard input and output,
//! one JSON object per line, and never opens a socket, a port or a web address. It reads one file, the model it is given.
//! Ported from the original Lastbook's packaging/llm/lastbook_llm.cpp; the protocol is the same.
//!
//!   lastbook-llm --model FILE [--ctx N] [--threads N] [--cpu-only] [--no-mmap] [--measure]
//!   lastbook-llm --devices
//!
//! --devices prints {"ev":"devices","list":[{"name","description","backend","memory_total","memory_free"},...]}: what
//! llama.cpp found to run on (the processor, a graphics card through Metal or Vulkan), and leaves.
//!
//! --no-mmap reads the whole file into memory once instead of mapping it: for a model on a network share or an external
//! disk, which fits in memory. --from-disk runs the model from the disk even if it would fit in memory (for tests).
//!
//! On start it fits the model to this computer's memory with llama.cpp's own fitting (common/fit.cpp, the same as
//! llama-server's default --fit), prints {"ev":"fit",...} with what it found, then loads the model and prints
//! {"ev":"ready",...}. --measure stops after the fit line and loads nothing.
//!
//! A mixture-of-experts model larger than the graphics memory keeps its experts in main memory, read from the model file
//! as they are needed (the file is mapped, not copied): a model larger than the memory itself still runs, from the disk,
//! more slowly. The fit line says so ("experts_on_cpu", "file_bytes" against "ram_total").
//!
//! Requests, one a line on standard input:
//!   {"op":"gen","id":1,"segments":[["<|im_start|>",true],["text",false],...],"max_tokens":1800,"temp":0.2,"top_k":64,
//!    "top_p":0.95,"min_p":0.05,"seed":1,"stop":["</tool_call>"]}
//!       A segment marked true is template markup and its special tokens are read as such. Text marked false never is: a
//!       document holding "<|im_start|>" stays plain text and cannot start a turn of its own.
//!   {"op":"cancel"}   stops the answer being written
//!   {"op":"quit"}     or the end of standard input: leave
//! Answers: {"ev":"tok","id":1,"t":"..."} for each piece, then {"ev":"done","id":1,"reason":"eog|stop|length|cancel",...}
//! or {"ev":"error","id":1,"code":"...","msg":"..."}. Everything else, llama.cpp's own log included, goes to standard error.

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::ffi::CString;
use std::io::{BufRead, Write};
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Instant;

mod experts;
mod memory;

// ------------------------------------------------------------------------------------------------------- the protocol --
static OUT: OnceLock<Mutex<std::fs::File>> = OnceLock::new();
/// The experts asked for ahead, when the model runs from the disk.
static PREFETCH: OnceLock<&'static experts::Prefetch> = OnceLock::new();

/// One line of JSON on the protocol's own channel.
fn send(v: &Value) {
    if let Some(out) = OUT.get() {
        let mut line = serde_json::to_vec(v).unwrap_or_default();
        line.push(b'\n');
        let mut f = out.lock().unwrap_or_else(|e| e.into_inner());
        let _ = f.write_all(&line);
        let _ = f.flush();
    }
}

/// The protocol keeps the real standard output to itself. File descriptor 1 then points at standard error, so a library
/// that prints can never break a line of JSON.
fn take_stdout() -> std::fs::File {
    #[cfg(unix)]
    unsafe {
        use std::os::unix::io::FromRawFd;
        let proto = libc::dup(1);
        libc::dup2(2, 1);
        std::fs::File::from_raw_fd(proto)
    }
    #[cfg(windows)]
    unsafe {
        use std::os::windows::io::FromRawHandle;
        let proto = libc::dup(1);
        libc::dup2(2, 1);
        std::fs::File::from_raw_handle(libc::get_osfhandle(proto) as _)
    }
}

// ----------------------------------------------------------------------------------------------- requests from the app --
#[derive(Default)]
struct Inbox {
    q: Mutex<VecDeque<Value>>,
    cv: Condvar,
    cancel: AtomicBool,
    quit: AtomicBool,
}

fn read_requests(inbox: Arc<Inbox>) {
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let v: Value = match serde_json::from_str::<Value>(&line) {
            Ok(v) if v.is_object() => v,
            _ => {
                send(&json!({"ev": "error", "code": "bad_request", "msg": "not a JSON object"}));
                continue;
            }
        };
        match v["op"].as_str() {
            Some("cancel") => inbox.cancel.store(true, Ordering::SeqCst),
            Some("quit") => break,
            _ => {
                inbox.q.lock().unwrap().push_back(v);
                inbox.cv.notify_one();
            }
        }
    }
    // the app closed the pipe or asked us to go: whatever is running stops too
    inbox.quit.store(true, Ordering::SeqCst);
    inbox.cancel.store(true, Ordering::SeqCst);
    inbox.cv.notify_one();
}

// ---------------------------------------------------------------------------------------------------------- generation --
struct Engine<'m> {
    model: &'m LlamaModel,
    ctx: LlamaContext<'m>,
    cached: Vec<LlamaToken>, // the tokens the context holds, in order
    inbox: Arc<Inbox>,
}

/// How much of s ends on a whole character; the rest waits for the next piece.
fn complete_utf8(s: &[u8]) -> usize {
    let n = s.len();
    for back in 1..=4.min(n) {
        let c = s[n - back];
        if c & 0xC0 == 0x80 {
            continue; // a continuation byte: look further back for the lead
        }
        let need = if c < 0x80 { 1 } else if c >> 5 == 6 { 2 } else if c >> 4 == 14 { 3 } else if c >> 3 == 30 { 4 } else { 1 };
        return if back >= need { n } else { n - back };
    }
    n
}

enum Stop {
    Cancel,
    Failed,
}

/// Reads toks[from..] into the context, a batch at a time, checking for a cancel between batches.
fn decode_all(e: &mut Engine, toks: &[LlamaToken], from: usize) -> Result<(), Stop> {
    let n_batch = e.ctx.n_batch().max(1) as usize;
    let mut batch = LlamaBatch::new(n_batch, 1);
    let mut i = from;
    while i < toks.len() {
        if e.inbox.cancel.load(Ordering::SeqCst) {
            return Err(Stop::Cancel);
        }
        let n = n_batch.min(toks.len() - i);
        batch.clear();
        for (k, t) in toks[i..i + n].iter().enumerate() {
            let pos = (i + k) as i32;
            let last = i + k == toks.len() - 1; // logits only for the last prompt token
            batch.add(*t, pos, &[0], last).map_err(|_| Stop::Failed)?;
        }
        e.ctx.decode(&mut batch).map_err(|_| Stop::Failed)?;
        e.cached.extend_from_slice(&toks[i..i + n]);
        i += n;
    }
    Ok(())
}

fn generate(e: &mut Engine, req: &Value) {
    let t0 = Instant::now();
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let fail = |code: &str, msg: String| send(&json!({"ev": "error", "id": id, "code": code, "msg": msg}));
    let vocab = e.model.vocab();
    let mut toks: Vec<LlamaToken> = Vec::new();
    if vocab.should_add_bos() {
        toks.push(vocab.bos());
    }
    for seg in req["segments"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
        match (seg.get(0).and_then(Value::as_str), seg.get(1).and_then(Value::as_bool), seg.as_array().map(Vec::len)) {
            (Some(text), Some(special), Some(2)) => toks.extend(vocab.tokenize(text.as_bytes(), false, special)),
            _ => return fail("bad_request", "segments are [text, special] pairs".into()),
        }
    }
    let n_ctx = e.ctx.n_ctx() as usize;
    if toks.len() + 16 >= n_ctx {
        return fail("too_long", format!("the question and its sources are {} tokens; the model holds {n_ctx}", toks.len()));
    }
    let max_tokens = (req["max_tokens"].as_i64().unwrap_or(1024).max(1) as usize).min(n_ctx - toks.len());

    // Reuse what the context already holds: a follow-up round repeats the conversation so far.
    let mut keep = e.cached.iter().zip(toks.iter()).take_while(|(a, b)| a == b).count();
    if keep == toks.len() {
        keep -= 1; // the last prompt token is decoded again, for its logits
    }
    if keep < e.cached.len() {
        // models with a recurrent part (Qwen3.5/3.6's linear attention) cannot always go back: start over
        if !matches!(e.ctx.clear_kv_cache_seq(Some(0), Some(keep as u32), None), Ok(true)) {
            e.ctx.clear_kv_cache();
            keep = 0;
        }
        e.cached.truncate(keep);
    }
    let reused = keep;

    e.inbox.cancel.store(false, Ordering::SeqCst);
    if let Err(stop) = decode_all(e, &toks, keep) {
        e.ctx.clear_kv_cache();
        e.cached.clear();
        return match stop {
            Stop::Cancel => send(&json!({"ev": "done", "id": id, "reason": "cancel", "n_prompt": toks.len(), "n_reused": reused, "n_gen": 0})),
            Stop::Failed => fail("decode", "the model could not read the question (llama_decode failed)".into()),
        };
    }
    let t1 = Instant::now();

    let num = |k: &str, d: f64| req[k].as_f64().unwrap_or(d) as f32;
    let mut sampler = LlamaSampler::chain_simple([
        LlamaSampler::top_k(req["top_k"].as_i64().unwrap_or(64) as i32),
        LlamaSampler::top_p(num("top_p", 0.95), 1),
        LlamaSampler::min_p(num("min_p", 0.05), 1),
        LlamaSampler::temp(num("temp", 0.2)),
        LlamaSampler::dist(req["seed"].as_u64().map(|s| s as u32).unwrap_or(0xFFFF_FFFF)),
    ]);
    let stops: Vec<String> = req["stop"].as_array().into_iter().flatten().filter_map(|s| s.as_str().map(str::to_string)).collect();
    let (mut pending, mut why, mut stopped_by) = (Vec::<u8>::new(), "length", String::new());
    let mut n_gen = 0;
    let mut batch = LlamaBatch::new(1, 1);
    while n_gen < max_tokens {
        if e.inbox.cancel.load(Ordering::SeqCst) {
            why = "cancel";
            break;
        }
        let t = sampler.sample(&e.ctx, -1);
        if vocab.is_eog(t) {
            why = "eog";
            break;
        }
        let p = vocab.token_to_piece(t, true, None);
        if let Some(s) = stops.iter().find(|s| s.as_bytes() == p.as_slice()) {
            stopped_by = s.clone();
            why = "stop";
            break;
        }
        pending.extend_from_slice(&p);
        let ok = complete_utf8(&pending);
        if ok > 0 {
            send(&json!({"ev": "tok", "id": id, "t": String::from_utf8_lossy(&pending[..ok])}));
            pending.drain(..ok);
        }
        batch.clear();
        let pos = e.cached.len() as i32;
        if batch.add(t, pos, &[0], true).is_err() || e.ctx.decode(&mut batch).is_err() {
            why = if e.inbox.cancel.load(Ordering::SeqCst) { "cancel" } else { "decode" };
            break;
        }
        e.cached.push(t);
        n_gen += 1;
    }
    if !pending.is_empty() {
        send(&json!({"ev": "tok", "id": id, "t": String::from_utf8_lossy(&pending)}));
    }
    if why == "decode" {
        e.ctx.clear_kv_cache();
        e.cached.clear();
        return fail("decode", "the model stopped while writing (llama_decode failed)".into());
    }
    let t2 = Instant::now();
    let mut done = json!({"ev": "done", "id": id, "reason": why, "n_prompt": toks.len(), "n_reused": reused, "n_gen": n_gen,
                          "prompt_ms": (t1 - t0).as_millis() as u64, "gen_ms": (t2 - t1).as_millis() as u64});
    if let Some(p) = PREFETCH.get() {
        done["experts_asked"] = json!(p.asked.load(std::sync::atomic::Ordering::Relaxed));
    }
    if !stopped_by.is_empty() {
        done["stop"] = json!(stopped_by);
    }
    send(&done);
}

// --------------------------------------------------------------------------------------------------------------- start --
struct Args {
    model: String,
    n_ctx: u32,
    threads: i32,
    cpu_only: bool,
    measure: bool,
    no_mmap: bool,
    from_disk: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args { model: String::new(), n_ctx: 16384, threads: 0, cpu_only: false, measure: false, no_mmap: false, from_disk: false };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--model" => a.model = it.next().ok_or("--model needs a file")?,
            "--ctx" => a.n_ctx = it.next().and_then(|v| v.parse().ok()).ok_or("--ctx needs a number")?,
            "--threads" => a.threads = it.next().and_then(|v| v.parse().ok()).ok_or("--threads needs a number")?,
            "--cpu-only" => a.cpu_only = true,
            "--measure" => a.measure = true,
            "--no-mmap" => a.no_mmap = true,
            "--from-disk" => a.from_disk = true,
            other => return Err(format!("unknown option {other}")),
        }
    }
    if a.model.is_empty() {
        return Err("--model FILE is required".into());
    }
    Ok(a)
}

/// Windows and Linux: llama.cpp's backends (Vulkan, and the CPU variants) are libraries of their own, loaded from beside
/// this program in a package, else from the build's own folder; each loads only where it can run, so a computer without a
/// graphics driver simply uses its processor. On a Mac, Metal is built in.
fn load_backends() {
    #[cfg(all(feature = "vulkan", any(windows, target_os = "linux")))]
    {
        let here = std::env::current_exe().ok().and_then(|e| e.parent().map(std::path::Path::to_path_buf));
        let beside = here.filter(|d| std::fs::read_dir(d).into_iter().flatten().flatten().any(|e| e.file_name().to_string_lossy().contains("ggml-cpu")));
        match beside {
            Some(d) => llama_cpp_2::llama_backend::load_backends_from_path(&d),
            None => llama_cpp_2::llama_backend::load_backends(),
        }
    }
}

/// Memory left for the system and the rest of Lastbook when deciding whether a model file fits in memory.
const DISK_RESERVE: u64 = 5 << 29; // 2.5 GiB

/// The routed experts' tensors in a mixture-of-experts model (not the shared expert, which every token uses).
const EXPERTS: &std::ffi::CStr = c"\\.ffn_(up|down|gate|gate_up)_(ch|)exps";

/// Model parameters as llama.cpp gives them, with the load reported in steps of 5% and stopped if the app goes.
fn model_params(inbox: &Arc<Inbox>) -> LlamaModelParams {
    let inbox = inbox.clone();
    let mut last = -1i32;
    LlamaModelParams::default().with_progress_callback(move |p| {
        let pct = (p * 100.0) as i32;
        if pct / 5 != last / 5 {
            last = pct;
            send(&json!({"ev": "progress", "p": pct}));
        }
        !inbox.quit.load(Ordering::SeqCst)
    })
}

fn main() {
    let _ = OUT.set(Mutex::new(take_stdout()));
    if std::env::args().skip(1).eq(["--devices"]) {
        load_backends();
        let _backend = LlamaBackend::init();
        let list: Vec<Value> = llama_cpp_2::list_llama_ggml_backend_devices().into_iter().map(|d| {
            json!({"name": d.name, "description": d.description, "backend": d.backend, "memory_total": d.memory_total, "memory_free": d.memory_free})
        }).collect();
        send(&json!({"ev": "devices", "list": list}));
        std::process::exit(0);
    }
    let args = match parse_args() {
        Ok(a) => a,
        Err(msg) => {
            send(&json!({"ev": "error", "code": "usage", "msg": msg}));
            std::process::exit(2);
        }
    };
    load_backends();
    let backend = match LlamaBackend::init() {
        Ok(b) => b,
        Err(e) => {
            send(&json!({"ev": "error", "code": "backend", "msg": e.to_string()}));
            std::process::exit(3);
        }
    };
    let inbox = Arc::new(Inbox::default());
    let file_bytes = std::fs::metadata(&args.model).map(|m| m.len()).unwrap_or(0);
    let Ok(cpath) = CString::new(args.model.clone()) else {
        send(&json!({"ev": "error", "code": "usage", "msg": "the model path holds a NUL"}));
        std::process::exit(2);
    };

    let mut mp = Box::pin(model_params(&inbox));
    let mut cp = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(args.n_ctx))
        .with_n_batch(2048)
        .with_n_ubatch(512)
        .with_n_seq_max(1)
        .with_no_perf(true)
        .with_swa_full(false); // as llama-server runs it: the sliding-window layers keep only their window
    if args.threads > 0 {
        cp = cp.with_n_threads(args.threads).with_n_threads_batch(args.threads);
    }

    // Fit: only settings left at llama.cpp's defaults are changed, and the context stays the size asked for.
    let t_fit = Instant::now();
    let status = if args.cpu_only {
        mp = Box::pin(std::pin::Pin::into_inner(mp).with_n_gpu_layers(0).with_devices(&[]).expect("no devices"));
        "fits"
    } else {
        let mut margins = vec![1usize << 30; unsafe { llama_cpp_sys_2::llama_max_devices() }];
        match mp.as_mut().fit_params(&cpath, &mut cp, &mut margins, args.n_ctx, llama_cpp_sys_2::GGML_LOG_LEVEL_ERROR) {
            Ok(_) => "fits",
            Err(llama_cpp_2::model::params::FitError::Failure) => "does_not_fit",
            Err(_) => {
                send(&json!({"ev": "error", "code": "model_file", "msg": "llama.cpp could not read the model file"}));
                std::process::exit(3);
            }
        }
    };
    let overrides = mp.tensor_buft_override_patterns();
    let experts_on_cpu = overrides.iter().any(|p| p.contains("exps"));
    let (ram_total, ram_free) = (memory::ram_total(), memory::ram_free());
    // From the disk: the file does not fit in this computer's memory beside everything else. Its weights then stay in the
    // mapped file (plain CPU buffers: llama.cpp's repacking would copy them into memory) and are read as they are needed.
    let from_disk = args.from_disk || file_bytes.saturating_add(DISK_RESERVE) > ram_total;
    let placement = if !from_disk {
        if args.cpu_only { "cpu" } else { "fit" }
    } else if args.cpu_only || (cfg!(target_os = "macos") && std::env::var_os("LASTBOOK_FROM_DISK_GPU").is_none()) {
        // everything on the CPU, the weights in the mapped file. On a Mac until the graphics processor's share below is
        // measured there (LASTBOOK_FROM_DISK_GPU=1 tries it).
        mp = Box::pin(model_params(&inbox).with_use_extra_bufts(false).with_n_gpu_layers(0));
        mp = Box::pin(std::pin::Pin::into_inner(mp).with_devices(&[]).expect("no devices"));
        "disk_cpu"
    } else {
        // The graphics processor gets a copy of the shared part (attention, the shared expert, the output), in memory of
        // its own; the experts stay in the mapped file, on the CPU. Copied, not mapped: llama.cpp as built here
        // (crates/llm/llama/lastbook.cmake) does so when asked, where on a Mac Metal would otherwise be handed the mapped
        // file from the first to the last of its tensors, nearly the whole file, experts and all.
        std::env::set_var("LASTBOOK_COPY_GPU_WEIGHTS", "1");
        mp = Box::pin(model_params(&inbox).with_use_extra_bufts(false));
        mp.as_mut().add_cpu_buft_override(EXPERTS);
        "disk_experts"
    };
    if from_disk {
        // a prompt read in pieces of 2048 tokens reads each layer's experts once per piece, not once per 512 tokens; and
        // while an answer is written, each layer's chosen experts are asked for together (experts.rs)
        let prefetch: &'static experts::Prefetch = Box::leak(Box::new(experts::Prefetch::new()));
        cp = cp.with_n_batch(2048).with_n_ubatch(2048)
            .with_cb_eval(Some(experts::eval_callback), prefetch as *const experts::Prefetch as *mut std::ffi::c_void);
        let _ = PREFETCH.set(prefetch);
    }
    send(&json!({"ev": "fit", "status": status, "n_ctx": args.n_ctx, "n_gpu_layers": mp.n_gpu_layers(), "cpu_only": args.cpu_only,
                 "ram_total": ram_total, "ram_free": ram_free, "host_overrides": overrides.len(), "experts_on_cpu": experts_on_cpu,
                 "file_bytes": file_bytes, "from_disk": from_disk, "placement": placement,
                 "fit_ms": t_fit.elapsed().as_millis() as u64}));
    if args.measure {
        return;
    }

    if args.no_mmap && !from_disk {
        mp = Box::pin(std::pin::Pin::into_inner(mp).with_use_mmap(false));
    }
    let reader_inbox = inbox.clone();
    std::thread::spawn(move || read_requests(reader_inbox));
    let t_load = Instant::now();
    let model = match LlamaModel::load_from_file(&backend, &args.model, &mp) {
        Ok(m) => m,
        Err(_) => {
            let code = if inbox.quit.load(Ordering::SeqCst) { "quit" } else { "load" };
            send(&json!({"ev": "error", "code": code, "msg": "the model could not be loaded"}));
            std::process::exit(4);
        }
    };
    let ctx = match model.new_context(&backend, cp) {
        Ok(c) => c,
        Err(_) => {
            send(&json!({"ev": "error", "code": "context", "msg": "there was not enough memory for the model's working space"}));
            std::process::exit(5);
        }
    };
    let desc = model.meta_val_str("general.name").unwrap_or_default();
    send(&json!({"ev": "ready", "n_ctx": ctx.n_ctx(), "desc": desc, "arch": model.meta_val_str("general.architecture").unwrap_or_default(),
                 "load_ms": t_load.elapsed().as_millis() as u64, "ram_free": memory::ram_free()}));
    let mut e = Engine { model: &model, ctx, cached: Vec::new(), inbox: inbox.clone() };
    loop {
        let req = {
            let mut q = inbox.q.lock().unwrap();
            loop {
                if inbox.quit.load(Ordering::SeqCst) {
                    return;
                }
                if let Some(r) = q.pop_front() {
                    break r;
                }
                q = inbox.cv.wait(q).unwrap();
            }
        };
        if req["op"] == "gen" {
            generate(&mut e, &req);
        } else {
            send(&json!({"ev": "error", "code": "bad_request", "msg": "unknown op"}));
        }
    }
}
