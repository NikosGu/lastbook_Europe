//! Search in the library's full-text index (`library-data/search.sqlite`, an FTS5 table `pages`): the interface's
//! plain-language search (`find`), the keyword half of the assistant's search (`search`), and the plain search of
//! engine/library.py (`simple_search`). Ported from the original Lastbook's server.py (find, clean_snip) and
//! assistant.py (_fts, hybrid).
//!
//! The original Lastbook halved the score of Spanish documents; here a document in the interface's language is preferred
//! instead: one in any other language counts half.

use super::catalog::Record;
use super::words::{fold, fts_expr};
use super::Library;
use crate::db;
use regex::Regex;
use rusqlite::types::Value as Sql;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, LazyLock, Weak};

static CLINICAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)surg|clinical|district hospital|essential drugs|tuberculosis|obstetric|cholera epidemic|measles epidemic|anaesth|war surgery").unwrap()
});
static GENERAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)first aid|emergency|basic|household|home|beginner|guide|handbook|where there is no").unwrap());
static INDEX_PAGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^index$|front matter|table of contents|credits|^contents").unwrap());
static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
static PLAIN_TERM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\w-]+").unwrap());
static QWORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\p{L}\p{N}-]{3,}").unwrap());
static MODEL_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\p{L}\p{N}-]+").unwrap());

/// Written for surgeons and clinicians: still found, but after general guidance unless the question asks for it.
pub fn is_clinical(text: &str) -> bool {
    CLINICAL.is_match(text)
}

/// The kinds a `kind=` filter keeps, or None for all.
fn kind_filter(kind: &str) -> Option<&'static [&'static str]> {
    match kind {
        "read" | "manual" => Some(&["pdf", "html", "pptx"]),
        "video" => Some(&["video", "transcript"]),
        _ => None,
    }
}

fn sql_json(v: Sql) -> Value {
    match v {
        Sql::Null => Value::Null,
        Sql::Integer(i) => json!(i),
        Sql::Real(f) => json!(f),
        Sql::Text(s) => json!(s),
        Sql::Blob(_) => Value::Null,
    }
}

/// A snippet as the interface shows it: one line, escaped, with the matched words in `<mark>`.
pub fn clean_snip(s: &str) -> String {
    let one = SPACES.replace_all(s, " ");
    let mut out = String::with_capacity(one.len());
    for c in one.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            '\u{1}' => out.push_str("<mark>"),
            '\u{2}' => out.push_str("</mark>"),
            c => out.push(c),
        }
    }
    out
}

/// The first n characters.
fn chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// One kept answer for `find`: a document, its score and its best pages.
struct Doc {
    r: Arc<Record>,
    score: f64,
    n: usize,
    hits: Vec<(Value, Option<String>)>,
}

impl Library {
    /// Rows from the index for the query groups: all the ideas, and if that finds few, any of them (the relaxed rows are
    /// added after). A query the index cannot take finds nothing.
    fn fts_rows(&self, select: &str, groups: &[Vec<String>], limit: usize, few: usize) -> (Vec<Vec<Sql>>, bool) {
        let Some(con) = self.lib().and_then(|l| db::open_ro(&l.join("search.sqlite"))) else { return (Vec::new(), false) };
        let sql = format!("SELECT {select} FROM pages WHERE pages MATCH ?1 ORDER BY bm25(pages,0,6,1,1,0,0,1) LIMIT {limit}");
        let run = |expr: &str| -> rusqlite::Result<Vec<Vec<Sql>>> {
            let mut st = con.prepare_cached(&sql)?;
            let n = st.column_count();
            let rows = st.query_map([expr], |row| (0..n).map(|i| row.get::<_, Sql>(i)).collect::<rusqlite::Result<Vec<Sql>>>())?;
            rows.collect()
        };
        let Ok(mut rows) = run(&fts_expr(groups)) else { return (Vec::new(), false) };
        let mut relaxed = false;
        if rows.len() < few && groups.len() > 1 {
            relaxed = true;
            let any = groups.iter().map(|g| format!("({})", fts_expr(std::slice::from_ref(g)))).collect::<Vec<_>>().join(" OR ");
            match run(&any) {
                Ok(more) => rows.extend(more),
                Err(_) => return (Vec::new(), relaxed),
            }
        }
        (rows, relaxed)
    }

