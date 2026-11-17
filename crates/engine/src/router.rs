//! Answers the window's requests: the interface's files under `/ui/`, the app page at `/`, setup at `/api/setup/`, and the
//! engine's other `/api/` routes. Each part of the engine adds its routes here as it is ported from the original Lastbook.

use crate::ai::assistant::{Knowledge, NoKnowledge};
use crate::ai::chat::Chats;
use crate::ai::local::Local;
use crate::http::{Request, Response};
use crate::knowledge::EngineKnowledge;
use crate::library::Library;
use crate::map::Map;
use crate::paths::Paths;
use crate::setup::api::Setup;
use crate::static_files;
use crate::update::Updater;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MAX_BODY: usize = 64 * 1024;
/// A chat request carries the conversation so far.
const MAX_CHAT_BODY: usize = 256 * 1024;

pub struct Engine {
    pub paths: Paths,
    pub setup: Option<Setup>,
    pub map: Arc<Map>,
    pub library: Arc<Library>,
    /// The local AI, once a library is open.
    pub ai: Option<Local>,
    pub chats: Chats,
    /// What the assistant reads: the library and the map.
    pub knowledge: Arc<dyn Knowledge>,
    /// Check for updates, when the person asks.
    pub updater: Updater,
    /// Whether the library's folder answered, and when that was asked.
    reach: Mutex<(Option<Instant>, bool)>,
}

impl Engine {
    pub fn new(paths: Paths) -> Engine {
        let map = Arc::new(Map::new(paths.clone()));
        let library = Arc::new(Library::new(paths.clone()));
        // with no library open, the assistant has nothing to read
        let knowledge: Arc<dyn Knowledge> = if paths.data.is_some() {
            Arc::new(EngineKnowledge { library: library.clone(), map: map.clone() })
        } else {
            Arc::new(NoKnowledge)
        };
        Engine { map, library, paths, setup: None, ai: None, chats: Chats::default(), knowledge, updater: Updater::product(),
                 reach: Mutex::new((None, true)) }
    }

    pub fn with_ai(mut self, ai: Local) -> Engine {
        self.ai = Some(ai);
        self
    }

    pub fn with_setup(mut self, setup: Setup) -> Engine {
        self.setup = Some(setup);
        self
    }

    pub fn handle(&self, req: &Request) -> Response {
        let method = req.method.as_str();
        let path = req.path.as_str();
        if path.contains('\0') || path.contains('\\') || path.split('/').any(|p| p == "..") {
            return Response::not_found();
        }
        if let Some(name) = path.strip_prefix("/api/setup/") {
            return self.setup_route(method, name.trim_matches('/'), req);
        }
        if let Some(name) = path.strip_prefix("/api/update/") {
            let (status, out) = self.updater.handle(if method == "HEAD" { "GET" } else { method }, name.trim_matches('/'));
            return Response::json_status(status, &out);
        }
        if let Some(r) = self.ai_route(method, path, req) {
            return r;
        }
        if path == "/api/health" {
            return Response::json(&json!({"service": "survival-library", "offline": true, "ok": true, "engine": "rust",
                                         "data": self.paths.data.is_some(), "library": self.library_here()}));
        }
        if self.paths.data.is_some() && !self.library_here() && library_request(path) {
            return Response::json_status(503, &self.unreachable());
        }
        if self.paths.data.is_some() {
            if let Some(r) = self.map.route(req) {
                return r;
            }
            if let Some(r) = self.library.route(req) {
                return if path == "/api/library" { self.with_pins(r) } else { r };
            }
        }
        match (method, path) {
            ("GET" | "HEAD", "/" | "/index.html") => {
                if self.paths.data.is_some() {
                    static_files::serve(&self.paths.web(), "index.html")
                } else {
                    static_files::serve(&self.paths.web(), "setup.html") // no library yet: setup is the whole app
                }
            }
            ("GET" | "HEAD", "/sw.js") => static_files::serve(&self.paths.web(), "sw.js"),
            ("GET" | "HEAD", p) if p.starts_with("/ui/") => static_files::serve(&self.paths.web(), &p["/ui/".len()..]),
            _ => Response::not_found(),
        }
    }

