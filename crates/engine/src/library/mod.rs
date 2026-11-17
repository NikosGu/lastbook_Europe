//! The library: the catalog, topics, search, shelves, videos and transcripts, PDF pages, encyclopedias (ZIM), saved web
//! pages, and the library's own files. Ported from the original Lastbook's server.py, engine/library.py and
//! engine/saved_pages.py.
//!
//! Nothing in `library-data/` is written. Page images are kept in `derived/cache/`.

mod api;
mod browse;
pub mod catalog;
pub mod files;
pub mod find;
pub mod pdf;
pub mod saved_pages;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_files;
#[cfg(test)]
mod tests_zim;
mod text;
pub mod topics;
pub mod words;
pub mod zim;

use crate::http::{Request, Response};
use crate::paths::Paths;
use catalog::{Catalog, Record};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::SystemTime;
use words::Words;

/// Answers of find by (query, kind, language), with the catalog and the search index's modified time they came from.
type FindCache = HashMap<(String, String, String), (Weak<Catalog>, Option<SystemTime>, Value)>;
/// The catalog's original addresses and the library's copy of each, for the catalog they came from.
type Links = Option<(Weak<Catalog>, Arc<HashMap<String, String>>)>;

/// What the catalog was read from: catalog.json's modified time and size, and video-exclusions.json's modified time.
type Stamp = (Option<(SystemTime, u64)>, Option<SystemTime>);

struct Loaded {
    stamp: Stamp,
    excluded: HashSet<String>,
    catalog: Arc<Catalog>,
}

pub struct Library {
    pub paths: Paths,
    /// Each language's search words, read at start from the program's `content/search`.
    pub words: Words,
    loaded: RwLock<Option<Loaded>>,
    loading: Mutex<()>,
    /// Answers of find, for the catalog and the search index they came from.
    find_cache: Mutex<FindCache>,
    /// The catalog's original addresses and the library's copy of each, for saved pages' links.
    links_cache: Mutex<Links>,
    pdf_sizes: Mutex<files::PdfSizes>,
    zims: zim::Open,
    /// False to behave as if PDFium were missing (tests).
    pub draw_pdf: bool,
    /// Whether the first request starts drawing the guides' page pictures (not in tests).
    pub prewarm: bool,
    prewarm_started: std::sync::atomic::AtomicBool,
}

impl Library {
    pub fn new(paths: Paths) -> Library {
        let words = Words::load(&paths.content().join("search"));
        Library { paths, words, loaded: RwLock::new(None), loading: Mutex::new(()), find_cache: Mutex::new(HashMap::new()),
                  links_cache: Mutex::new(None), pdf_sizes: Mutex::new(HashMap::new()), zims: Mutex::new(Vec::new()), draw_pdf: true,
                  prewarm: !cfg!(test), prewarm_started: std::sync::atomic::AtomicBool::new(false) }
    }

    /// The library folder, `<data>/library-data`, or None with no data folder.
    pub fn lib(&self) -> Option<PathBuf> {
        self.paths.data.as_ref().map(|d| d.join("library-data"))
    }

    /// The catalog, read again when catalog.json or video-exclusions.json has changed. A catalog.json that does not
    /// parse (being rewritten right now) leaves the copy already read; with none, the catalog is empty.
    pub fn catalog(&self) -> Arc<Catalog> {
        let Some(lib) = self.lib() else { return Arc::new(Catalog::default()) };
        let (cat_file, ex_file) = (lib.join("catalog.json"), lib.join("video-exclusions.json"));
        let stamp_of = || -> Stamp {
            let c = std::fs::metadata(&cat_file).ok().and_then(|m| Some((m.modified().ok()?, m.len())));
            (c, std::fs::metadata(&ex_file).ok().and_then(|m| m.modified().ok()))
        };
        let stamp = stamp_of();
        if let Some(l) = self.loaded.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
            if l.stamp == stamp {
                return l.catalog.clone();
            }
        }
        let _one = self.loading.lock().unwrap_or_else(|e| e.into_inner());
        let previous = self.loaded.read().unwrap_or_else(|e| e.into_inner()).as_ref().map(|l| (l.stamp, l.excluded.clone(), l.catalog.clone()));
        if let Some((s, _, c)) = &previous {
            if *s == stamp {
                return c.clone();
            }
        }
        let excluded = match std::fs::read(&ex_file) {
            Err(_) => HashSet::new(),
            Ok(b) => match serde_json::from_slice::<Value>(&b) {
                Ok(v) => catalog::exclusions(&v),
                Err(_) => previous.as_ref().map(|p| p.1.clone()).unwrap_or_default(), // being rewritten: the list already read
            },
        };
        let list = match std::fs::read(&cat_file).map(|b| serde_json::from_slice::<Value>(&b)) {
            Ok(Ok(Value::Array(list))) => list,
            Err(_) => Vec::new(),
            Ok(_) => return previous.map(|p| p.2).unwrap_or_default(),
        };
        let catalog = Arc::new(Catalog::new(list, &excluded));
        *self.loaded.write().unwrap_or_else(|e| e.into_inner()) = Some(Loaded { stamp, excluded, catalog: catalog.clone() });
        catalog
    }

    /// What the interface is shown of a record (server.py public).
    pub fn public(&self, r: &Record, full: bool) -> Value {
        let lib = self.lib().unwrap_or_default();
        self.catalog().public(r, full, &lib)
    }

    /// The answer to a library request (`/api/...` for the library, the library's files, `/zim/...`), or None if the
    /// request is not the library's.
    pub fn route(&self, req: &Request) -> Option<Response> {
        self.paths.data.as_ref()?;
        if req.method != "GET" && req.method != "HEAD" {
            return None; // the library has no POST routes
        }
        if self.prewarm && !self.prewarm_started.swap(true, std::sync::atomic::Ordering::Relaxed) {
            self.start_prewarm();
        }
        let r = match std::panic::catch_unwind(AssertUnwindSafe(|| self.answer(req))) {
            Ok(r) => r?,
            Err(_) => api::json_status(500, &serde_json::json!({"error": "Server error (panic)."})), // a readable answer, naming no file
        };
        Some(if req.method == "HEAD" { head(r) } else { r })
    }

    /// Draws small pictures of the pages the guides point to, in the background, so guide pages show them at once
    /// (server.py prewarm_previews). The thread has a library of its own, on the same folders.
    fn start_prewarm(&self) {
        let paths = self.paths.clone();
        let _ = std::thread::Builder::new().name("lastbook-previews".into()).spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(5));
            Library::new(paths).prewarm_previews();
        });
    }

    fn answer(&self, req: &Request) -> Option<Response> {
        let path = req.path.as_str();
        if path.starts_with("/api/") {
            return self.api(req);
        }
        if path.starts_with("/zim/") {
            return Some(self.serve_zim(req));
        }
        if path == "/reader.css" || files::LIBRARY_PREFIXES.iter().any(|p| path.starts_with(p)) {
            return Some(self.serve_file(req));
        }
        None
    }
}

/// A HEAD answer: the headers of the GET answer, with its length and no body.
fn head(mut r: Response) -> Response {
    if r.header("Content-Length").is_none() {
        let n = r.body.len().to_string();
        r = r.with("Content-Length", &n);
    }
    r.body.clear();
    r
}