    /// The interface's search (server.py find): documents grouped from their matching pages, ranked by the index,
    /// how far their source is trusted, the query's words in their title and a preference for `lang`. `kind` is
    /// `read`, `video` or `all`.
    pub fn find(&self, q: &str, kind: &str, lang: &str) -> Value {
        let lang = self.words.code(lang);
        let cat = self.catalog();
        let stamp = self.lib().and_then(|l| std::fs::metadata(l.join("search.sqlite")).ok()).and_then(|m| m.modified().ok());
        let key = (q.to_string(), kind.to_string(), lang.clone());
        {
            let cache = self.find_cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((c, s, v)) = cache.get(&key) {
                if Weak::ptr_eq(c, &Arc::downgrade(&cat)) && *s == stamp {
                    return v.clone();
                }
            }
        }
        let out = self.find_now(q, kind, &lang, 30);
        let mut cache = self.find_cache.lock().unwrap_or_else(|e| e.into_inner());
        if cache.len() >= 256 {
            cache.clear();
        }
        cache.insert(key, (Arc::downgrade(&cat), stamp, out.clone()));
        out
    }

    fn find_now(&self, q: &str, kind: &str, lang: &str, limit: usize) -> Value {
        let (content, groups) = self.words.query_groups(q, lang);
        if groups.is_empty() {
            return json!({"q": q, "results": [], "expanded": []});
        }
        let (rows, relaxed) = self.fts_rows("doc_id,page,snippet(pages,6,char(1),char(2),' … ',28),bm25(pages,0,6,1,1,0,0,1)", &groups, 600, 8);
        let cat = self.catalog();
        let allowed = kind_filter(kind);
        let mut docs: Vec<Doc> = Vec::new();
        let mut at: HashMap<String, usize> = HashMap::new();
        for row in rows {
            let mut row = row.into_iter();
            let (Some(Sql::Text(doc_id)), Some(page), Some(snip), Some(score)) = (row.next(), row.next(), row.next(), row.next()) else { continue };
            let score = match score {
                Sql::Real(f) => f,
                Sql::Integer(i) => i as f64,
                _ => 0.0,
            };
            let snip = match snip {
                Sql::Text(s) => Some(s),
                _ => None,
            };
            let Some(mut r) = cat.get(&doc_id).cloned() else { continue };
            let mut page = sql_json(page);
            if r.kind == "metadata" {
                continue;
            }
            if r.kind == "transcript" {
                // the video itself, with the transcript's words
                if let Some(v) = cat.by_path(r.str("video_path")).filter(|_| r.truthy("video_path")) {
                    r = v.clone();
                    page = Value::Null;
                }
            }
            if allowed.is_some_and(|a| !a.contains(&r.kind.as_str())) {
                continue;
            }
            let i = *at.entry(r.id.clone()).or_insert_with(|| {
                docs.push(Doc { r: r.clone(), score: 0.0, n: 0, hits: Vec::new() });
                docs.len() - 1
            });
            let d = &mut docs[i];
            if d.hits.len() < 3 && !d.hits.iter().any(|h| h.0 == page) {
                d.hits.push((if r.kind == "pdf" { page } else { Value::Null }, snip));
            }
            d.n += 1;
            if d.n == 1 {
                d.score = -score;
            } else if d.n <= 4 {
                d.score += -score * 0.1; // a little credit for a few more matching pages, not for sheer length
            }
        }
        let folded: Vec<Vec<String>> = groups.iter().map(|g| g.iter().map(|w| fold(w)).collect()).collect();
        let asks_clinical = is_clinical(q);
        let mut ranked: Vec<(f64, Doc)> = docs
            .into_iter()
            .map(|d| {
                let r = &d.r;
                let title = fold(&r.title);
                let tb = folded.iter().filter(|g| g.iter().any(|x| title.contains(x.as_str()))).count() as f64 / groups.len() as f64;
                let mut s = d.score * r.trust.1 * (1.0 + tb);
                if !r.in_lang(lang) {
                    s *= 0.5;
                }
                if is_clinical(&format!("{} {}", r.title, r.str("collection"))) && !asks_clinical {
                    s *= 0.6;
                }
                if INDEX_PAGE.is_match(&r.clean_title()) {
                    s *= 0.3; // index pages match everything but rarely answer anything
                }
                if GENERAL.is_match(&r.title) {
                    s *= 1.25;
                }
                (s, d)
            })
            .collect();
        ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let lib = self.lib().unwrap_or_default();
        let results: Vec<Value> = ranked
            .iter()
            .take(limit)
            .map(|(_, d)| {
                let mut p = cat.public(&d.r, false, &lib);
                p["hits"] = d.hits.iter().map(|(page, snip)| json!({"page": page, "snippet": clean_snip(snip.as_deref().unwrap_or(""))})).collect();
                p
            })
            .collect();
        let content: BTreeSet<&String> = content.iter().collect();
        let expanded: BTreeSet<&String> = groups.iter().flat_map(|g| g.iter().skip(1)).filter(|w| !content.contains(w)).collect();
        json!({"q": q, "mode": if relaxed { "some words" } else { "all words" }, "expanded": expanded, "results": results})
    }

