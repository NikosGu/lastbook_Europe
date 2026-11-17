//! The chat's answers while they are written. The window's address scheme answers each request whole, so an answer cannot
//! stream to the page: the page starts an answer, then asks for what is new every moment, and the app's page script turns
//! that back into the stream `web/chat.js` reads (`data: {event}` lines). Ported from the original Lastbook's bridge.
//!
//!   POST /api/chat/start   {messages, about, lang}  → {id}
//!   GET  /api/chat/poll/ID?from=N                   → {events: [...], next, done}
//!   POST /api/chat/cancel/ID                        → {ok}
//!   POST /api/chat                                  → the whole answer as `data:` lines, for a page without the script
//!
//! An error event says what went wrong as a code with params, which the interface words in the person's language
//! (web/locales: chat.code.<code>), beside the English (`text`), for logs and for an interface that does not know the code.
//! Its `error` is its kind: model_unavailable (the local AI cannot answer), empty (no answer was written), busy.

use super::assistant::{self, Knowledge, LocalModel};
use super::local::{self, Local};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The chat's own codes. The local AI's (ai/local.rs CODES) come too: a model_unavailable event that repeats one of its
/// sentences takes its code and params; one that does not is said by its kind.
pub const CODES: [&str; 4] = ["no_local_ai", "too_many", "model_unavailable", "empty"];

/// An error event's code and params: the local AI's own for a sentence it answered this thread with, else its kind.
fn coded(ev: &mut Value) {
    let own = if ev["error"] == "model_unavailable" { ev["text"].as_str().and_then(local::told) } else { None };
    let (code, params) = match own {
        Some(s) => (s.code, s.params),
        None => (ev["error"].as_str().unwrap_or("").to_string(), json!({})),
    };
    ev["code"] = json!(code);
    ev["params"] = params;
}

/// Answers being written at one time.
const MAX_RUNNING: usize = 4;
/// An answer nobody has asked about for this long is stopped and forgotten.
const IDLE: Duration = Duration::from_secs(120);

struct Job {
    events: Vec<Value>,
    done: bool,
    cancel: Arc<AtomicBool>,
    seen: Instant,
}

#[derive(Clone, Default)]
pub struct Chats(Arc<Mutex<HashMap<String, Job>>>);

impl Chats {
    fn reap(&self) {
        let mut jobs = self.0.lock().unwrap();
        jobs.retain(|_, j| {
            let keep = j.seen.elapsed() < IDLE;
            if !keep {
                j.cancel.store(true, Ordering::SeqCst);
            }
            keep
        });
    }

