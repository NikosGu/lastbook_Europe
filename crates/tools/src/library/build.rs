//! build-library: a LastbookData folder from documents and their sources list.
//!
//!   lastbook-tools build-library --sources content/library/starter.json --docs derived/sources/library \
//!       --out derived/packs/starter-li/LastbookData [--replace] [--built 2026-11-17] [--redistributable-only]
//!
//! With --redistributable-only it is the pack that may be published: only the documents whose terms let them be passed
//! on (the list's "redistribute": "yes"); nothing of the others is written, not their files, texts, records or index
//! rows.
//!
//! Everything it writes is what the engine reads:
//!   library-data/catalog.json         one record per document, and the Sources page's
//!   library-data/documents/<file>     the PDFs, as they came (checked against the list's SHA-256 and size)
//!   library-data/pages/<file>         the saved web pages
//!   library-data/text/<id>.txt        each document's text, its pages separated by "\n\f\n"; on a page with few words
//!                                     of its own, what a tagged PDF says its pictures show follows them, each on a line
//!                                     of its own as "[Alt text: …]"
//!   library-data/search.sqlite        the full-text index: FTS5 table `pages`, a row for each page that has words
//!                                     (alt text among them, without its mark)
//!   library-data/summary.json         the counts /api/library gives
//!   library-data/documents/lastbook/sources-and-permissions.html
//!                                     every item with its publisher and terms
//!   pack.json                         the pack's name, title, content revision and map
//!   pack-manifest.tsv                 every file's path, size and SHA-256 (setup's repair checks against it)
//!
//! The folder is built beside the one asked for and put in its place when whole. PDF text needs PDFium: beside this
//! program, or its folder in LASTBOOK_PDFIUM. --built sets the date written into the records and pack.json
//! (default today), so a rebuild of the same documents can come out byte for byte the same.

