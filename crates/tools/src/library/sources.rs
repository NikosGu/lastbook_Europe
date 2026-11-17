//! The sources list: what a library is made of, written down so anyone can download the same files and build it again.
//!
//! ```json
//! {"pack": {"id": "starter-li", "title": "...", "summary": "...", "revision": "2026-11-17-v1", "map": "base"},
//!  "documents": [{"id": "who-water-treatment", "file": "who-tn05.pdf", "url": "https://.../the.pdf",
//!                 "sha256": "...", "bytes": 1383781, "title": "...", "publisher": "...", "year": 2013, "lang": "en",
//!                 "licence": "...", "licence_url": "https://...", "source": "https://... (where it is published)",
//!                 "collection": "WHO", "category": "water", "notes": "..."}]}
//! ```
//!
//! `file` is the document's name in the documents folder; `url` and `sha256` say where it was downloaded from and
//! which bytes were taken, so a rebuild uses the same ones. A PDF or a saved web page (.html, .htm).

use lastbook_engine::setup::manifest::{name_ok, sha_ok};
use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;

/// The id of the Sources and permissions page every library gets.
pub const SOURCES_ID: &str = "lastbook-sources";

#[derive(Debug, Clone)]
pub struct Pack {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub revision: String,
    /// The map the pack holds: "base" for none of its own.
    pub map: String,
}

#[derive(Debug, Clone)]
pub struct Doc {
    pub id: String,
    pub file: String,
    pub kind: &'static str,
    pub url: Option<String>,
    pub sha256: Option<String>,
    pub bytes: Option<u64>,
    pub title: String,
    pub publisher: String,
    pub year: Option<String>,
    pub lang: String,
    pub licence: String,
    pub licence_url: Option<String>,
    pub source: String,
    pub collection: String,
    pub category: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Sources {
    pub pack: Pack,
    pub docs: Vec<Doc>,
}

fn text(v: &Value, key: &str, at: &str) -> Result<String, String> {
    match v.get(key).and_then(Value::as_str).map(str::trim) {
        Some(s) if !s.is_empty() => Ok(s.to_string()),
        _ => Err(format!("{at}: \"{key}\" is missing or empty")),
    }
}

fn optional(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

/// The list, checked: every field a library record needs, ids that can name files, no id twice.
pub fn parse(v: &Value) -> Result<Sources, String> {
    let p = v.get("pack").ok_or("the list has no \"pack\"")?;
    let pack = Pack {
        id: text(p, "id", "pack")?,
        title: text(p, "title", "pack")?,
        summary: optional(p, "summary").unwrap_or_default(),
        revision: optional(p, "revision").unwrap_or_default(),
        map: optional(p, "map").unwrap_or_else(|| "base".into()),
    };
    if !name_ok(&pack.id) || !(pack.revision.is_empty() || name_ok(&pack.revision)) || !name_ok(&pack.map) {
        return Err("pack: id, revision and map must be names of letters, digits, '.', '_' and '-'".into());
    }
    let list = v.get("documents").and_then(Value::as_array).ok_or("the list has no \"documents\"")?;
    let mut docs = Vec::new();
    let mut ids = HashSet::new();
    for (i, d) in list.iter().enumerate() {
        let at = format!("document {}", i + 1);
        let id = text(d, "id", &at)?;
        let at = format!("document {id}");
        if !name_ok(&id) || id.chars().any(|c| c.is_ascii_uppercase()) || id == SOURCES_ID {
            return Err(format!("{at}: an id is lower-case letters, digits, '.', '_' and '-', and not {SOURCES_ID}"));
        }
        if !ids.insert(id.clone()) {
            return Err(format!("{at}: the id is used twice"));
        }
        let file = text(d, "file", &at)?;
        if !name_ok(&file) {
            return Err(format!("{at}: \"file\" is a plain file name in the documents folder"));
        }
        let ext = Path::new(&file).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
        let kind = match ext.as_str() {
            "pdf" => "pdf",
            "html" | "htm" => "html",
            _ => return Err(format!("{at}: only PDFs and saved web pages (.html, .htm) for now")),
        };
        let sha256 = optional(d, "sha256").map(|s| s.to_ascii_lowercase());
        if sha256.as_deref().is_some_and(|s| !sha_ok(s)) {
            return Err(format!("{at}: \"sha256\" is 64 hex digits"));
        }
        let lang = text(d, "lang", &at)?.to_ascii_lowercase();
        if lang.len() != 2 || !lang.chars().all(|c| c.is_ascii_lowercase()) {
            return Err(format!("{at}: \"lang\" is a two-letter code such as it, de or en"));
        }
        let year = match d.get("year") {
            Some(Value::Number(n)) => Some(n.to_string()),
            Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
            _ => None,
        };
        docs.push(Doc {
            file,
            kind,
            url: optional(d, "url"),
            sha256,
            bytes: d.get("bytes").and_then(Value::as_u64),
            title: text(d, "title", &at)?,
            publisher: text(d, "publisher", &at)?,
            year,
            lang,
            licence: text(d, "licence", &at)?,
            licence_url: optional(d, "licence_url"),
            source: text(d, "source", &at)?,
            collection: text(d, "collection", &at)?,
            category: text(d, "category", &at)?,
            notes: optional(d, "notes"),
            id,
        });
    }
    if docs.is_empty() {
        return Err("the list has no documents".into());
    }
    Ok(Sources { pack, docs })
}

pub fn load(path: &Path) -> Result<Sources, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v: Value = serde_json::from_slice(&bytes).map_err(|e| format!("{}: not JSON ({e})", path.display()))?;
    parse(&v).map_err(|e| format!("{}: {e}", path.display()))
}