    /// The best passages for a query from the full-text index, as the assistant's hybrid search gives them without its
    /// meaning-based half (assistant.py hybrid): at most two per document, with where they came from. `kind` is `any`,
    /// `manual` or `video`; `lang` the interface's language.
    pub fn search(&self, q: &str, k: usize, kind: &str, lang: &str) -> Vec<Value> {
        self.search_in(q, k, kind, lang, None)
    }

    /// [`Library::search`] within one document when `doc` is given.
    pub fn search_in(&self, q: &str, k: usize, kind: &str, lang: &str, doc: Option<&str>) -> Vec<Value> {
        let lang = self.words.code(lang);
        let cat = self.catalog();
        let allowed = |r: &Record| r.kind != "metadata" && kind_filter(kind).is_none_or(|a| a.contains(&r.kind.as_str())) && doc.is_none_or(|d| d == r.id);
        let (_, groups) = self.words.query_groups(q, &lang);
        let rows = if groups.is_empty() { Vec::new() } else { self.fts_rows("doc_id,page,snippet(pages,6,'','',' … ',40)", &groups, 120, 10).0 };
        // the keyword list: (document, page for a PDF, excerpt), each passage once at its best rank
        let mut keys: Vec<(String, Value)> = Vec::new();
        let mut text: HashMap<(String, String), String> = HashMap::new();
        for row in rows {
            let mut row = row.into_iter();
            let (Some(Sql::Text(doc_id)), Some(page), Some(snip)) = (row.next(), row.next(), row.next()) else { continue };
            let Some(mut r) = cat.get(&doc_id).cloned() else { continue };
            let mut page = sql_json(page);
            if r.kind == "transcript" {
                let Some(v) = cat.by_path(r.str("video_path")).filter(|_| r.truthy("video_path")) else { continue };
                r = v.clone();
                page = Value::Null;
            }
            if !allowed(&r) {
                continue;
            }
            let page = if r.kind == "pdf" { page } else { Value::Null };
            let key = (r.id.clone(), page.to_string());
            if let std::collections::hash_map::Entry::Vacant(e) = text.entry(key) {
                e.insert(if let Sql::Text(s) = snip { s } else { String::new() });
                keys.push((r.id.clone(), page));
            }
        }
        let fq = fold(q);
        let qwords: Vec<String> = QWORD.find_iter(&fq).map(|m| m.as_str().to_string()).filter(|w| !self.words.is_stop(w, &lang)).collect();
        let models: Vec<String> = MODEL_WORD
            .find_iter(&fq)
            .map(|m| m.as_str().to_string())
            .filter(|w| w.chars().any(|c| c.is_numeric()) && w.chars().any(|c| c.is_alphabetic()) && w.chars().count() >= 3)
            .collect();
        let asks_clinical = is_clinical(q);
        let mut out: Vec<(f64, usize)> = keys
            .iter()
            .enumerate()
            .map(|(rank, (d, _))| {
                let r = &cat.get(d).unwrap();
                let mut score = 1.0 / (60.0 + rank as f64 + 1.0) * (0.75 + 0.25 * r.trust.1);
                if is_clinical(&format!("{} {}", r.title, r.str("collection"))) && !asks_clinical {
                    score *= 0.55; // surgery and clinician texts after general guidance unless asked for
                }
                let model = if r.truthy("model") { super::catalog::py_str(r.get("model")) } else { String::new() };
                let title = fold(&format!("{} {}", r.title, model));
                if !models.is_empty() && models.iter().any(|m| title.contains(m.as_str())) {
                    score *= 3.0; // the exact equipment model named in the question
                } else if qwords.iter().filter(|w| title.contains(w.as_str())).count() >= 2.max(qwords.len() / 2) {
                    score *= 1.3;
                }
                if !r.in_lang(&lang) {
                    score *= 0.5;
                }
                (score, rank)
            })
            .collect();
        out.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let lib = self.lib().unwrap_or_default();
        let mut res = Vec::new();
        let mut per_doc: HashMap<&str, usize> = HashMap::new();
        for (score, rank) in out {
            let (d, page) = &keys[rank];
            let n = per_doc.entry(d.as_str()).or_insert(0);
            if *n >= 2 {
                continue; // variety: at most two passages per document
            }
            *n += 1;
            let r = cat.get(d).unwrap();
            let p = cat.public(r, false, &lib);
            let excerpt = SPACES.replace_all(&text[&(d.clone(), page.to_string())], " ").into_owned();
            res.push(json!({
                "doc_id": d, "title": p["title"], "publisher": p["publisher"], "year": p["year"], "kind": r.kind, "trust": p["trust"],
                "pages": p["pages"], "page": page, "t": null, "duration": p["duration"], "thumb": p.get("thumb"),
                "excerpt": chars(&excerpt, 900), "found_by": {"keyword": rank + 1}, "score": ((score * 1000.0) * 100.0).round_ties_even() / 100.0,
                "notes": chars(r.str("notes"), 300), "imageOnly": r.image_only(),
            }));
            if res.len() >= k {
                break;
            }
        }
        res
    }

