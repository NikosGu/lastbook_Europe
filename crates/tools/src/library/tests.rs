//! The library tools against small libraries made here: a saved web page and, where PDFium is here, a PDF with real
//! text. What they build is read back through the engine's own library and setup's own installer.

use super::build::{build, SOURCES_PATH};
use super::pack::build_pack;
use super::sources::{self, Sources};
use lastbook_engine::http::Request;
use lastbook_engine::library::Library;
use lastbook_engine::paths::Paths;
use lastbook_engine::setup::download::Control;
use lastbook_engine::setup::install;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn temp(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let d = std::env::temp_dir().join(format!("lb-tools-{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Closes this thread's database connections first: Windows keeps a file that is open.
fn cleanup(dir: &Path) {
    lastbook_engine::db::close_all();
    let _ = std::fs::remove_dir_all(dir);
}

/// A PDF whose pages say the words given, in Helvetica: PDFium reads them back as text.
fn text_pdf(pages: &[&str]) -> Vec<u8> {
    let mut objs = vec!["<</Type/Catalog/Pages 2 0 R>>".to_string(), String::new(), "<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>".to_string()];
    let mut kids = Vec::new();
    for text in pages {
        let n = objs.len() + 1;
        kids.push(format!("{n} 0 R"));
        objs.push(format!("<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Resources<</Font<</F1 3 0 R>>>>/Contents {} 0 R>>", n + 1));
        let stream = format!("BT /F1 18 Tf 60 700 Td ({text}) Tj ET");
        objs.push(format!("<</Length {}>>\nstream\n{stream}\nendstream", stream.len()));
    }
    objs[1] = format!("<</Type/Pages/Count {}/Kids[{}]>>", pages.len(), kids.join(" "));
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

const PAGE: &str = "<!doctype html><html><head><title>Acqua</title><script>var hidden = 'never indexed';</script><style>p{color:red}</style></head>\
<body><nav>Menu Home</nav><h1>Acqua potabile in casa</h1><p>Conservare 9 litri d&rsquo;acqua a persona &amp; cambiarla ogni anno.</p>\
<footer>Footer text</footer></body></html>";

fn pdfium_here() -> bool {
    lastbook_engine::library::pdf::pdfium().is_some()
}

/// A documents folder and its list: a saved page in Italian and, with PDFium, a two-page PDF in German.
fn made_up(dir: &Path, with_pdf: bool) -> Sources {
    let docs = dir.join("docs");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(docs.join("acqua.html"), PAGE).unwrap();
    let mut list = vec![json!({"id": "acqua-casa", "file": "acqua.html", "url": "https://example.org/acqua.html", "title": "Acqua potabile in casa",
                                "publisher": "Protezione Civile", "year": 2025, "lang": "it", "licence": "No licence stated.",
                                "source": "https://example.org/acqua", "collection": "Italia", "category": "water", "notes": "Una nota."})];
    if with_pdf {
        let pdf = text_pdf(&["Trinkwasser fuer den Notfall bevorraten: zwei Liter pro Person und Tag", "Stromausfall: Kerzen, Taschenlampe und Batterien bereithalten"]);
        std::fs::write(docs.join("vorrat.pdf"), &pdf).unwrap();
        list.push(json!({"id": "vorrat-de", "file": "vorrat.pdf", "sha256": super::sha256_file(&docs.join("vorrat.pdf")).unwrap(), "bytes": pdf.len(),
                         "title": "Notvorrat", "publisher": "BBK", "year": "2025", "lang": "de", "licence": "Copyright BBK.",
                         "source": "https://example.org/vorrat", "collection": "Deutschland", "category": "preparedness"}));
    }
    sources::parse(&json!({"pack": {"id": "test-pack", "title": "Test pack", "revision": "2026-11-17-test", "map": "base"}, "documents": list})).unwrap()
}

fn library(data: &Path) -> Library {
    let mut l = Library::new(Paths::new(repo(), Some(data.to_path_buf())));
    l.prewarm = false;
    l
}

fn get(l: &Library, path: &str) -> Value {
    let r = l.route(&Request::get(path)).unwrap();
    assert_eq!(r.status, 200, "{path}: {}", String::from_utf8_lossy(&r.body));
    serde_json::from_slice(&r.body).unwrap()
}

#[test]
fn the_sources_list_is_checked() {
    let ok = json!({"id": "a", "file": "a.pdf", "title": "T", "publisher": "P", "lang": "it", "licence": "L", "source": "S", "collection": "C", "category": "water"});
    let with = |k: &str, v: Value| {
        let mut d = ok.clone();
        d[k] = v;
        json!({"pack": {"id": "p", "title": "P"}, "documents": [d]})
    };
    assert!(sources::parse(&with("id", json!("a"))).is_ok());
    for (k, v, says) in [
        ("id", json!("A b"), "lower-case"),
        ("id", json!("lastbook-sources"), "lower-case"),
        ("file", json!("../a.pdf"), "plain file name"),
        ("file", json!("a.docx"), "only PDFs"),
        ("lang", json!("ita"), "two-letter"),
        ("sha256", json!("abc"), "64 hex"),
        ("licence", json!(""), "\"licence\" is missing"),
    ] {
        let e = sources::parse(&with(k, v)).unwrap_err();
        assert!(e.contains(says), "{k}: {e}");
    }
    let twice = json!({"pack": {"id": "p", "title": "P"}, "documents": [ok.clone(), ok.clone()]});
    assert!(sources::parse(&twice).unwrap_err().contains("used twice"));
    assert!(sources::parse(&json!({"pack": {"id": "p", "title": "P"}, "documents": []})).is_err());
    assert!(sources::parse(&json!({"pack": {"id": "p q", "title": "P"}, "documents": [ok]})).is_err());
    // the starter list in the repository is well formed
    let starter = sources::load(&repo().join("content/library/starter.json")).unwrap();
    assert!((10..=20).contains(&starter.docs.len()));
    assert!(starter.docs.iter().all(|d| d.sha256.is_some() && d.bytes.is_some() && d.url.as_deref().is_some_and(|u| u.starts_with("https://"))));
    let langs: std::collections::BTreeSet<&str> = starter.docs.iter().map(|d| d.lang.as_str()).collect();
    assert_eq!(langs.into_iter().collect::<Vec<_>>(), ["de", "en", "it"]);
    assert!(starter.docs.iter().map(|d| d.bytes.unwrap()).sum::<u64>() <= 100_000_000);
}

#[test]
fn a_library_built_is_what_the_engine_reads() {
    let dir = temp("build");
    let with_pdf = pdfium_here();
    if !with_pdf {
        eprintln!("no PDFium here (set LASTBOOK_PDFIUM to its folder): the PDF part is not checked");
    }
    let list = made_up(&dir, with_pdf);
    let data = dir.join("LastbookData");
    let summary = build(&list, &dir.join("docs"), &data, false, "2026-11-17").unwrap();
    assert_eq!(summary["records"], if with_pdf { 3 } else { 2 });
    assert!(install::is_library(&data));
    // a saved page's text: the words people read, entities decoded, no script, style, menu or footer
    let text = std::fs::read_to_string(data.join("library-data/text/acqua-casa.txt")).unwrap();
    assert!(text.contains("Conservare 9 litri d’acqua a persona & cambiarla ogni anno."), "{text}");
    assert!(!text.contains("never indexed") && !text.contains("color:red") && !text.contains("Menu") && !text.contains("Footer"), "{text}");
    let l = library(&data);
    let found = get(&l, "/api/find?q=litri+acqua&lang=it");
    assert_eq!(found["results"][0]["id"], "acqua-casa");
    let doc = get(&l, "/api/doc/acqua-casa");
    for (k, v) in [("kind", json!("html")), ("publisher", json!("Protezione Civile")), ("year", json!("2025")), ("license", json!("No licence stated.")),
                   ("source_url", json!("https://example.org/acqua")), ("notes", json!("Una nota.")), ("retrieved_at", json!("2026-11-17"))] {
        assert_eq!(doc[k], v, "{k}");
    }
    // the Sources page: a record, served as a page of its own, found by /api/library and by search
    let info = get(&l, "/api/library");
    assert_eq!(info["sources"], "lastbook-sources");
    assert_eq!(info["web_pages"], 2);
    let page = l.route(&Request::get(&format!("/{SOURCES_PATH}"))).unwrap();
    assert_eq!(page.status, 200);
    let html = String::from_utf8(page.body).unwrap();
    assert!(html.contains("<h2>Protezione Civile</h2>") && html.contains("terms: No licence stated.") && html.contains("source: https://example.org/acqua"));
    assert!(!html.contains("<script") && !html.contains("<a "), "words, not links");
    assert!(html.contains("lb-own"), "the app's own look");
    assert!(get(&l, "/api/find?q=permissions")["results"].as_array().unwrap().iter().any(|r| r["id"] == "lastbook-sources"));
    if with_pdf {
        let p = get(&l, "/api/page/vorrat-de/2");
        assert_eq!(p["pages"], 2);
        assert!(p["text"].as_str().unwrap().contains("Stromausfall"), "{p}");
        let hit = get(&l, "/api/find?q=Trinkwasser&lang=de");
        assert_eq!(hit["results"][0]["id"], "vorrat-de");
        assert_eq!(hit["results"][0]["hits"][0]["page"], 1);
        let doc = get(&l, "/api/doc/vorrat-de");
        assert_eq!((doc["pages"].as_u64(), doc["searchability"].as_str()), (Some(2), Some("text")));
        let s = get(&l, "/api/library");
        assert_eq!((s["pdfs"].as_u64(), s["pdf_pages"].as_u64()), (Some(1), Some(2)));
    }
    // pack.json and pack-manifest.tsv: every file, at its size and hash
    let pack: Value = serde_json::from_slice(&std::fs::read(data.join("pack.json")).unwrap()).unwrap();
    assert_eq!((pack["name"].as_str(), pack["content_revision"].as_str(), pack["map"].as_str()), (Some("test-pack"), Some("2026-11-17-test"), Some("base")));
    let tsv = std::fs::read_to_string(data.join("pack-manifest.tsv")).unwrap();
    assert!(tsv.starts_with("path\tbytes\tsha256\n"));
    for line in tsv.lines().skip(1) {
        let parts: Vec<&str> = line.split('\t').collect();
        let p = data.join(parts[0]);
        assert_eq!(std::fs::metadata(&p).unwrap().len().to_string(), parts[1], "{}", parts[0]);
        assert_eq!(super::sha256_file(&p).unwrap(), parts[2], "{}", parts[0]);
    }
    assert!(tsv.contains("library-data/search.sqlite\t") && tsv.contains("pack.json\t") && !tsv.contains("pack-manifest.tsv"));
    // built again with the same date: the same bytes
    let again = dir.join("again/LastbookData");
    build(&list, &dir.join("docs"), &again, false, "2026-11-17").unwrap();
    assert_eq!(std::fs::read(data.join("pack-manifest.tsv")).unwrap(), std::fs::read(again.join("pack-manifest.tsv")).unwrap());
    // and not over a folder that is there, unless asked
    assert!(build(&list, &dir.join("docs"), &data, false, "2026-11-17").unwrap_err().contains("--replace"));
    assert!(build(&list, &dir.join("docs"), &data, true, "2026-11-18").is_ok());
    cleanup(&dir);
}

#[test]
fn documents_must_be_the_ones_the_list_names() {
    let dir = temp("refuse");
    let mut list = made_up(&dir, false);
    list.docs[0].sha256 = Some("0".repeat(64));
    let e = build(&list, &dir.join("docs"), &dir.join("out"), false, "2026-11-17").unwrap_err();
    assert!(e.contains("not the file the list names"), "{e}");
    list.docs[0].sha256 = None;
    list.docs[0].file = "missing.html".into();
    let e = build(&list, &dir.join("docs"), &dir.join("out"), false, "2026-11-17").unwrap_err();
    assert!(e.contains("Download it from https://example.org/acqua.html"), "{e}");
    assert!(!dir.join("out").exists(), "nothing half built is left in its place");
    if !pdfium_here() {
        std::fs::write(dir.join("docs/x.pdf"), text_pdf(&["x"])).unwrap();
        let mut list = made_up(&dir, false);
        list.docs[0].file = "x.pdf".into();
        list.docs[0].kind = "pdf";
        let e = build(&list, &dir.join("docs"), &dir.join("out2"), false, "2026-11-17").unwrap_err();
        assert!(e.contains("LASTBOOK_PDFIUM"), "{e}");
    }
    cleanup(&dir);
}

#[test]
fn a_pack_is_what_setup_installs() {
    let dir = temp("pack");
    let list = made_up(&dir, pdfium_here());
    let data = dir.join("LastbookData");
    build(&list, &dir.join("docs"), &data, false, "2026-11-17").unwrap();
    let (zip, entry) = build_pack(&data, &dir.join("out"), "https://downloads.example.org/v1").unwrap();
    assert_eq!(zip.file_name().unwrap(), "Lastbook-test-pack.zip");
    for (k, v) in [("id", json!("test-pack")), ("title", json!("Test pack")), ("revision", json!("2026-11-17-test")), ("map", json!("base")), ("available", json!(true))] {
        assert_eq!(entry[k], v, "{k}");
    }
    let f = &entry["files"][0];
    assert_eq!(f["name"], "Lastbook-test-pack.zip");
    assert_eq!(f["url"], "https://downloads.example.org/v1/Lastbook-test-pack.zip");
    assert_eq!(f["size"].as_u64(), Some(std::fs::metadata(&zip).unwrap().len()));
    assert_eq!(f["sha256"].as_str(), Some(super::sha256_file(&zip).unwrap().as_str()));
    assert_eq!(entry["download_bytes"], f["size"]);
    // setup unpacks it into a library, the same files as were built
    let into = dir.join("unpack");
    let lib = install::unpack(&zip, &into, entry["installed_bytes"].as_u64(), &Control::new(), &mut |_, _| {}, None, None).unwrap();
    assert!(install::is_library(&lib));
    for line in std::fs::read_to_string(data.join("pack-manifest.tsv")).unwrap().lines().skip(1) {
        let rel = line.split('\t').next().unwrap();
        assert_eq!(std::fs::read(lib.join(rel)).unwrap(), std::fs::read(data.join(rel)).unwrap(), "{rel}");
    }
    assert_eq!(std::fs::read(lib.join("pack-manifest.tsv")).unwrap(), std::fs::read(data.join("pack-manifest.tsv")).unwrap());
    // the same folder gives the same zip
    let (zip2, entry2) = build_pack(&data, &dir.join("out2"), "https://downloads.example.org/v1").unwrap();
    assert_eq!(std::fs::read(&zip).unwrap(), std::fs::read(&zip2).unwrap());
    assert_eq!(entry, entry2);
    // a folder that is not what its manifest says is refused
    std::fs::write(data.join("library-data/extra.txt"), "x").unwrap();
    assert!(build_pack(&data, &dir.join("out3"), "https://downloads.example.org/v1").unwrap_err().contains("not listed"));
    std::fs::remove_file(data.join("library-data/extra.txt")).unwrap();
    std::fs::write(data.join("library-data/summary.json"), "{}").unwrap();
    assert!(build_pack(&data, &dir.join("out3"), "https://downloads.example.org/v1").unwrap_err().contains("has changed"));
    cleanup(&dir);
}