    /// The assistant's status, the local AI's buttons, and the chat (see ai/chat.rs).
    fn ai_route(&self, method: &str, path: &str, req: &Request) -> Option<Response> {
        let body = || -> Result<Value, Response> {
            if req.body.len() > MAX_CHAT_BODY {
                return Err(Response::json_status(413, &json!({"error": "request too large"})));
            }
            match serde_json::from_slice::<Value>(&req.body) {
                Ok(v) if v.is_object() => Ok(v),
                _ if req.body.is_empty() => Ok(json!({})),
                _ => Err(Response::json_status(400, &json!({"error": "bad request"}))),
            }
        };
        Some(match (method, path) {
            ("GET" | "HEAD", "/api/assistant/status") => Response::json(&self.assistant_status()),
            ("POST", "/api/assistant/local") => match (&self.ai, body()) {
                (_, Err(r)) => r,
                (None, _) => Response::json_status(404, &json!({"error": "There is no library open, so there is no local AI."})),
                (Some(ai), Ok(b)) => Response::json(&ai.action(b["action"].as_str().unwrap_or(""))),
            },
            ("POST", "/api/chat/start") => match body() {
                Err(r) => r,
                Ok(b) => match self.chats.start(&b, self.ai.clone(), self.knowledge.clone()) {
                    Ok(id) => Response::json(&json!({"id": id})),
                    Err(e) => Response::json_status(400, &json!({"error": e})),
                },
            },
            ("GET" | "HEAD", p) if p.starts_with("/api/chat/poll/") => {
                let from = req.param("from").and_then(|f| f.parse().ok()).unwrap_or(0);
                match self.chats.poll(&p["/api/chat/poll/".len()..], from) {
                    Some(v) => Response::json(&v),
                    None => Response::json_status(404, &json!({"error": "This answer is no longer kept."})),
                }
            }
            ("POST", p) if p.starts_with("/api/chat/cancel/") => Response::json(&json!({"ok": self.chats.cancel(&p["/api/chat/cancel/".len()..])})),
            ("POST", "/api/chat") => match body() {
                Err(r) => r,
                Ok(b) => match self.chats.whole(&b, self.ai.clone(), self.knowledge.clone()) {
                    Ok(text) => Response::new(200, "text/event-stream; charset=utf-8", text),
                    Err(e) => Response::json_status(400, &json!({"error": e})),
                },
            },
            _ => return None,
        })
    }

    /// Whether the library's folder answers. A disk unplugged or a network share gone (a NAS out of reach) makes every
    /// library request fail: the page is then told so plainly. Asked at most every two seconds.
    pub fn library_here(&self) -> bool {
        let Some(d) = &self.paths.data else { return true };
        let mut r = self.reach.lock().unwrap_or_else(|e| e.into_inner());
        if r.0.is_some_and(|t| t.elapsed() < Duration::from_secs(2)) {
            return r.1;
        }
        let ok = d.join("library-data").is_dir();
        if !ok && r.1 {
            eprintln!("library: {} is not reachable", d.display());
        }
        *r = (Some(Instant::now()), ok);
        ok
    }

    fn unreachable(&self) -> Value {
        let folder = self.paths.data.as_ref().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
        json!({"error": format!("The library in {folder} cannot be reached. Connect its disk or its network, then try again."),
               "code": "library_unreachable", "folder": folder})
    }

    /// /api/library's `pins`: whether the map has help points to show (a library without a pack always offers them).
    fn with_pins(&self, r: Response) -> Response {
        let Ok(mut v) = serde_json::from_slice::<Value>(&r.body) else { return r };
        if r.status == 200 && v.is_object() {
            let layers = self.map.map_contents()["layers"].as_array().is_some_and(|l| !l.is_empty())
                || self.map.map_contents()["layers"].as_object().is_some_and(|l| !l.is_empty());
            v["pins"] = json!(self.map.pack_info().is_none() || layers);
            return Response::json(&v);
        }
        r
    }

    pub fn assistant_status(&self) -> Value {
        match &self.ai {
            Some(ai) => {
                let local = ai.status();
                json!({"model": local["name"], "online": ai.available(), "error": null, "local": local, "vision": false, "index": {"state": "off"},
                       "coverage": self.map.coverage()})
            }
            None => json!({"model": null, "online": false, "error": "no library", "local": null, "vision": false, "index": {"state": "off"}}),
        }
    }

    fn setup_route(&self, method: &str, name: &str, req: &Request) -> Response {
        let Some(setup) = &self.setup else { return Response::not_found() };
        let method = if method == "HEAD" { "GET" } else { method };
        let body = if method == "POST" {
            if req.body.len() > MAX_BODY {
                return Response::json_status(413, &json!({"error": "request too large"}));
            }
            if req.body.is_empty() {
                json!({})
            } else {
                match serde_json::from_slice::<Value>(&req.body) {
                    Ok(v) if v.is_object() => v,
                    _ => return Response::json_status(400, &json!({"error": "bad request"})),
                }
            }
        } else {
            json!({})
        };
        let (status, out) = setup.handle(method, name, &body);
        Response::json_status(status, &out)
    }
}

