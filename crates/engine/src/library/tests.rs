//! The library against small made-up libraries, built here: the shape of the original Lastbook's test library
//! (smaller files; the big ones are sparse), with its canary that must never be served. Ported from
//! app/tests/test_bridge.py (the library's part), test_what_ships.py (the library's part) and test_libq.py.

use super::*;
use crate::http::Request;
use crate::router::Engine;
use serde_json::json;
use std::path::Path;
use std::sync::OnceLock;

pub const CANARY: &[u8] = b"CANARY-MUST-NEVER-BE-SERVED";
pub const MIB: usize = 1024 * 1024;
pub const A_SIZE: usize = 10 * MIB;

pub fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Bytes that say where they are, so a wrong slice cannot pass (fixture.py pattern).
pub fn pattern(offset: usize, n: usize) -> Vec<u8> {
    let first = offset / 4;
    let words: Vec<u8> = (first..first + n / 4 + 2).flat_map(|i| (i as u32).to_be_bytes()).collect();
    words[offset - first * 4..offset - first * 4 + n].to_vec()
}

fn write_pattern(path: &Path, size: usize) {
    let mut out = Vec::with_capacity(size);
    let mut off = 0;
    while off < size {
        let n = MIB.min(size - off);
        out.extend(pattern(off, n));
        off += n;
    }
    std::fs::write(path, out).unwrap();
}

fn sparse(path: &Path, size: u64) {
    std::fs::File::create(path).unwrap().set_len(size).unwrap();
}

