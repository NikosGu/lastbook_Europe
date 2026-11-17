//! The library's `/api/` routes, with the answers the original Lastbook gave (server.py Handler.do_GET and
//! engine/library.py Handler.do_GET).

use super::Library;
use crate::http::{Request, Response};
use regex::Regex;
use serde_json::{json, Value};
use std::sync::LazyLock;

static PAGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/api/page/([^/]+)/(\d+)(\.png)?$").unwrap());

/// What a pack's map holds, as server.py's map_contents looked for it. Until the map is ported, `pins` is whether
/// these files are there at all.
const MAP_FILES: [&str; 5] = ["derived/geo/map.sqlite", "derived/geo/national.sqlite", "library-data/geography/resources/atlas.sqlite", "derived/geo/water.sqlite", "derived/geo/crops.sqlite"];

/// A JSON answer, with the header every library answer has.
pub fn json(v: &Value) -> Response {
    Response::json(v).with("X-Content-Type-Options", "nosniff")
}

pub fn json_status(status: u16, v: &Value) -> Response {
    Response::json_status(status, v).with("X-Content-Type-Options", "nosniff")
}

/// A query parameter, its first 200 characters.
fn param(req: &Request, name: &str, default: &str) -> String {
    req.param(name).filter(|v| !v.is_empty()).unwrap_or_else(|| default.to_string()).chars().take(200).collect()
}

fn offset(req: &Request) -> usize {
    req.param("offset").and_then(|v| v.trim().parse::<i64>().ok()).unwrap_or(0).max(0) as usize
}

/// The interface's language for a search: `lang`, else English.
fn lang(req: &Request) -> String {
    param(req, "lang", "en")
}

impl Library {
    /// One of the library's `/api/` routes, or None.
    pub(super) fn api(&self, req: &Request) -> Option<Response> {
        let path = req.path.as_str();
        let cat = || self.catalog();
        Some(match path {
            "/api/health" => json(&json!({"service": "survival-library", "offline": true, "ok": true, "engine": "rust", "data": true})),
            "/api/topics" => json(&Value::Array(self.topics_resolved())),
            "/api/find" => json(&self.find(&param(req, "q", ""), &param(req, "kind", "all"), &lang(req))),
            "/api/search" => json(&Value::Array(self.simple_search(&req.param("q").unwrap_or_default(), 40))),
            "/api/retrieve" => {
                // the assistant's search, keyword half only until the meaning-based index is ported
                let k = req.param("k").and_then(|k| k.parse::<usize>().ok()).unwrap_or(8).min(20);
                let q = param(req, "q", "");
                json(&json!({"q": req.param("q").unwrap_or_default(), "results": self.search(&q, k, &param(req, "kind", "any"), &lang(req)),
                             "semantic": {"state": "off", "chunks": 0, "docs": 0, "pending": null, "rate": null,
                                          "note": "Meaning-based search is off: no model in this library. Keyword search still works."}}))
            }
            "/api/videos" => json(&self.videos_home()),
            "/api/shelves" => json(&self.shelves()),
            "/api/collection" => json(&self.collection_items(&req.param("name").unwrap_or_default(), offset(req))),
            "/api/library" => json(&self.library_info()),
            "/api/download-status" => json(&self.download_status()),
            "/api/collection-search" => self.collection_search(&req.param("q").unwrap_or_default()),
            _ => {
                if let Some(id) = path.strip_prefix("/api/doc/") {
                    return Some(match cat().get(id) {
                        Some(r) => json(&self.public(r, true)),
                        None => json_status(404, &json!({"error": "This item is not in the library catalog."})),
                    });
                }
                if let Some(m) = PAGE.captures(path) {
                    let Some(r) = cat().get(&m[1]).cloned() else { return Some(json_status(404, &json!({"error": "unknown document"}))) };
                    let n: usize = m[2].parse().unwrap_or(usize::MAX);
                    if m.get(3).is_some() {
                        if r.kind != "pdf" {
                            return Some(json_status(400, &json!({"error": "not a PDF"})));
                        }
                        let w = req.param("w").and_then(|w| w.trim().parse::<i64>().ok()).unwrap_or(1000);
                        return Some(self.page_png(&r, n, w));
                    }
                    let ps = self.page_texts(&r);
                    let text = if n >= 1 && n <= ps.len() { ps[n - 1].as_str() } else { "" };
                    return Some(json(&json!({"page": m[2].parse::<u64>().unwrap_or(0), "pages": ps.len(), "text": text})));
                }
                if let Some(id) = path.strip_prefix("/api/transcript/") {
                    return Some(match self.transcript(id) {
                        Some(t) => json(&t),
                        None => json_status(404, &json!({"error": "unknown video"})),
                    });
                }
                if let Some(id) = path.strip_prefix("/api/videos/related/") {
                    return Some(json(&self.videos_related(id)));
                }
                if let Some(row) = path.strip_prefix("/api/videos/") {
                    return Some(json(&self.videos_list(row, &param(req, "q", ""), &param(req, "len", ""), &param(req, "sort", ""), offset(req))));
                }
                if let Some(id) = path.strip_prefix("/api/shelf/") {
                    return Some(json(&self.shelf(id, &param(req, "kind", "all"), &param(req, "q", ""), offset(req))));
                }
                return None;
            }
        })
    }

    /// What this library holds, for the interface (`/api/library`). `pins` is whether the map has help pins: for now,
    /// whether a pack has any of the map's files (the map's own module will say what they hold).
    pub fn library_info(&self) -> Value {
        let lib = self.lib().unwrap_or_default();
        let data = self.paths.data.clone().unwrap_or_default();
        let s: Value = std::fs::read(lib.join("summary.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(json!({}));
        let mut out = serde_json::Map::new();
        for k in ["resources", "pdfs", "pdf_pages", "web_pages", "videos", "collections", "document_bytes"] {
            out.insert(k.into(), s.get(k).cloned().unwrap_or(Value::Null));
        }
        let cat = self.catalog();
        let sources = cat.by_path("documents/lastbook/sources-and-permissions.html").map(|r| json!(r.id)).unwrap_or(Value::Null);
        let pack = std::fs::read(data.join("pack.json")).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).is_some_and(|v| !v.is_null());
        out.insert("zims".into(), json!(self.zim_names()));
        out.insert("sources".into(), sources);
        out.insert("pack".into(), json!(pack));
        out.insert("pins".into(), json!(!pack || MAP_FILES.iter().any(|f| data.join(f).is_file())));
        Value::Object(out)
    }

    /// The library's own download progress files, as they are.
    pub fn download_status(&self) -> Value {
        let lib = self.lib().unwrap_or_default();
        let mut out = serde_json::Map::new();
        for (key, rel) in [("channels", "channel-archive-status.json"), ("collections", "manifests/kiwix-progress.json"), ("transcripts", "transcript-status.json"), ("subjects", "subject-video-status.json")] {
            if let Some(mut v) = std::fs::read(lib.join(rel)).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
                if key == "transcripts" {
                    if let Some(o) = v.as_object_mut() {
                        o.remove("videos");
                    }
                }
                out.insert(key.into(), v);
            }
        }
        Value::Object(out)
    }
}