/// Requests that read the library: its own files and the routes of the map and the library (not setup, the update check,
/// the AI's status, the interface's own files).
fn library_request(path: &str) -> bool {
    let own = ["/api/setup/", "/api/update/", "/api/assistant/", "/api/chat", "/api/health", "/ui/", "/sw.js"];
    path != "/" && path != "/index.html" && !own.iter().any(|o| path.starts_with(o))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn engine(data: Option<PathBuf>) -> Engine {
        Engine::new(Paths::new(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."), data))
    }

    #[test]
    fn interface_files() {
        let e = engine(None);
        let r = e.handle(&Request::get("/ui/setup.js?v=5"));
        assert_eq!(r.status, 200);
        assert!(String::from_utf8_lossy(&r.body).contains("Lastbook setup"));
        assert_eq!(e.handle(&Request::get("/ui/../Cargo.toml")).status, 404);
        assert_eq!(e.handle(&Request::get("/ui/%2e%2e/Cargo.toml")).status, 404);
        assert_eq!(e.handle(&Request::get("/nothing")).status, 404);
        assert_eq!(e.handle(&Request::get("/api/setup/state")).status, 404, "no setup configured");
    }

    #[test]
    fn a_library_out_of_reach_says_so() {
        let d = std::env::temp_dir().join(format!("lb-reach-{}", std::process::id()));
        std::fs::create_dir_all(d.join("library-data")).unwrap();
        std::fs::write(d.join("library-data/catalog.json"), b"[]").unwrap();
        let e = engine(Some(d.clone()));
        assert!(e.library_here());
        assert_ne!(e.handle(&Request::get("/api/topics")).status, 503);
        // the disk is unplugged (here: the folder goes away), and two seconds pass
        std::fs::remove_dir_all(&d).unwrap();
        *e.reach.lock().unwrap() = (None, true);
        let r = e.handle(&Request::get("/api/topics"));
        assert_eq!(r.status, 503);
        assert_eq!(serde_json::from_slice::<Value>(&r.body).unwrap()["code"], "library_unreachable");
        assert_eq!(e.handle(&Request::get("/api/map/info")).status, 503);
        assert_eq!(e.handle(&Request::get("/ui/setup.js")).status, 200, "the interface still loads");
        assert_eq!(serde_json::from_slice::<Value>(&e.handle(&Request::get("/api/health")).body).unwrap()["library"], false);
    }

    /// A library on a network share: on Windows, the same folder reached as \\localhost\D$\... (the drive's
    /// administrative share). Skipped where that share is closed.
    #[cfg(windows)]
    #[test]
    fn a_library_on_a_network_path() {
        let local = std::env::temp_dir().join(format!("lb-unc-{}", std::process::id()));
        std::fs::create_dir_all(local.join("library-data")).unwrap();
        std::fs::write(local.join("library-data/catalog.json"), b"[]").unwrap();
        {
            let c = rusqlite::Connection::open(local.join("library-data/t.sqlite")).unwrap();
            c.execute_batch("CREATE TABLE t(x); INSERT INTO t VALUES (42);").unwrap();
        }
        let s = local.canonicalize().unwrap().to_string_lossy().trim_start_matches(r"\\?\").to_string();
        let unc = PathBuf::from(format!(r"\\localhost\{}${}", &s[..1], &s[2..]));
        if !unc.join("library-data").is_dir() {
            eprintln!("{} cannot be reached here: skipped", unc.display());
            return;
        }
        assert_eq!(crate::setup::volumes::kind(&unc), "network");
        let e = engine(Some(unc.clone()));
        assert!(e.library_here());
        assert_eq!(e.handle(&Request::get("/api/topics")).status, 200);
        let c = crate::db::open_ro(&unc.join("library-data/t.sqlite")).expect("SQLite opens over the network path");
        assert_eq!(c.query_row("SELECT x FROM t", [], |r| r.get::<_, i64>(0)).unwrap(), 42);
        drop(c);
        crate::db::close_all();
        let _ = std::fs::remove_dir_all(&local);
    }

    #[test]
    fn first_page_is_setup_until_there_is_a_library() {
        let page = |e: &Engine| String::from_utf8_lossy(&e.handle(&Request::get("/")).body).into_owned();
        assert!(page(&engine(None)).contains("setup.js"));
        assert!(page(&engine(Some(PathBuf::from("/tmp")))).contains("app.js"));
    }
}
