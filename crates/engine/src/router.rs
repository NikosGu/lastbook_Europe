//! Answers the window's requests: the interface's files under `/ui/`, the app page at `/`, setup at `/api/setup/`, and the
//! engine's other `/api/` routes. Each part of the engine adds its routes here as it is ported from the original Lastbook.

use crate::ai::assistant::{Knowledge, NoKnowledge};
use crate::ai::chat::Chats;
use crate::ai::local::Local;
use crate::http::{Request, Response};
use crate::library::Library;
use crate::map::Map;
use crate::paths::Paths;
use crate::setup::api::Setup;
use crate::static_files;
use serde_json::{json, Value};
use std::sync::Arc;

const MAX_BODY: usize = 64 * 1024;
/// A chat request carries the conversation so far.
const MAX_CHAT_BODY: usize = 256 * 1024;

pub struct Engine {
    pub paths: Paths,
    pub setup: Option<Setup>,
    pub map: Map,
    pub library: Library,
    /// The local AI, once a library is open.
    pub ai: Option<Local>,
    pub chats: Chats,
    /// What the assistant reads: the library and the map.
    pub knowledge: Arc<dyn Knowledge>,
}

impl Engine {
    pub fn new(paths: Paths) -> Engine {
        Engine { map: Map::new(paths.clone()), library: Library::new(paths.clone()), paths, setup: None, ai: None, chats: Chats::default(),
                 knowledge: Arc::new(NoKnowledge) }
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
        if let Some(r) = self.ai_route(method, path, req) {
            return r;
        }
        if self.paths.data.is_some() {
            if let Some(r) = self.map.route(req) {
                return r;
            }
            if let Some(r) = self.library.route(req) {
                return r;
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
            ("GET" | "HEAD", "/api/health") => Response::json(&json!({"ok": true, "engine": "rust", "data": self.paths.data.is_some()})),
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

    pub fn assistant_status(&self) -> Value {
        match &self.ai {
            Some(ai) => {
                let local = ai.status();
                json!({"model": local["name"], "online": ai.available(), "error": null, "local": local, "vision": false, "index": {"state": "off"}})
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
    fn first_page_is_setup_until_there_is_a_library() {
        let page = |e: &Engine| String::from_utf8_lossy(&e.handle(&Request::get("/")).body).into_owned();
        assert!(page(&engine(None)).contains("setup.js"));
        assert!(page(&engine(Some(PathBuf::from("/tmp")))).contains("app.js"));
    }
}