    /// engine/library.py's plain search (`/api/search`): every word, the matching pages as the index has them.
    pub fn simple_search(&self, query: &str, limit: usize) -> Vec<Value> {
        let terms: Vec<String> = PLAIN_TERM.find_iter(query).map(|m| format!("\"{}\"", m.as_str().replace('"', "\"\""))).collect();
        if terms.is_empty() {
            return Vec::new();
        }
        let Some(con) = self.lib().and_then(|l| db::open_ro(&l.join("search.sqlite"))) else { return Vec::new() };
        let sql = "SELECT doc_id,title,collection,category,path,page,snippet(pages,6,'[',']',' … ',42) AS excerpt,bm25(pages,0,5,2,1,0,0,1) AS score \
                   FROM pages WHERE pages MATCH ?1 ORDER BY score LIMIT ?2";
        let names = ["doc_id", "title", "collection", "category", "path", "page", "excerpt", "score"];
        let run = || -> rusqlite::Result<Vec<Value>> {
            let mut st = con.prepare_cached(sql)?;
            let rows = st.query_map(rusqlite::params![terms.join(" AND "), limit as i64], |row| {
                let mut o = serde_json::Map::new();
                for (i, n) in names.iter().enumerate() {
                    o.insert((*n).into(), sql_json(row.get::<_, Sql>(i)?));
                }
                Ok(Value::Object(o))
            })?;
            rows.collect()
        };
        run().unwrap_or_default()
    }
}
