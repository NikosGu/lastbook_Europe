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
        objs.push(stream(&format!("BT /F1 18 Tf 60 700 Td ({text}) Tj ET")));
    }
    objs[1] = format!("<</Type/Pages/Count {}/Kids[{}]>>", pages.len(), kids.join(" "));
    pdf_of(&objs)
}

/// A tagged PDF, as accessible PDFs are made: page 1 a heading and an infographic whose alt text (written in UTF-16, as
/// a PDF writes words with accents) says what it shows; page 2 a page of text with a tagged photo; page 3 a picture
/// with no tags.
fn tagged_pdf(alt: &str, lines: &[&str]) -> Vec<u8> {
    let utf16 = |s: &str| format!("<FEFF{}>", s.encode_utf16().map(|u| format!("{u:04X}")).collect::<String>());
    let picture = "q 200 0 0 200 100 300 cm /Im1 Do Q";
    let page = |contents: usize, tags: &str| {
        format!("<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Resources<</Font<</F1 3 0 R>>/XObject<</Im1 10 0 R>>>>/Contents {contents} 0 R{tags}>>")
    };
    let text: String = lines.iter().map(|l| format!("({l}) Tj T* ")).collect();
    pdf_of(&[
        "<</Type/Catalog/Pages 2 0 R/MarkInfo<</Marked true>>/StructTreeRoot 11 0 R>>".into(),
        "<</Type/Pages/Count 3/Kids[4 0 R 6 0 R 8 0 R]>>".into(),
        "<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>".into(),
        page(5, "/StructParents 0"),
        stream(&format!("BT /F1 18 Tf 60 700 Td (Agua) Tj ET /Figure <</MCID 0>> BDC {picture} EMC")),
        page(7, "/StructParents 1"),
        stream(&format!("BT /F1 11 Tf 60 740 Td 14 TL {text}ET /Figure <</MCID 0>> BDC {picture} EMC")),
        page(9, ""),
        stream(picture),
        "<</Type/XObject/Subtype/Image/Width 2/Height 2/ColorSpace/DeviceGray/BitsPerComponent 8/Length 4>>\nstream\n@~~@\nendstream".into(),
        "<</Type/StructTreeRoot/K 12 0 R/ParentTree 15 0 R/ParentTreeNextKey 2>>".into(),
        "<</Type/StructElem/S/Document/P 11 0 R/K[13 0 R 14 0 R]>>".into(),
        format!("<</Type/StructElem/S/Figure/P 12 0 R/Pg 4 0 R/K 0/Alt{}>>", utf16(alt)),
        "<</Type/StructElem/S/Figure/P 12 0 R/Pg 6 0 R/K 0/Alt(Una foto del puente sobre el rio crecido)>>".into(),
        "<</Nums[0[13 0 R] 1[14 0 R]]>>".into(),
    ])
}

/// A stream object of the content given.
fn stream(content: &str) -> String {
    format!("<</Length {}>>\nstream\n{content}\nendstream", content.len())
}

/// A PDF of the objects given, numbered from 1 (the first is the catalog), with the cross-reference table PDFium reads.
fn pdf_of(objs: &[String]) -> Vec<u8> {
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
        ("redistribute", json!("maybe"), "yes, no or unclear"),
        ("redistribute", json!("yes"), "\"terms_quote\""),
    ] {
        let e = sources::parse(&with(k, v)).unwrap_err();
        assert!(e.contains(says), "{k}: {e}");
    }
    // a decision about passing a document on carries the address of its terms and the sentence that decides
    let mut decided = ok.clone();
    for (k, v) in [("redistribute", "yes"), ("licence_url", "https://example.org/terms"), ("terms_quote", "Reuse is allowed.")] {
        decided[k] = json!(v);
    }
    let one = sources::parse(&json!({"pack": {"id": "p", "title": "P", "revision": "r1"}, "documents": [decided.clone()]})).unwrap();
    assert_eq!((one.docs[0].redistribute, one.docs[0].terms_quote.as_deref()), (Some("yes"), Some("Reuse is allowed.")));
    assert!(one.redistributable().unwrap_err().contains("\"redistributable\""), "the pack of what may be passed on needs its own summary and revision");
    let same = json!({"pack": {"id": "p", "title": "P", "revision": "r1", "redistributable": {"summary": "S", "revision": "r1"}}, "documents": [decided.clone()]});
    assert!(sources::parse(&same).unwrap_err().contains("must not share"));
    let twice = json!({"pack": {"id": "p", "title": "P"}, "documents": [ok.clone(), ok.clone()]});
    assert!(sources::parse(&twice).unwrap_err().contains("used twice"));
    assert!(sources::parse(&json!({"pack": {"id": "p", "title": "P"}, "documents": []})).is_err());
    assert!(sources::parse(&json!({"pack": {"id": "p q", "title": "P"}, "documents": [ok]})).is_err());
    // the starter list in the repository is well formed: documents in the interface's six languages, under 100 MB
    let starter = sources::load(&repo().join("content/library/starter.json")).unwrap();
    assert!((10..=30).contains(&starter.docs.len()));
    assert!(starter.docs.iter().all(|d| d.sha256.is_some() && d.bytes.is_some() && d.url.as_deref().is_some_and(|u| u.starts_with("https://"))));
    let langs: std::collections::BTreeSet<&str> = starter.docs.iter().map(|d| d.lang.as_str()).collect();
    assert_eq!(langs.into_iter().collect::<Vec<_>>(), ["de", "en", "es", "fr", "it", "pl"]);
    assert!(starter.docs.iter().map(|d| d.bytes.unwrap()).sum::<u64>() <= 100_000_000);
    // each document's terms are settled: whether it may be passed on, where its publisher says so, and in what words
    for d in &starter.docs {
        assert!(d.redistribute.is_some() && d.terms_quote.is_some() && d.licence_url.as_deref().is_some_and(|u| u.starts_with("https://")), "{}", d.id);
    }
    let open = starter.redistributable().unwrap();
    assert!(!open.docs.is_empty() && open.docs.len() < starter.docs.len());
    assert_ne!(open.pack.revision, starter.pack.revision);
    assert_eq!(open.pack.id, starter.pack.id, "the same pack, made of what may be published");
}