    /// Starts an answer on its own thread: its id, or an error for a request without messages. With too many answers
    /// already being written, the answer only says so (busy, too_many).
    pub fn start(&self, body: &Value, local: Option<Local>, knowledge: Arc<dyn Knowledge>) -> Result<String, String> {
        self.reap();
        let messages: Vec<Value> = body["messages"].as_array().cloned().ok_or("messages are missing")?;
        let about = body["about"].as_str().map(str::to_string);
        let lang = body["lang"].as_str().unwrap_or("en").chars().take(5).collect::<String>();
        let id = uuid::Uuid::new_v4().simple().to_string();
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut jobs = self.0.lock().unwrap();
            if jobs.values().filter(|j| !j.done).count() >= MAX_RUNNING {
                let ev = json!({"type": "error", "error": "busy", "code": "too_many", "params": {},
                                "text": "Too many answers are being written at once. Wait for one to finish."});
                jobs.insert(id.clone(), Job { events: vec![ev], done: true, cancel, seen: Instant::now() });
                return Ok(id);
            }
            jobs.insert(id.clone(), Job { events: Vec::new(), done: false, cancel: cancel.clone(), seen: Instant::now() });
        }
        let me = self.clone();
        let jid = id.clone();
        std::thread::Builder::new().name("chat".into()).spawn(move || {
            // on this thread, as the assistant asks the local AI: told() knows what it answered
            let mut emit = |mut ev: Value| {
                if ev["type"] == "error" && ev.get("code").is_none() {
                    coded(&mut ev);
                }
                if let Some(j) = me.0.lock().unwrap().get_mut(&jid) {
                    j.events.push(ev);
                }
            };
            let cancelled = || cancel.load(Ordering::SeqCst);
            match local {
                Some(l) => assistant::chat(&messages, about.as_deref(), &lang, &mut emit, &cancelled, knowledge.as_ref(), &LocalModel(l)),
                None => emit(json!({"type": "error", "error": "model_unavailable", "code": "no_local_ai", "params": {}, "text": "There is no local AI in this library."})),
            }
            if let Some(j) = me.0.lock().unwrap().get_mut(&jid) {
                j.done = true;
            }
        }).map_err(|e| e.to_string())?;
        Ok(id)
    }

    /// What is new since event `from`: {events, next, done}, or None for an id not known (or forgotten).
    pub fn poll(&self, id: &str, from: usize) -> Option<Value> {
        self.reap();
        let mut jobs = self.0.lock().unwrap();
        let j = jobs.get_mut(id)?;
        j.seen = Instant::now();
        let events: Vec<Value> = j.events.iter().skip(from).cloned().collect();
        let out = json!({"events": events, "next": j.events.len(), "done": j.done});
        if j.done && from + events.len() >= j.events.len() && !events.is_empty() {
            j.seen = Instant::now() - IDLE + Duration::from_secs(10); // read to the end: forgotten soon
        }
        Some(out)
    }

    pub fn cancel(&self, id: &str) -> bool {
        match self.0.lock().unwrap().get(id) {
            Some(j) => {
                j.cancel.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        }
    }

    /// The whole answer at once, as the `data:` lines chat.js reads, for a page without the polling script.
    pub fn whole(&self, body: &Value, local: Option<Local>, knowledge: Arc<dyn Knowledge>) -> Result<String, String> {
        let id = self.start(body, local, knowledge)?;
        let mut out = String::new();
        let mut from = 0;
        loop {
            let Some(p) = self.poll(&id, from) else { break };
            for ev in p["events"].as_array().into_iter().flatten() {
                out.push_str(&format!("data: {ev}\n\n"));
            }
            from = p["next"].as_u64().unwrap_or(0) as usize;
            if p["done"].as_bool().unwrap_or(true) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(out)
    }
}

/// The page script that turns `fetch('/api/chat')` into polls, so chat.js streams as before. It also tells a page that asks
/// for the location that Lastbook stays offline. Given to the window as an initialization script.
pub const PAGE_SCRIPT: &str = r#"(() => {
  if (window.top !== window || window.origin === 'null') return;
  const none = { getCurrentPosition(ok, no) { if (no) setTimeout(no, 0, { code: 1, message: 'Lastbook stays offline and does not ask where you are.' }) }, watchPosition() { return 0 }, clearWatch() { } };
  try { Object.defineProperty(Navigator.prototype, 'geolocation', { get() { return none }, configurable: false }) } catch { }
  const real = window.fetch.bind(window);
  const own = location.protocol + '//' + location.host;
  const aborted = () => new DOMException('The operation was aborted.', 'AbortError');
  async function polled(body, signal) {
    if (signal && signal.aborted) throw aborted();
    let lang = 'en'; try { lang = localStorage.getItem('lastbook.lang') || document.documentElement.lang || 'en' } catch { }
    let sent = {}; try { sent = JSON.parse(new TextDecoder().decode(body)) } catch { }
    const s = await real('/api/chat/start', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(Object.assign({ lang }, sent)) });
    if (!s.ok) return s;
    const { id } = await s.json();
    let from = 0, over = false, ctl;
    const enc = new TextEncoder();
    const cancel = () => { if (!over) { over = true; real('/api/chat/cancel/' + id, { method: 'POST' }).catch(() => { }) } };
    const stream = new ReadableStream({
      start(c) { ctl = c },
      async pull(c) {
        for (;;) {
          if (over) return;
          let p;
          try { p = await (await real('/api/chat/poll/' + id + '?from=' + from)).json() } catch (e) { over = true; c.error(e); return }
          if (over) return;
          from = p.next;
          const text = (p.events || []).map(ev => 'data: ' + JSON.stringify(ev) + '\n\n').join('');
          if (text) c.enqueue(enc.encode(text));
          if (p.done) { over = true; c.close(); return }
          if (text) return;
          await new Promise(r => setTimeout(r, 120));
        }
      },
      cancel() { cancel() }
    });
    if (signal) signal.addEventListener('abort', () => { cancel(); try { ctl.error(aborted()) } catch { } });
    return new Response(stream, { status: 200, headers: { 'Content-Type': 'text/event-stream' } });
  }
  window.fetch = async function (input, init) {
    const req = new Request(input, init);
    const u = new URL(req.url, location.href);
    if (u.protocol + '//' + u.host === own && req.method === 'POST' && u.pathname === '/api/chat') return polled(await req.arrayBuffer(), req.signal);
    return real(input, init);
  };
})();"#;

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::assistant::NoKnowledge as Nothing;

    /// Every event of one answer, polled to its end.
    fn events(chats: &Chats, id: &str) -> Vec<Value> {
        let (mut got, mut from) = (Vec::new(), 0);
        for _ in 0..500 {
            let p = chats.poll(id, from).unwrap();
            from = p["next"].as_u64().unwrap() as usize;
            got.extend(p["events"].as_array().unwrap().clone());
            if p["done"] == true {
                return got;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("the answer never ended: {got:?}");
    }

    /// An error event's (error, code, params).
    fn said(ev: &Value) -> (&str, &str, &Value) {
        assert_eq!(ev["type"], "error", "{ev}");
        (ev["error"].as_str().unwrap_or(""), ev["code"].as_str().unwrap_or(""), &ev["params"])
    }

    #[test]
    fn start_poll_and_whole() {
        let chats = Chats::default();
        let k: Arc<dyn Knowledge> = Arc::new(Nothing);
        let body = json!({"messages": [{"role": "user", "content": "Ciao"}]});
        let id = chats.start(&body, None, k.clone()).unwrap();
        let last = events(&chats, &id).pop().unwrap();
        assert_eq!(said(&last), ("model_unavailable", "no_local_ai", &json!({})));
        assert_eq!(last["text"], "There is no local AI in this library.", "the English stays beside the code");
        assert!(chats.poll("nope", 0).is_none());
        assert!(chats.start(&json!({}), None, k.clone()).is_err());
        let whole = chats.whole(&body, None, k).unwrap();
        assert!(whole.starts_with("data: {") && whole.contains("model_unavailable") && whole.contains("no_local_ai") && whole.ends_with("\n\n"), "{whole}");
        assert!(PAGE_SCRIPT.contains("/api/chat/start"));
    }

    #[test]
    fn too_many_answers_at_once_is_an_answer_that_says_so() {
        let chats = Chats::default();
        for n in 0..MAX_RUNNING {
            chats.0.lock().unwrap().insert(format!("writing{n}"), Job { events: Vec::new(), done: false, cancel: Arc::new(AtomicBool::new(false)), seen: Instant::now() });
        }
        let k: Arc<dyn Knowledge> = Arc::new(Nothing);
        let body = json!({"messages": [{"role": "user", "content": "Ciao"}]});
        let ev = events(&chats, &chats.start(&body, None, k.clone()).unwrap());
        assert_eq!(ev.len(), 1, "{ev:?}");
        assert_eq!(said(&ev[0]), ("busy", "too_many", &json!({})));
        assert_eq!(ev[0]["text"], "Too many answers are being written at once. Wait for one to finish.");
        assert!(chats.whole(&body, None, k).unwrap().contains("\"too_many\""));
    }

    #[test]
    fn the_local_ai_says_why_it_cannot_answer_by_its_code() {
        let d = std::env::temp_dir().join(format!("lb-chat-local-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let k: Arc<dyn Knowledge> = Arc::new(Nothing);
        let body = json!({"messages": [{"role": "user", "content": "Ciao"}]});
        let chats = Chats::default();
        // nothing looked for yet: not set up
        let l = Local::new(d.clone(), Box::new(Vec::new), None);
        let last = events(&chats, &chats.start(&body, Some(l.clone()), k.clone()).unwrap()).pop().unwrap();
        assert_eq!(said(&last), ("model_unavailable", "not_set_up", &json!({})));
        assert_eq!(last["text"], "The local AI is not set up on this computer.");
        // no runtime in this copy: the state's own message, by its code
        l.discover();
        let last = events(&chats, &chats.start(&body, Some(l.clone()), k.clone()).unwrap()).pop().unwrap();
        assert_eq!(said(&last), ("model_unavailable", "no_runtime", &json!({})));
        assert_eq!(last["text"], l.status()["message"]);
        // a sentence the local AI did not say is said by its kind, as is an answer not written
        let mut ev = json!({"type": "error", "error": "model_unavailable", "text": "Something else went wrong."});
        coded(&mut ev);
        assert_eq!(said(&ev), ("model_unavailable", "model_unavailable", &json!({})));
        let mut ev = json!({"type": "error", "error": "empty", "text": "The model did not produce an answer. The passages it found are listed below."});
        coded(&mut ev);
        assert_eq!(said(&ev), ("empty", "empty", &json!({})));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn the_assistants_own_sentences_have_codes() {
        // the assistant (ai/assistant.rs) fails a round with a few sentences of its own: the local AI knows them by their words
        let mut n = 0;
        for part in include_str!("assistant.rs").split("RoundError::Failed(\"").skip(1) {
            let text = part.split('"').next().unwrap();
            assert!(local::told(text).is_some(), "ai/local.rs has no code for {text:?}");
            n += 1;
        }
        assert!(n >= 2, "the assistant's sentences were not found");
    }

    #[test]
    fn every_code_has_words_in_the_interface() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/locales/en.json");
        let words: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        for code in CODES {
            assert!(words[format!("chat.code.{code}")].is_string(), "web/locales/en.json has no chat.code.{code}");
        }
    }
}