use super::sources::{self, Doc, Sources, SOURCES_ID};
use super::{extract, sha256_file, today, Args};
use lastbook_engine::dist;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Where every pack keeps its Sources and permissions page (the engine's /api/library finds it by this path).
pub const SOURCES_PATH: &str = "documents/lastbook/sources-and-permissions.html";
/// The FTS5 table the engine searches, as the original Lastbook made it.
pub const INDEX_SCHEMA: &str = "CREATE VIRTUAL TABLE pages USING fts5(doc_id UNINDEXED,title,collection,category,path UNINDEXED,page UNINDEXED,body,tokenize=\"unicode61\")";

const HELP: &str = "lastbook-tools build-library --sources LIST.json --docs FOLDER --out LastbookData [--replace] [--built YYYY-MM-DD]
                             [--redistributable-only]

  --sources   the sources list (content/library/*.json): the pack, and each document's file, address and terms
  --docs      the folder the documents were downloaded into
  --out       the LastbookData folder to make; --replace makes it again if it is there
  --built     the date to write into the records and pack.json (default: today)
  --redistributable-only
              only the documents the list marks \"redistribute\": \"yes\", whose terms let them be passed on: the pack
              that may be published, with the summary and revision of the list's pack.redistributable
PDF text needs PDFium: beside this program, or its folder in LASTBOOK_PDFIUM.";

pub fn main(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{HELP}");
        return 0;
    }
    let run = || -> Result<Value, String> {
        let a = Args::parse(args, &["replace", "redistributable-only"])?;
        let list = sources::load(Path::new(a.need("sources")?))?;
        let list = if a.flag("redistributable-only") { list.redistributable()? } else { list };
        let built = a.get("built").map(str::to_string).unwrap_or_else(today);
        build(&list, Path::new(a.need("docs")?), Path::new(a.need("out")?), a.flag("replace"), &built)
    };
    match run() {
        Ok(summary) => {
            println!("{}", serde_json::to_string_pretty(&summary).unwrap_or_default());
            0
        }
        Err(e) => {
            eprintln!("build-library: {e}\n\n{HELP}");
            1
        }
    }
}

fn io(path: &Path) -> impl Fn(std::io::Error) -> String + '_ {
    move |e| format!("{}: {e}", path.display())
}

fn write(path: &Path, data: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io(dir))?;
    }
    std::fs::write(path, data).map_err(io(path))
}

/// Text for HTML, quotes included.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#x27;")
}

/// The library's place for a document: PDFs under documents/, saved pages under pages/.
fn rel_path(d: &Doc) -> String {
    format!("{}/{}", if d.kind == "pdf" { "documents" } else { "pages" }, d.file)
}

/// The document as downloaded, checked against what the list says was taken.
fn source_file(d: &Doc, docs: &Path) -> Result<PathBuf, String> {
    let src = docs.join(&d.file);
    if !src.is_file() {
        let from = d.url.as_deref().map(|u| format!(" Download it from {u} into {}.", docs.display())).unwrap_or_default();
        return Err(format!("{}: not found.{from}", src.display()));
    }
    let size = std::fs::metadata(&src).map_err(io(&src))?.len();
    let sha = sha256_file(&src).map_err(io(&src))?;
    if d.bytes.is_some_and(|b| b != size) || d.sha256.as_ref().is_some_and(|s| *s != sha) {
        return Err(format!(
            "{}: not the file the list names ({size} bytes, SHA-256 {sha}). The publisher may have changed it: look at it, then put its bytes and sha256 in the list.",
            src.display()
        ));
    }
    Ok(src)
}

/// The Sources and permissions page, and its words for the index: each publisher, what of theirs is here, on what
/// terms, and where it came from. Addresses are words, not links: in the app a link to the web can only do nothing.
fn sources_page(docs: &[Doc]) -> (String, String) {
    let removal = match dist::CONTACT {
        Some(c) => format!("To ask for anything to be removed, write to {c}."),
        None => "To ask for anything to be removed, open an issue where this project is published.".to_string(),
    };
    let mut by_publisher: BTreeMap<String, Vec<&Doc>> = BTreeMap::new();
    for d in docs {
        by_publisher.entry(d.publisher.to_lowercase()).or_default().push(d);
    }
    let intro = "Everything in this library was made by the organizations listed below, and stays theirs. Visit them: they keep their guidance up to date.";
    let mut page = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Sources and permissions</title>\
         <style>body{{font:17px/1.6 system-ui,sans-serif;max-width:900px;margin:32px auto;padding:0 20px}}h2{{margin-top:1.6em;font-size:1.15em}}li{{margin:.25em 0}}</style>\
         </head><body><h1>Sources and permissions</h1><p>{} {}</p><p>Not clinically reviewed. Check medical guidance with a clinician when you can.</p>",
        esc(intro),
        esc(&removal)
    );
    let mut text = vec!["Sources and permissions".to_string(), format!("{intro} {removal}")];
    for list in by_publisher.values() {
        let name = &list[0].publisher;
        let mut list = list.clone();
        list.sort_by_key(|d| d.title.to_lowercase());
        let terms: BTreeSet<&str> = list.iter().map(|d| d.licence.as_str()).collect();
        let terms = terms.into_iter().collect::<Vec<_>>().join("; ");
        let what = if list.len() == 1 { "1 document".to_string() } else { format!("{} documents", list.len()) };
        page.push_str(&format!("<h2>{}</h2><p>{what} · terms: {}</p><details open><summary>What is included</summary><ul>", esc(name), esc(&terms)));
        text.push(format!("{name}: {what}; terms: {terms}"));
        for d in list {
            let year = d.year.as_deref().map(|y| format!(" ({y})")).unwrap_or_default();
            // where the terms are stated, and whether they let the document be passed on (on what conditions)
            let mut terms = Vec::new();
            if let Some(u) = &d.licence_url {
                terms.push(format!("terms: {u}"));
            }
            if let Some(r) = d.redistribute {
                terms.push(format!("may be passed on: {r}{}", d.conditions.as_deref().map(|c| format!(" ({c})")).unwrap_or_default()));
            }
            let terms = terms.join(" · ");
            let small = if terms.is_empty() { String::new() } else { format!("<br><small>{}</small>", esc(&terms)) };
            page.push_str(&format!("<li>{}{year} · {} · source: {}{small}</li>", esc(&d.title), esc(&d.lang), esc(&d.source)));
            text.push(format!("{}{year}, source {}{}", d.title, d.source, if terms.is_empty() { String::new() } else { format!("; {terms}") }));
        }
        page.push_str("</ul></details>");
    }
    page.push_str("</body></html>");
    (page, text.join("\n"))
}

/// The search index, as the engine reads it: one row for each page with words, optimized and checked.
fn write_index(path: &Path, rows: &[(String, String, String, String, String, i64, String)]) -> Result<(), String> {
    let e = |e: rusqlite::Error| format!("{}: {e}", path.display());
    let db = rusqlite::Connection::open(path).map_err(e)?;
    db.execute_batch(INDEX_SCHEMA).map_err(e)?;
    {
        let tx = db.unchecked_transaction().map_err(e)?;
        {
            let mut st = tx.prepare("INSERT INTO pages VALUES (?1,?2,?3,?4,?5,?6,?7)").map_err(e)?;
            for r in rows {
                st.execute(rusqlite::params![r.0, r.1, r.2, r.3, r.4, r.5, r.6]).map_err(e)?;
            }
        }
        tx.commit().map_err(e)?;
    }
    db.execute_batch("INSERT INTO pages(pages) VALUES('optimize'); VACUUM; PRAGMA journal_mode=DELETE;").map_err(e)?;
    let ok: String = db.query_row("PRAGMA integrity_check", [], |r| r.get(0)).map_err(e)?;
    if ok != "ok" {
        return Err(format!("{}: the new index fails its integrity check ({ok})", path.display()));
    }
    Ok(())
}

/// Every file under the folder but pack-manifest.tsv itself: path (with '/'), size, SHA-256, sorted by path.
pub fn manifest_rows(root: &Path) -> Result<Vec<(String, u64, String)>, String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).map_err(io(&dir))? {
            let p = e.map_err(io(&dir))?.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let rel = p.strip_prefix(root).unwrap_or(&p).components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
            if rel == "pack-manifest.tsv" {
                continue;
            }
            let size = std::fs::metadata(&p).map_err(io(&p))?.len();
            out.push((rel, size, sha256_file(&p).map_err(io(&p))?));
        }
    }
    out.sort();
    Ok(out)
}

/// Builds the folder; gives what was built, as a summary.
pub fn build(list: &Sources, docs: &Path, out: &Path, replace: bool, built: &str) -> Result<Value, String> {
    if out.exists() && !replace {
        return Err(format!("{} is already there: --replace builds it again", out.display()));
    }
    let name = out.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or("--out names no folder")?;
    let work = out.with_file_name(format!("{name}.building"));
    if work.exists() {
        std::fs::remove_dir_all(&work).map_err(io(&work))?;
    }
    let lib = work.join("library-data");
    let mut records: Vec<Value> = Vec::new();
    let mut rows = Vec::new();
    let (mut pdf_pages, mut document_bytes) = (0u64, 0u64);
    for d in &list.docs {
        let src = source_file(d, docs)?;
        let rel = rel_path(d);
        let dest = lib.join(&rel);
        std::fs::create_dir_all(dest.parent().unwrap()).map_err(io(&dest))?;
        std::fs::copy(&src, &dest).map_err(io(&dest))?; // a copy (a clone where the disk can), never a hard link: the engine refuses those
        let pages = match d.kind {
            "pdf" => extract::pdf_pages(&dest)?,
            _ => vec![extract::Page::of(extract::html_text(&std::fs::read(&dest).map_err(io(&dest))?))],
        };
        let text_rel = format!("text/{}.txt", d.id);
        write(&lib.join(&text_rel), pages.iter().map(extract::Page::kept).collect::<Vec<_>>().join("\n\u{c}\n").as_bytes())?;
        let words: Vec<String> = pages.iter().map(extract::Page::words).collect();
        for (i, body) in words.iter().enumerate() {
            if !body.trim().is_empty() {
                rows.push((d.id.clone(), d.title.clone(), d.collection.clone(), d.category.clone(), rel.clone(), i as i64 + 1, body.clone()));
            }
        }
        let bytes = std::fs::metadata(&dest).map_err(io(&dest))?.len();
        document_bytes += bytes;
        let mut r = Map::new();
        for (k, v) in [("id", &d.id), ("kind", &d.kind.to_string()), ("path", &rel), ("title", &d.title), ("publisher", &d.publisher), ("collection", &d.collection),
                       ("category", &d.category), ("lang", &d.lang), ("license", &d.licence), ("source_url", &d.source), ("text_path", &text_rel), ("retrieved_at", &built.to_string())] {
            r.insert(k.into(), json!(v));
        }
        for (k, v) in [("publication_date", &d.year), ("license_url", &d.licence_url), ("url", &d.url), ("notes", &d.notes)] {
            if let Some(v) = v {
                r.insert(k.into(), json!(v));
            }
        }
        if d.kind == "pdf" {
            pdf_pages += pages.len() as u64;
            r.insert("pages".into(), json!(pages.len()));
            let words: usize = words.iter().map(|p| p.trim().len()).sum();
            r.insert("searchability".into(), json!(if words > 100 { "text" } else { "image-only: needs OCR" }));
        }
        r.insert("bytes".into(), json!(bytes));
        r.insert("sha256".into(), json!(sha256_file(&dest).map_err(io(&dest))?));
        records.push(Value::Object(r));
    }
    // the Sources and permissions page: an ordinary record, in the index like any other
    let (page, text) = sources_page(&list.docs);
    write(&lib.join(SOURCES_PATH), page.as_bytes())?;
    let text_rel = format!("text/{SOURCES_ID}.txt");
    write(&lib.join(&text_rel), text.as_bytes())?;
    rows.push((SOURCES_ID.into(), "Sources and permissions".into(), "About this library".into(), "about".into(), SOURCES_PATH.into(), 1, text));
    records.push(json!({"id": SOURCES_ID, "kind": "html", "path": SOURCES_PATH, "title": "Sources and permissions", "publisher": dist::NAME,
                        "collection": "About this library", "category": "about", "lang": "en", "source_url": "", "text_path": text_rel,
                        "notes": "Generated from this pack.", "retrieved_at": built, "bytes": page.len(), "sha256": sha256_file(&lib.join(SOURCES_PATH)).map_err(io(&lib))?}));
    let key = |r: &Value| [r["category"].as_str().unwrap_or("").to_string(), r["collection"].as_str().unwrap_or("").to_string(), r["title"].as_str().unwrap_or("").to_string()];
    records.sort_by_key(key);
    write(&lib.join("catalog.json"), serde_json::to_string_pretty(&records).unwrap_or_default().as_bytes())?;
    write_index(&lib.join("search.sqlite"), &rows)?;
    let count = |k: &str| records.iter().filter(|r| r["kind"] == k).count();
    let summary = json!({"resources": records.len(), "pdfs": count("pdf"), "pdf_pages": pdf_pages, "web_pages": count("html"), "videos": 0, "collections": 0,
                         "document_bytes": document_bytes + page.len() as u64});
    write(&lib.join("summary.json"), serde_json::to_string_pretty(&summary).unwrap_or_default().as_bytes())?;
    let kinds: BTreeMap<String, usize> = records.iter().fold(BTreeMap::new(), |mut m, r| {
        *m.entry(r["kind"].as_str().unwrap_or("").to_string()).or_default() += 1;
        m
    });
    let mut pack = json!({"name": list.pack.id, "title": list.pack.title, "summary": list.pack.summary, "map": list.pack.map, "built": built, "counts": kinds, "incomplete": 0});
    if !list.pack.revision.is_empty() {
        pack["content_revision"] = json!(list.pack.revision);
    }
    write(&work.join("pack.json"), serde_json::to_string_pretty(&pack).unwrap_or_default().as_bytes())?;
    let rows_m = manifest_rows(&work)?;
    let mut tsv = String::from("path\tbytes\tsha256\n");
    for (rel, size, sha) in &rows_m {
        tsv.push_str(&format!("{rel}\t{size}\t{sha}\n"));
    }
    write(&work.join("pack-manifest.tsv"), tsv.as_bytes())?;
    if out.exists() {
        std::fs::remove_dir_all(out).map_err(io(out))?;
    }
    std::fs::rename(&work, out).map_err(io(out))?;
    let installed: u64 = rows_m.iter().map(|r| r.1).sum::<u64>() + tsv.len() as u64;
    Ok(json!({"folder": out.to_string_lossy(), "pack": list.pack.id, "records": records.len(), "pdf_pages": pdf_pages,
              "index_rows": rows.len(), "files": rows_m.len() + 1, "bytes": installed}))
}
