//! The sources list: what a library is made of, written down so anyone can download the same files and build it again.
//!
//! ```json
//! {"pack": {"id": "starter-li", "title": "...", "summary": "...", "revision": "2026-11-17-v1", "map": "base",
//!           "redistributable": {"summary": "...", "revision": "2026-11-17-redistributable-v1"}},
//!  "documents": [{"id": "who-water-treatment", "file": "who-tn05.pdf", "url": "https://.../the.pdf",
//!                 "sha256": "...", "bytes": 1383781, "title": "...", "publisher": "...", "year": 2013, "lang": "en",
//!                 "licence": "...", "licence_url": "https://...", "redistribute": "no",
//!                 "terms_quote": "© World Health Organization 2013. All rights reserved.", "conditions": "...",
//!                 "source": "https://... (where it is published)", "collection": "WHO", "category": "water", "notes": "..."}]}
//! ```
//!
//! `file` is the document's name in the documents folder; `url` and `sha256` say where it was downloaded from and
//! which bytes were taken, so a rebuild uses the same ones. A PDF or a saved web page (.html, .htm).
//!
//! The terms: `licence` names them, `licence_url` is where the publisher states them, and `redistribute` is what they
//! allow: "yes" only when they clearly allow copying the document and passing it on (a non-commercial licence included,
//! Lastbook being free; `conditions` says what they ask in return), "no" when they do not, "unclear" when nothing
//! settles it. `terms_quote` is the sentence that decides, in the publisher's words. A pack of only the documents marked
//! "yes" (build-library --redistributable-only) takes its summary and content revision from `pack.redistributable`.
//!
//! `alt_text`, for a PDF whose pictures carry words its file gives no alt text for: page numbers (from 1) to the words of
//! each picture on that page, as printed, e.g. `{"17": ["AUTOPROTECCIÓN: Altas temperaturas. Limita tu exposición…"]}`.
//! They are written out for this library, so the Sources page says so; the build adds them to those pages as it adds a
//! tagged PDF's own alt text, and stops if the file has alt text of its own there or no such page.

use lastbook_engine::setup::manifest::{name_ok, sha_ok};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
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
    /// The summary and content revision of the pack of only the documents that may be passed on.
    pub redistributable: Option<(String, String)>,
}

/// What a document's terms allow, about copying it and passing it on.
pub const REDISTRIBUTE: [&str; 3] = ["yes", "no", "unclear"];

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
    /// "yes", "no" or "unclear" (REDISTRIBUTE); None where the list does not say.
    pub redistribute: Option<&'static str>,
    pub terms_quote: Option<String>,
    pub conditions: Option<String>,
    pub source: String,
    pub collection: String,
    pub category: String,
    pub notes: Option<String>,
    /// Alt text written out for this library: page (from 1) -> the words of each picture on it, as printed.
    pub alt_text: BTreeMap<usize, Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct Sources {
    pub pack: Pack,
    pub docs: Vec<Doc>,
}

impl Sources {
    /// The pack of only the documents marked "redistribute": "yes", with the summary and content revision of
    /// `pack.redistributable`. Every other document is left out whole: its file, its text, its record and its index
    /// rows are not built.
    pub fn redistributable(&self) -> Result<Sources, String> {
        let (summary, revision) = self
            .pack
            .redistributable
            .clone()
            .ok_or("pack: \"redistributable\" (the summary and revision of the pack of only the documents that may be passed on) is missing")?;
        let docs: Vec<Doc> = self.docs.iter().filter(|d| d.redistribute == Some("yes")).cloned().collect();
        if docs.is_empty() {
            return Err("no document is marked \"redistribute\": \"yes\"".into());
        }
        Ok(Sources { pack: Pack { summary, revision, redistributable: None, ..self.pack.clone() }, docs })
    }
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

/// A document's `alt_text`: page numbers as written ("17", not "017" or "0") to one or more texts, each with words.
fn alt_text(d: &Value, at: &str, kind: &str) -> Result<BTreeMap<usize, Vec<String>>, String> {
    let Some(v) = d.get("alt_text") else { return Ok(BTreeMap::new()) };
    let bad = || format!("{at}: \"alt_text\" maps page numbers (from 1) to the words of each picture on the page, e.g. {{\"17\": [\"...\"]}}");
    let pages = v.as_object().ok_or_else(bad)?;
    if kind != "pdf" {
        return Err(format!("{at}: \"alt_text\" is for a PDF's pages"));
    }
    let mut out = BTreeMap::new();
    for (k, v) in pages {
        let page = k.parse::<usize>().ok().filter(|n| *n >= 1 && n.to_string() == *k).ok_or_else(bad)?;
        let texts: Option<Vec<String>> = v.as_array().ok_or_else(bad)?.iter().map(|t| t.as_str().map(str::trim).filter(|t| super::extract::letters(t) > 0).map(str::to_string)).collect();
        match texts {
            Some(texts) if !texts.is_empty() => out.insert(page, texts),
            _ => return Err(bad()),
        };
    }
    Ok(out)
}

/// The list, checked: every field a library record needs, ids that can name files, no id twice.
pub fn parse(v: &Value) -> Result<Sources, String> {
    let p = v.get("pack").ok_or("the list has no \"pack\"")?;
    let redistributable = match p.get("redistributable") {
        None => None,
        Some(r) => Some((text(r, "summary", "pack.redistributable")?, text(r, "revision", "pack.redistributable")?)),
    };
    let pack = Pack {
        id: text(p, "id", "pack")?,
        title: text(p, "title", "pack")?,
        summary: optional(p, "summary").unwrap_or_default(),
        revision: optional(p, "revision").unwrap_or_default(),
        map: optional(p, "map").unwrap_or_else(|| "base".into()),
        redistributable,
    };
    let mut revisions = std::iter::once(pack.revision.as_str()).chain(pack.redistributable.iter().map(|r| r.1.as_str()));
    if !name_ok(&pack.id) || revisions.any(|r| !(r.is_empty() || name_ok(r))) || !name_ok(&pack.map) {
        return Err("pack: id, revision and map must be names of letters, digits, '.', '_' and '-'".into());
    }
    if pack.redistributable.as_ref().is_some_and(|r| r.1 == pack.revision) {
        return Err("pack.redistributable: its revision is the whole pack's, and two different packs must not share one".into());
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
        let redistribute = match optional(d, "redistribute") {
            None => None,
            Some(r) => Some(*REDISTRIBUTE.iter().find(|&&x| x == r).ok_or(format!("{at}: \"redistribute\" is yes, no or unclear"))?),
        };
        let terms_quote = optional(d, "terms_quote");
        if redistribute.is_some() && (terms_quote.is_none() || optional(d, "licence_url").is_none()) {
            return Err(format!("{at}: a \"redistribute\" decision needs the address of the terms (\"licence_url\") and the sentence that decides (\"terms_quote\")"));
        }
        let year = match d.get("year") {
            Some(Value::Number(n)) => Some(n.to_string()),
            Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
            _ => None,
        };
        let alt_text = alt_text(d, &at, kind)?;
        docs.push(Doc {
            alt_text,
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
            redistribute,
            terms_quote,
            conditions: optional(d, "conditions"),
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
