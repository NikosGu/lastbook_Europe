//! Answers the window's requests: the interface's files under `/ui/`, the app page at `/`, setup at `/api/setup/`, and the
//! engine's other `/api/` routes. Each part of the engine adds its routes here as it is ported from the original Lastbook.

use crate::http::{Request, Response};
use crate::library::Library;
use crate::map::Map;
use crate::paths::Paths;
use crate::setup::api::Setup;
use crate::static_files;
use serde_json::{json, Value};

const MAX_BODY: usize = 64 * 1024;

pub struct Engine {
    pub paths: Paths,
    pub setup: Option<Setup>,
    pub map: Map,
    pub library: Library,
}

impl Engine {
    pub fn new(paths: Paths) -> Engine {
        Engine { map: Map::new(paths.clone()), library: Library::new(paths.clone()), paths, setup: None }
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