#[test]
fn a_pack_of_what_may_be_passed_on_has_nothing_of_the_rest() {
    let dir = temp("open");
    let docs = dir.join("docs");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(docs.join("acqua.html"), PAGE).unwrap();
    std::fs::write(docs.join("chiuso.html"), "<!doctype html><title>Chiuso</title><p>Riserva segreta: parole che non escono mai</p>").unwrap();
    let doc = |id: &str, file: &str, redistribute: &str, quote: &str| {
        json!({"id": id, "file": file, "title": id, "publisher": "Protezione Civile", "lang": "it", "licence": "CC BY 4.0", "licence_url": "https://example.org/note-legali",
               "redistribute": redistribute, "terms_quote": quote, "conditions": "name the source", "source": "https://example.org", "collection": "Italia", "category": "water"})
    };
    let list = sources::parse(&json!({"pack": {"id": "test-pack", "title": "Test pack", "summary": "Everything", "revision": "2026-11-17-test",
                                                "redistributable": {"summary": "Only what may be passed on", "revision": "2026-11-17-test-open"}},
                                       "documents": [doc("acqua-casa", "acqua.html", "yes", "Contents are under CC BY 4.0."),
                                                     doc("riserva", "chiuso.html", "no", "All rights reserved.")]}))
    .unwrap();
    let open = list.redistributable().unwrap();
    assert_eq!(open.docs.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(), ["acqua-casa"]);
    let data = dir.join("LastbookData");
    build(&open, &docs, &data, false, "2026-11-17").unwrap();
    // nothing of the document that may not be passed on: no file, no text, no record, no index row, no word
    let lib = data.join("library-data");
    assert!(lib.join("pages/acqua.html").is_file() && !lib.join("pages/chiuso.html").exists() && !lib.join("text/riserva.txt").exists());
    let manifest = std::fs::read_to_string(data.join("pack-manifest.tsv")).unwrap();
    assert!(!manifest.contains("chiuso") && !manifest.contains("riserva"), "{manifest}");
    for f in ["catalog.json", "search.sqlite", SOURCES_PATH] {
        let bytes = std::fs::read(lib.join(f)).unwrap();
        let s = String::from_utf8_lossy(&bytes);
        assert!(!s.contains("riserva") && !s.contains("Riserva") && !s.contains("chiuso"), "{f}");
    }
    let pack: Value = serde_json::from_slice(&std::fs::read(data.join("pack.json")).unwrap()).unwrap();
    assert_eq!((pack["content_revision"].as_str(), pack["summary"].as_str()), (Some("2026-11-17-test-open"), Some("Only what may be passed on")));
    // its Sources page says on what terms each document may be passed on
    let sources = std::fs::read_to_string(lib.join(SOURCES_PATH)).unwrap();
    assert!(sources.contains("terms: https://example.org/note-legali · may be passed on: yes (name the source)"), "{sources}");
    let l = library(&data);
    assert_eq!(get(&l, "/api/library")["resources"], 2, "the document and the Sources page");
    drop(l);
    cleanup(&dir);
}