/// Three real pages and a script that would run on opening, were anything to open it with a script engine
/// (test_bridge.py fixture_pdf).
pub fn fixture_pdf() -> Vec<u8> {
    let page = "<</Type/Page/Parent 2 0 R/MediaBox[0 0 300 400]>>";
    let objs = ["<</Type/Catalog/Pages 2 0 R/OpenAction<</S/JavaScript/JS(app.alert(\"ran\"))>>>>", "<</Type/Pages/Count 3/Kids[3 0 R 4 0 R 5 0 R]>>", page, page, page];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut at = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        at.push(out.len());
        out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).bytes());
    for a in at {
        out.extend(format!("{a:010} 00000 n \n").bytes());
    }
    out.extend(format!("trailer\n<</Size {}/Root 1 0 R>>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).bytes());
    out
}

pub struct Fixture {
    pub data: PathBuf,
    pub lib: PathBuf,
    pub linked: bool,
}

/// A fresh folder for one test run; ones left by runs more than an hour old go.
pub fn temp(name: &str) -> PathBuf {
    let base = std::env::temp_dir();
    if let Ok(entries) = std::fs::read_dir(&base) {
        for e in entries.flatten() {
            let old = e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).is_some_and(|a| a.as_secs() > 3600);
            if old && e.file_name().to_string_lossy().starts_with("lb-library-") {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
    // the clock alone is not enough: on a Mac it moves by microseconds, and two tests can start within one
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let nanos = SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let d = base.join(format!("lb-library-{name}-{}-{nanos}-{n}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// An FTS5 index shaped like the app's own (engine/library.py build).
pub fn make_index(lib: &Path, rows: &[(&str, &str, &str, &str, &str, i64, &str)]) {
    let con = rusqlite::Connection::open(lib.join("search.sqlite")).unwrap();
    con.execute_batch("CREATE VIRTUAL TABLE pages USING fts5(doc_id UNINDEXED,title,collection,category,path UNINDEXED,page UNINDEXED,body,tokenize=\"unicode61\")").unwrap();
    for r in rows {
        con.execute("INSERT INTO pages VALUES (?,?,?,?,?,?,?)", rusqlite::params![r.0, r.1, r.2, r.3, r.4, r.5, r.6]).unwrap();
    }
}

/// The made-up library the file and header tests share (fixture.py ensure), built once per test run.
pub fn fixture() -> &'static Fixture {
    static F: OnceLock<Fixture> = OnceLock::new();
    F.get_or_init(|| {
        let data = temp("fixture");
        let lib = data.join("library-data");
        for d in ["videos", "documents", "pages", "tools", "logs", "collections", "transcripts", "assets", "text"] {
            std::fs::create_dir_all(lib.join(d)).unwrap();
        }
        std::fs::create_dir_all(data.join("derived/cache")).unwrap();
        write_pattern(&lib.join("videos/a.mp4"), A_SIZE);
        write_pattern(&lib.join("videos/b.webm"), MIB);
        sparse(&lib.join("videos/huge.mkv"), 200 * MIB as u64);
        std::fs::write(lib.join("documents/s.pdf"), pattern(0, 64 * 1024)).unwrap(); // not a PDF inside
        std::fs::write(lib.join("documents/real.pdf"), fixture_pdf()).unwrap();
        write_pattern(&lib.join("documents/mid.bin"), MIB);
        sparse(&lib.join("documents/giant.bin"), 300 * MIB as u64);
        std::fs::write(lib.join("documents/plain.txt"), "plain text file\n").unwrap();
        for p in ["documents/x.info.json", "documents/.hidden", "documents/video-auth-copy.json", "video-auth.json", "canary-root.txt", "tools/secret.txt", "logs/x.log", "documents/hl.txt"] {
            std::fs::write(lib.join(p), CANARY).unwrap();
        }
        std::fs::write(data.join("outside-canary.txt"), CANARY).unwrap();
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(data.join("outside-canary.txt"), lib.join("documents/out.txt")).is_ok();
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(data.join("outside-canary.txt"), lib.join("documents/out.txt")).is_ok();
        std::fs::hard_link(lib.join("documents/hl.txt"), lib.join("pages/hl2.txt")).unwrap();
        std::fs::write(lib.join("pages/s.html"), "<!doctype html><title>Saved</title><p>saved page</p>").unwrap();
        std::fs::write(lib.join("pages/a.svg"), "<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>").unwrap();
        std::fs::write(lib.join("reader.css"), "body{}").unwrap();
        std::fs::write(lib.join("text/realpdf.txt"), "Page one\n\u{c}\nPage two\n\u{c}\nPage three").unwrap();
        let mut recs = Vec::new();
        for (id, kind, path) in [("pdfsmall", "pdf", "documents/s.pdf"), ("saved", "html", "pages/s.html"), ("vida", "video", "videos/a.mp4"), ("txt", "text", "documents/plain.txt"), ("realpdf", "pdf", "documents/real.pdf")] {
            let bytes = std::fs::metadata(lib.join(path)).unwrap().len();
            recs.push(json!({"id": id, "kind": kind, "path": path, "title": id, "category": "test", "collection": "Test", "publisher": "Test", "bytes": bytes}));
        }
        recs[4]["pages"] = json!(3);
        recs[4]["text_path"] = json!("text/realpdf.txt");
        std::fs::write(lib.join("catalog.json"), serde_json::to_vec(&recs).unwrap()).unwrap();
        std::fs::write(lib.join("summary.json"), r#"{"resources": 5}"#).unwrap();
        make_index(&lib, &[("pdfsmall", "pdfsmall", "Test", "test", "documents/s.pdf", 1, "boil water for one minute")]);
        Fixture { data, lib, linked }
    })
}

/// The library of the shared fixture.
pub fn library() -> Library {
    Library::new(Paths::new(repo(), Some(fixture().data.clone())))
}

/// The whole engine on the shared fixture, as the window asks it.
pub fn engine() -> Engine {
    Engine::new(Paths::new(repo(), Some(fixture().data.clone())))
}

pub fn get(l: &Library, path: &str) -> Response {
    l.route(&Request::get(path)).unwrap_or_else(|| panic!("not the library's: {path}"))
}

pub fn body_json(r: &Response) -> Value {
    serde_json::from_slice(&r.body).unwrap_or_else(|_| panic!("not JSON: {}", String::from_utf8_lossy(&r.body)))
}

#[test]
fn ordinary_api() {
    let l = library();
    let r = get(&l, "/api/health");
    assert_eq!(body_json(&r)["offline"], true);
    assert_eq!(r.header("X-Content-Type-Options"), Some("nosniff"));
    assert!(l.route(&Request::get("/api/nope")).is_none(), "the router answers 404");
    assert_eq!(engine().handle(&Request::get("/api/nope")).status, 404);
    let found = body_json(&get(&l, "/api/find?q=boil"));
    assert_eq!(found["results"][0]["id"], "pdfsmall");
    assert_eq!(found["results"][0]["hits"][0], json!({"page": 1, "snippet": "<mark>boil</mark> water for one minute"}));
    assert_eq!(body_json(&get(&l, "/api/doc/pdfsmall"))["id"], "pdfsmall");
    let r = get(&l, "/api/doc/nothing");
    assert_eq!(r.status, 404);
    assert_eq!(body_json(&r)["error"], "This item is not in the library catalog.");
    assert!(l.route(&Request::post("/api/find", "{}")).is_none(), "no POST routes");
    // HEAD: the headers, no body
    let mut h = Request::get("/api/doc/pdfsmall");
    h.method = "HEAD".into();
    let r = l.route(&h).unwrap();
    assert!(r.body.is_empty());
    assert!(r.header("Content-Length").unwrap().parse::<usize>().unwrap() > 10);
    // no library: nothing is the library's
    assert!(Library::new(Paths::new(repo(), None)).route(&Request::get("/api/health")).is_none());
}

#[test]
fn page_text_and_pictures() {
    let mut l = library();
    let t = body_json(&get(&l, "/api/page/realpdf/2"));
    assert_eq!(t, json!({"page": 2, "pages": 3, "text": "Page two"}));
    assert_eq!(body_json(&get(&l, "/api/page/realpdf/9"))["text"], "");
    assert_eq!(get(&l, "/api/page/nothing/1").status, 404);
    assert_eq!(get(&l, "/api/page/saved/1.png").status, 400);
    // without PDFium: a plain refusal, and the text still comes
    l.draw_pdf = false;
    let r = get(&l, "/api/page/realpdf/1.png?w=200");
    assert_eq!(r.status, 503);
    assert!(body_json(&r)["error"].as_str().unwrap().contains("PDFium"));
    assert_eq!(get(&l, "/api/page/realpdf/1").status, 200);
    l.draw_pdf = true;
    if !l.can_draw_pdf() {
        eprintln!("no PDFium here (set LASTBOOK_PDFIUM to its folder): page pictures not checked");
        return;
    }
    let r = get(&l, "/api/page/realpdf/2.png?w=200");
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    assert_eq!(r.header("Content-Type"), Some("image/png"));
    assert_eq!(r.header("Cache-Control"), Some("max-age=86400"));
    assert_eq!(&r.body[..8], b"\x89PNG\r\n\x1a\n");
    let d = png::Decoder::new(std::io::Cursor::new(&r.body)).read_info().unwrap();
    assert_eq!((d.info().width, d.info().height), (150, 200), "the longer side is the width asked");
    assert!(fixture().data.join("derived/cache/realpdf-2-200.png").is_file());
    let small = png::Decoder::new(std::io::Cursor::new(get(&l, "/api/page/realpdf/1.png?w=5").body)).read_info().unwrap();
    assert_eq!(small.info().height, 160, "at least 160");
    assert_eq!(get(&l, "/api/page/realpdf/9.png").status, 404);
    assert_eq!(get(&l, "/api/page/pdfsmall/1.png").status, 500, "not a PDF inside");
}

#[test]
fn guides_show_nothing_that_opens_nothing() {
    // test_what_ships.py Guides: the made-up library holds none of the guides' documents and no encyclopedia
    let l = library();
    assert!(l.zim_names().is_empty());
    let topics = l.topics_resolved();
    assert!(topics.len() >= 10);
    for t in &topics {
        let mut items: Vec<&Value> = Vec::new();
        for k in ["prepare", "learn"] {
            items.extend(t[k].as_array().unwrap());
        }
        for k in ["choices", "route"] {
            for c in t[k].as_array().unwrap() {
                items.extend(c["read"].as_array().unwrap());
            }
        }
        assert!(!items.iter().any(|i| i.get("zim").is_some() || (i.get("doc").is_some() && i["src"].is_null())), "{}", t["id"]);
        for lim in t["limits"].as_array().unwrap() {
            assert!(lim["text"].as_str().is_some_and(|s| !s.is_empty()), "{}", t["id"]);
            assert!(lim.get("zim").is_none() && (lim.get("doc").is_none() || !lim["src"].is_null()));
        }
    }
    let cat = l.catalog();
    assert!(topics::item_here(&json!({"doc": "saved"}), &[], &cat));
    assert!(topics::item_here(&json!({"zim": "zim/m.zim/A"}), &["m.zim".into()], &cat));
    assert!(!topics::item_here(&json!({"zim": "zim/w.zim/A"}), &["m.zim".into()], &cat));
    assert!(!topics::item_here(&json!({"doc": "not-here"}), &[], &cat));
    assert!(topics::item_here(&json!({"text": "only words"}), &[], &cat));
    let r = topics::resolve_item(&json!({"doc": "saved", "page": 2}), &cat, &fixture().lib);
    assert_eq!(r["src"]["kindLabel"], "Saved web page");
    assert_eq!(r["page"], 2);
}

#[test]
fn library_info_and_pins() {
    let l = library();
    let i = body_json(&get(&l, "/api/library"));
    assert_eq!(i["resources"], 5);
    assert_eq!(i["pdfs"], Value::Null);
    assert_eq!(i["zims"], json!([]));
    assert_eq!(i["pack"], false);
    assert_eq!(i["pins"], true, "not a pack: everything stays offered");
    // a pack: pins only when the map has its files
    let data = temp("pack");
    std::fs::create_dir_all(data.join("library-data")).unwrap();
    std::fs::write(data.join("library-data/catalog.json"), "[]").unwrap();
    std::fs::write(data.join("pack.json"), r#"{"name": "small"}"#).unwrap();
    let p = Library::new(Paths::new(repo(), Some(data.clone())));
    assert_eq!(p.library_info()["pins"], false);
    std::fs::create_dir_all(data.join("derived/geo")).unwrap();
    std::fs::write(data.join("derived/geo/map.sqlite"), "x").unwrap();
    assert_eq!(p.library_info()["pins"], true);
    assert_eq!(p.library_info()["pack"], true);
    std::fs::remove_dir_all(&data).unwrap();
}

/// A library for search: records in several languages and kinds, a video with its transcript, and an index.
fn search_library() -> (PathBuf, Library) {
    let data = temp("search");
    let lib = data.join("library-data");
    std::fs::create_dir_all(lib.join("transcripts")).unwrap();
    let cat = json!([
        {"id": "who", "kind": "pdf", "path": "documents/who.pdf", "title": "Water treatment in emergencies", "publisher": "World Health Organization", "collection": "WHO", "category": "water", "pages": 12},
        {"id": "ref", "kind": "pdf", "path": "documents/ref.pdf", "title": "Notes on wells", "publisher": "Someone", "collection": "Misc", "category": "water", "pages": 40},
        {"id": "it", "kind": "pdf", "path": "documents/it.pdf", "title": "Acqua potabile", "publisher": "Someone", "collection": "Misc", "category": "water", "pages": 4, "lang": "it"},
        {"id": "surg", "kind": "pdf", "path": "documents/surg.pdf", "title": "War surgery", "publisher": "ICRC", "collection": "ICRC", "category": "medical", "pages": 300},
        {"id": "page", "kind": "html", "path": "pages/p.html", "title": "Boiling water at altitude", "publisher": "Someone", "collection": "Misc", "category": "water"},
        {"id": "vid", "kind": "video", "path": "videos/v.mp4", "title": "How to boil water", "publisher": "Someone", "collection": "Videos", "category": "water-treatment", "duration": 300},
        {"id": "tx", "kind": "transcript", "path": "transcripts/v.txt", "video_path": "videos/v.mp4", "title": "How to boil water", "collection": "Videos"},
        {"id": "metadata-vid", "kind": "metadata", "path": "metadata/videos/v.json", "video_path": "videos/v.mp4", "title": "x", "collection": "Videos"}
    ]);
    std::fs::write(lib.join("catalog.json"), serde_json::to_vec(&cat).unwrap()).unwrap();
    std::fs::write(lib.join("transcripts/v.txt"), "Bring the water to a boil.\n\nLet it cool.").unwrap();
    make_index(&lib, &[
        ("who", "Water treatment in emergencies", "WHO", "water", "documents/who.pdf", 3, "Boil water for one minute to make it safe."),
        ("who", "Water treatment in emergencies", "WHO", "water", "documents/who.pdf", 4, "Water that cannot be boiled may be treated with chlorine."),
        ("ref", "Notes on wells", "Misc", "water", "documents/ref.pdf", 7, "Boil water drawn from a new well."),
        ("it", "Acqua potabile", "Misc", "water", "documents/it.pdf", 1, "Boil water: far bollire l'acqua per un minuto."),
        ("surg", "War surgery", "ICRC", "medical", "documents/surg.pdf", 99, "Boil water for instruments before surgery."),
        ("page", "Boiling water at altitude", "Misc", "water", "pages/p.html", 1, "At altitude, boil water <longer> & let it cool."),
        ("tx", "How to boil water", "Videos", "water", "transcripts/v.txt", 1, "Bring the water to a boil and let it cool."),
        ("metadata-vid", "x", "Videos", "water", "metadata/videos/v.json", 1, "boil water"),
    ]);
    let l = Library::new(Paths::new(repo(), Some(data.clone())));
    (data, l)
}

#[test]
fn find_ranks_groups_and_prefers_the_language() {
    let (data, l) = search_library();
    let ids = |v: &Value| v["results"].as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    let r = l.find("boil water", "all", "en");
    assert_eq!(r["mode"], "some words", "few rows: any of the words too");
    let got = ids(&r);
    let at = |id: &str| got.iter().position(|i| i == id).unwrap();
    assert!(at("who") < at("ref"), "most trusted first: {got:?}");
    assert!(got.contains(&"vid".to_string()), "a transcript's words find its video");
    assert!(!got.iter().any(|i| i == "tx" || i == "metadata-vid"));
    assert!(at("surg") > at("ref"), "clinical texts after general guidance: {got:?}");
    let who = r["results"].as_array().unwrap().iter().find(|x| x["id"] == "who").unwrap();
    assert_eq!(who["hits"].as_array().unwrap().len(), 2, "two pages of one document");
    assert_eq!(who["hits"][0]["page"], 3);
    let page = r["results"].as_array().unwrap().iter().find(|x| x["id"] == "page").unwrap();
    assert_eq!(page["hits"][0]["page"], Value::Null, "pages are PDF pages only");
    assert!(page["hits"][0]["snippet"].as_str().unwrap().contains("&lt;longer&gt; &amp; let"));
    // the interface's language comes first; in English it comes after
    let pos = |v: &Value, id: &str| ids(v).iter().position(|i| i == id).unwrap();
    let en = l.find("boil water", "all", "en");
    let it = l.find("boil water", "all", "it");
    assert!(pos(&it, "it") < pos(&en, "it"), "{:?} / {:?}", ids(&it), ids(&en));
    // kinds, and the words looked for as well
    assert_eq!(ids(&l.find("boil water", "video", "en")), vec!["vid"]);
    assert!(!ids(&l.find("boil water", "read", "en")).contains(&"vid".to_string()));
    let r = l.find("cut", "all", "en");
    assert_eq!(r["expanded"], json!(["bleeding", "laceration", "wound", "wounds"]));
    assert_eq!(l.find("the", "all", "en")["results"][0]["id"], "vid", "only stop words: they are searched for after all");
    assert_eq!(l.find("zzzz", "all", "en")["results"], json!([]));
    assert_eq!(l.find("", "all", "en"), json!({"q": "", "results": [], "expanded": []}));
    assert_eq!(l.find("\"*(", "all", "en")["results"], json!([]), "what the index cannot take finds nothing");
    std::fs::remove_dir_all(&data).unwrap();
}

#[test]
fn keyword_search_for_the_assistant() {
    let (data, l) = search_library();
    let res = l.search("boil water", 8, "any", "en");
    assert!(!res.is_empty());
    let first = &res[0];
    for k in ["doc_id", "title", "publisher", "year", "kind", "trust", "pages", "page", "t", "duration", "thumb", "excerpt", "found_by", "score", "notes", "imageOnly"] {
        assert!(first.get(k).is_some(), "{k}");
    }
    assert!(res.iter().any(|r| r["found_by"] == json!({"keyword": 1})));
    let docs: Vec<&str> = res.iter().map(|r| r["doc_id"].as_str().unwrap()).collect();
    assert!(docs.iter().filter(|d| **d == "who").count() <= 2, "at most two passages per document");
    assert!(docs.contains(&"vid") && !docs.contains(&"tx"), "a transcript counts for its video");
    let v = res.iter().find(|r| r["doc_id"] == "vid").unwrap();
    assert_eq!(v["page"], Value::Null);
    assert_eq!(v["kind"], "video");
    assert!(l.search("boil water", 8, "manual", "en").iter().all(|r| r["kind"] == "pdf" || r["kind"] == "html"));
    assert!(l.search("boil water", 8, "video", "en").iter().all(|r| r["kind"] == "video"));
    assert!(l.search_in("boil water", 8, "any", "en", Some("ref")).iter().all(|r| r["doc_id"] == "ref"));
    assert_eq!(l.search("boil water", 1, "any", "en").len(), 1);
    // the plain search of engine/library.py
    let plain = l.simple_search("boil water", 40);
    assert!(plain.iter().any(|r| r["doc_id"] == "metadata-vid"), "as the index has it");
    assert!(plain[0]["excerpt"].as_str().unwrap().to_lowercase().contains("[boil]"));
    let r = body_json(&get(&l, "/api/retrieve?q=boil+water&k=3&kind=manual"));
    assert_eq!(r["results"].as_array().unwrap().len(), 3);
    assert_eq!(r["semantic"]["chunks"], 0);
    std::fs::remove_dir_all(&data).unwrap();
}

#[test]
fn shelves_collections_videos_and_transcripts() {
    let (data, l) = search_library();
    let sh = body_json(&get(&l, "/api/shelves"));
    let water = sh.as_array().unwrap().iter().find(|s| s["id"] == "water").unwrap();
    assert_eq!((water["read"].as_u64(), water["video"].as_u64()), (Some(4), Some(1)));
    assert_eq!(sh.as_array().unwrap().last().unwrap()["id"], "other");
    let s = body_json(&get(&l, "/api/shelf/water?kind=read"));
    assert_eq!(s["total"], 4);
    assert_eq!(s["collections"][0]["collection"], "WHO", "most trusted collection first");
    assert_eq!(body_json(&get(&l, "/api/shelf/water?q=acqua"))["total"], 1);
    assert_eq!(body_json(&get(&l, "/api/shelf/water?q=ACQUÀ"))["total"], 1, "typed filters fold");
    let c = body_json(&get(&l, "/api/collection?name=Videos"));
    assert_eq!(c["total"], 2, "everything but the video details");
    let home = body_json(&get(&l, "/api/videos"));
    assert_eq!(home[0]["id"], "guides");
    let row = home.as_array().unwrap().iter().find(|r| r["id"] == "water").unwrap();
    assert_eq!(row["items"][0], json!({"id": "vid", "title": "How to boil water", "publisher": "Someone", "duration": 300, "thumb": null, "transcript": true, "trust": "Reference", "year": null}));
    let list = body_json(&get(&l, "/api/videos/all?len=short&q=BOIL"));
    assert_eq!(list["total"], 1);
    assert_eq!(list["title"], "All videos");
    assert_eq!(body_json(&get(&l, "/api/videos/all?len=long"))["total"], 0);
    assert_eq!(body_json(&get(&l, "/api/videos/related/vid")), json!([]));
    let t = body_json(&get(&l, "/api/transcript/vid"));
    assert_eq!(t["source"], "transcript");
    assert_eq!(t["cues"], json!([{"t": null, "text": "Bring the water to a boil."}, {"t": null, "text": "Let it cool."}]));
    assert_eq!(get(&l, "/api/transcript/nothing").status, 404);
    std::fs::remove_dir_all(&data).unwrap();
}

#[test]
fn libq_against_a_small_library() {
    // test_libq.py: a catalog of two records, the text of a three-page PDF and the app's own index, in a folder whose
    // name has a space and a '#'
    let dir = temp("libq");
    let data = dir.join("Lastbook Data #1");
    let lib = data.join("library-data");
    std::fs::create_dir_all(lib.join("text")).unwrap();
    let pages = ["Safe water\nBring water to a rolling boil.", "Storing water\nUse clean containers: “food grade”.", "When to get help"];
    let cat = json!([{"id": "doc-water", "kind": "pdf", "pages": 3, "title": "Agua segura: “guía”", "collection": "Water", "text_path": "text/doc-water.txt"},
                     {"id": "metadata-video-1", "kind": "metadata", "title": "Water video details", "collection": "Water"}]);
    std::fs::write(lib.join("catalog.json"), serde_json::to_vec(&cat).unwrap()).unwrap();
    std::fs::write(lib.join("text/doc-water.txt"), pages.join("\n\u{c}\n")).unwrap();
    let rows: Vec<(&str, &str, &str, &str, &str, i64, &str)> = pages.iter().enumerate().map(|(i, b)| ("doc-water", "Agua segura", "Water", "water", "documents/w.pdf", i as i64 + 1, *b)).collect();
    make_index(&lib, &rows);
    let l = Library::new(Paths::new(repo(), Some(data.clone())));
    // search finds the page
    let s = l.simple_search("rolling boil", 25);
    assert_eq!((s[0]["doc_id"].as_str(), s[0]["page"].as_i64()), (Some("doc-water"), Some(1)));
    assert_eq!(l.find("rolling boil", "all", "en")["results"][0]["hits"][0]["page"], 1);
    // page and outline
    let r = l.catalog().get("doc-water").unwrap().clone();
    let texts = l.page_texts(&r);
    assert_eq!(texts.len(), 3);
    assert!(texts[1].contains("“food grade”"));
    assert_eq!(texts[2].split_whitespace().collect::<Vec<_>>().join(" "), "When to get help");
    assert_eq!(body_json(&get(&l, "/api/page/doc-water/2"))["pages"], 3);
    // titles leave out the video details; a record's own fields
    let c = body_json(&get(&l, "/api/collection?name=Water"));
    assert_eq!(c["items"].as_array().unwrap().iter().map(|i| i["id"].clone()).collect::<Vec<_>>(), vec![json!("doc-water")]);
    assert_eq!(body_json(&get(&l, "/api/doc/doc-water"))["pages"], 3);
    // a folder with no library answers, without a library
    let empty = Library::new(Paths::new(repo(), Some(dir.join("empty"))));
    assert_eq!(get(&empty, "/api/doc/doc-water").status, 404);
    assert_eq!(body_json(&get(&empty, "/api/find?q=water"))["results"], json!([]));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_catalog_is_read_again_when_it_changes() {
    let data = temp("reread");
    let lib = data.join("library-data");
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::write(lib.join("catalog.json"), r#"[{"id": "a", "kind": "pdf", "path": "a.pdf", "title": "A"}]"#).unwrap();
    let l = Library::new(Paths::new(repo(), Some(data.clone())));
    assert!(l.catalog().get("a").is_some());
    std::fs::write(lib.join("catalog.json"), r#"[{"id": "a", "kind": "video", "path": "a.mp4", "title": "A"}, {"id": "b", "kind": "pdf", "path": "b.pdf", "title": "B"}]"#).unwrap();
    assert!(l.catalog().get("b").is_some(), "a new size is a new catalog");
    std::fs::write(lib.join("video-exclusions.json"), r#"{"videos": {"a": {}}}"#).unwrap();
    assert!(l.catalog().get("a").is_none(), "a video turned down goes at once");
    std::fs::write(lib.join("catalog.json"), "[{\"id\": \"half").unwrap();
    assert!(l.catalog().get("b").is_some(), "half written: the copy already read stays");
    std::fs::remove_dir_all(&data).unwrap();
}