/// The release workflow builds the check library in packaging/rust/check-library: its list names the PDF beside it.
#[test]
fn the_release_check_library_is_whole() {
    let folder = repo().join("packaging/rust/check-library");
    let list = sources::load(&folder.join("sources.json")).unwrap();
    assert_eq!(list.docs.len(), 1);
    for d in &list.docs {
        let file = folder.join(&d.file);
        assert_eq!(std::fs::metadata(&file).unwrap().len(), d.bytes.unwrap(), "{}", d.file);
        assert_eq!(super::sha256_file(&file).unwrap(), d.sha256.clone().unwrap(), "{} (a .pdf must stay binary in git)", d.file);
    }
    if !pdfium_here() {
        return;
    }
    let dir = temp("check");
    build(&list, &folder, &dir.join("LastbookData"), false, "2026-11-17").unwrap();
    let text = std::fs::read_to_string(dir.join("LastbookData/library-data/text/pdf-page-check.txt")).unwrap();
    assert!(text.contains("A PDF page, drawn by PDFium") && text.contains("found the PDFium library installed beside it"), "{text}");
    cleanup(&dir);
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
    // the engine's open files closed, its shared database connections too: Windows cannot remove a folder that holds open
    // files, and --replace below does
    drop(l);
    lastbook_engine::db::close_all();
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
    let replaced = build(&list, &dir.join("docs"), &data, true, "2026-11-18");
    assert!(replaced.is_ok(), "{replaced:?}");
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

#[test]
fn page_text_is_cleaned_as_the_library_keeps_it() {
    use super::extract::clean;
    assert_eq!(clean("l’approvvi\u{2}\r\ngionamento\r\nè garantito"), "l’approvvigionamento\nè garantito");
    assert_eq!(clean("Wasser\u{2}vorrat\u{c}x\0y\u{1b}z  \n\n"), "Wasservorrat\nx y z");
    assert_eq!(clean("a\tb\rc"), "a\tb\nc");
}

#[test]
fn a_page_with_few_words_has_what_its_tagged_pdf_says_its_pictures_show() {
    use super::extract::{letters, pdf_pages, Page, FEW_LETTERS};
    if !pdfium_here() {
        eprintln!("no PDFium here (set LASTBOOK_PDFIUM to its folder): alt text is not checked");
        return;
    }
    let dir = temp("alt");
    let docs = dir.join("docs");
    std::fs::create_dir_all(&docs).unwrap();
    let alt = "Infografía: llena garrafas con cuatro litros de agua por persona y día";
    let lines = ["Antes de una inundacion, prepara una mochila con agua, comida y linterna.",
                 "Sube a los pisos de arriba y no cruces nunca una calle inundada a pie o en coche.",
                 "Desconecta la electricidad y el gas, y sigue los avisos de las autoridades.",
                 "Ten a mano una radio con pilas para escuchar las noticias de la emergencia."];
    std::fs::write(docs.join("inundaciones.pdf"), tagged_pdf(alt, &lines)).unwrap();
    let pages = pdf_pages(&docs.join("inundaciones.pdf")).unwrap();
    assert_eq!(pages.len(), 3);
    assert_eq!(pages[0], Page { text: "Agua".into(), alt: vec![alt.into()] }, "a page of few words: its picture's alt text, accents and all");
    assert_eq!(pages[0].kept(), format!("Agua\n[Alt text: {alt}]"));
    assert_eq!(pages[0].words(), format!("Agua\n{alt}"));
    assert!(letters(&pages[1].text) >= FEW_LETTERS && pages[1].text.contains("prepara una mochila"), "{:?}", pages[1]);
    assert!(pages[1].alt.is_empty(), "a page of text keeps its own words, not its photo's description: {:?}", pages[1]);
    assert_eq!(pages[2], Page::default(), "a picture with no tags has no words");
    // built into a library: the page texts keep the mark, the index has the words without it
    let list = sources::parse(&json!({"pack": {"id": "test-pack", "title": "Test pack", "revision": "2026-11-17-test", "map": "base"}, "documents": [
        {"id": "inundaciones-es", "file": "inundaciones.pdf", "title": "Inundaciones", "publisher": "Protección Civil", "year": 2023, "lang": "es",
         "licence": "No licence stated.", "source": "https://example.org/inundaciones", "collection": "España", "category": "preparedness"}]}))
    .unwrap();
    let data = dir.join("LastbookData");
    build(&list, &docs, &data, false, "2026-11-17").unwrap();
    let kept = std::fs::read_to_string(data.join("library-data/text/inundaciones-es.txt")).unwrap();
    assert_eq!(kept.split("\n\u{c}\n").map(str::to_string).collect::<Vec<_>>(), pages.iter().map(Page::kept).collect::<Vec<_>>());
    let l = library(&data);
    let p = get(&l, "/api/page/inundaciones-es/1");
    assert_eq!(p["text"], format!("Agua\n[Alt text: {alt}]"), "the reading page's text view shows it as alt text");
    let hit = get(&l, "/api/find?q=garrafas&lang=es");
    assert_eq!(hit["results"][0]["id"], "inundaciones-es");
    assert_eq!(hit["results"][0]["hits"][0]["page"], 1);
    let snippet = hit["results"][0]["hits"][0]["snippet"].as_str().unwrap();
    assert!(snippet.contains("<mark>garrafas</mark>") && !snippet.contains("Alt text"), "{snippet}");
    for q in ["alt", "text"] {
        let found = get(&l, &format!("/api/find?q={q}&lang=es"));
        assert!(!found["results"].as_array().unwrap().iter().any(|r| r["id"] == "inundaciones-es"), "{q}: {found}");
    }
    drop(l);
    cleanup(&dir);
}
