//! Encyclopedias against small ZIM files written here: one in the newer layout (articles in `C`, `W/mainPage`, a
//! listing of front articles) with zstd, xz and stored clusters, and one in the older layout (articles in `A`). Ported
//! from the ZIM parts of app/tests/test_bridge.py, test_libq.py and test_what_ships.py.

use super::files::{PDF_NOTICE, ZIM_CSP};
use super::tests::*;
use super::zim::Zim;
use super::*;
use crate::http::Request;
use serde_json::json;
use std::path::Path;

/// One entry to write: (namespace, path, title, mime type or "" for a redirect, content or the redirect's target path).
type Entry<'a> = (u8, &'a str, &'a str, &'a str, &'a [u8]);

fn compress(kind: u8, data: &[u8]) -> Vec<u8> {
    match kind {
        4 => {
            let mut out = Vec::new();
            lzma_rs::xz_compress(&mut std::io::BufReader::new(data), &mut out).unwrap();
            out
        }
        5 => ruzstd::encoding::compress_to_vec(data, ruzstd::encoding::CompressionLevel::Fastest),
        _ => data.to_vec(),
    }
}

/// A ZIM file. HTML goes in a cluster compressed with `kind` (1 stored, 4 xz, 5 zstd), the rest in a stored one.
/// Newer layout: the front articles' title listing is added; older: the main page is in the header.
fn write_zim(path: &Path, entries: &[Entry], newer: bool, kind: u8, main: (u8, &str)) {
    let mut all: Vec<(u8, String, String, String, Vec<u8>)> = entries.iter().map(|e| (e.0, e.1.to_string(), e.2.to_string(), e.3.to_string(), e.4.to_vec())).collect();
    if newer {
        all.push((b'X', "listing/titleOrdered/v1".into(), String::new(), "application/octet-stream+zimlisting".into(), Vec::new()));
    }
    all.sort_by(|a, b| (a.0, a.1.as_bytes()).cmp(&(b.0, b.1.as_bytes())));
    let title = |e: &(u8, String, String, String, Vec<u8>)| if e.2.is_empty() { e.1.clone() } else { e.2.clone() };
    if newer {
        let mut front: Vec<u32> = (0..all.len() as u32).filter(|i| all[*i as usize].0 == b'C' && all[*i as usize].3.contains("html")).collect();
        front.sort_by_key(|i| title(&all[*i as usize]));
        let at = all.iter().position(|e| e.0 == b'X').unwrap();
        all[at].4 = front.iter().flat_map(|i| i.to_le_bytes()).collect();
    }
    let index = |ns: u8, p: &str| all.iter().position(|e| e.0 == ns && e.1 == p).unwrap() as u32;
    let mut mimes: Vec<String> = Vec::new();
    for e in &all {
        if !e.3.is_empty() && !mimes.contains(&e.3) {
            mimes.push(e.3.clone());
        }
    }
    // two clusters: HTML (compressed), everything else (stored)
    let mut blobs: [Vec<Vec<u8>>; 2] = [Vec::new(), Vec::new()];
    let mut placed: Vec<Option<(u32, u32)>> = Vec::new();
    for e in &all {
        if e.3.is_empty() {
            placed.push(None);
            continue;
        }
        let c = usize::from(!e.3.contains("html"));
        placed.push(Some((c as u32, blobs[c].len() as u32)));
        blobs[c].push(e.4.clone());
    }
    let clusters: Vec<Vec<u8>> = blobs
        .iter()
        .enumerate()
        .map(|(c, bs)| {
            let mut offsets = Vec::new();
            let mut at = 4 * (bs.len() + 1);
            for b in bs.iter() {
                offsets.extend((at as u32).to_le_bytes());
                at += b.len();
            }
            offsets.extend((at as u32).to_le_bytes());
            let data = [offsets, bs.concat()].concat();
            let k = if c == 0 { kind } else { 1 };
            [vec![k], compress(k, &data)].concat()
        })
        .collect();
    let dirent = |i: usize| -> Vec<u8> {
        let e = &all[i];
        let mut d = Vec::new();
        match placed[i] {
            None => {
                d.extend(0xffffu16.to_le_bytes());
                d.extend([0, e.0]);
                d.extend(0u32.to_le_bytes());
                let to = std::str::from_utf8(&e.4).unwrap();
                let (ns, p) = to.split_once('/').unwrap();
                d.extend(index(ns.as_bytes()[0], p).to_le_bytes());
            }
            Some((c, b)) => {
                d.extend((mimes.iter().position(|m| *m == e.3).unwrap() as u16).to_le_bytes());
                d.extend([0, e.0]);
                d.extend(0u32.to_le_bytes());
                d.extend(c.to_le_bytes());
                d.extend(b.to_le_bytes());
            }
        }
        d.extend(e.1.as_bytes());
        d.push(0);
        d.extend(e.2.as_bytes());
        d.push(0);
        d
    };
    let n = all.len();
    let mime_list: Vec<u8> = mimes.iter().flat_map(|m| [m.as_bytes(), &[0]].concat()).chain([0]).collect();
    let mime_pos = 80u64;
    let path_ptr = mime_pos + mime_list.len() as u64;
    let title_ptr = path_ptr + 8 * n as u64;
    let dirents: Vec<Vec<u8>> = (0..n).map(dirent).collect();
    let mut at = title_ptr + 4 * n as u64;
    let mut path_ptrs = Vec::new();
    for d in &dirents {
        path_ptrs.extend(at.to_le_bytes());
        at += d.len() as u64;
    }
    let cluster_ptr = at;
    at += 8 * clusters.len() as u64;
    let mut cluster_ptrs = Vec::new();
    for c in &clusters {
        cluster_ptrs.extend(at.to_le_bytes());
        at += c.len() as u64;
    }
    let checksum = at;
    let mut by_title: Vec<u32> = (0..n as u32).collect();
    by_title.sort_by(|a, b| (all[*a as usize].0, title(&all[*a as usize])).cmp(&(all[*b as usize].0, title(&all[*b as usize]))));
    let mut f = Vec::new();
    f.extend(72173914u32.to_le_bytes());
    f.extend(if newer { [6u16, 1] } else { [5u16, 0] }.iter().flat_map(|v| v.to_le_bytes()));
    f.extend([7u8; 16]);
    f.extend((n as u32).to_le_bytes());
    f.extend((clusters.len() as u32).to_le_bytes());
    for v in [path_ptr, title_ptr, cluster_ptr, mime_pos] {
        f.extend(v.to_le_bytes());
    }
    f.extend(if newer { 0xffff_ffffu32 } else { index(main.0, main.1) }.to_le_bytes());
    f.extend(0xffff_ffffu32.to_le_bytes());
    f.extend(checksum.to_le_bytes());
    f.extend(mime_list);
    f.extend(path_ptrs);
    f.extend(by_title.iter().flat_map(|i| i.to_le_bytes()));
    f.extend(dirents.concat());
    f.extend(cluster_ptrs);
    f.extend(clusters.concat());
    f.extend([0u8; 16]);
    std::fs::write(path, f).unwrap();
}

const RAIN: &[u8] = b"<html><head><title>Rainwater harvesting</title><script>alert(1)</script></head><body><p onclick=\"x()\">Collect rainwater in a clean barrel. \
                      <a href=\"https://en.wikipedia.org/wiki/Rain\">Rain</a> <a href=\"Seed_saving\">seeds</a></p></body></html>";

fn newer_entries() -> Vec<Entry<'static>> {
    vec![
        (b'C', "Rainwater_harvesting", "Rainwater harvesting", "text/html", RAIN),
        (b'C', "Seed_saving", "Seed saving", "text/html", b"<html><head></head><body><p>Dry seeds fully before storing them.</p></body></html>"),
        (b'C', "Main_Page", "Tiny", "text/html", b"<html><head></head><body><h1>Tiny</h1></body></html>"),
        (b'C', "Rain_barrel", "Rain barrel", "", b"C/Rainwater_harvesting"),
        (b'C', "I/drop.svg", "", "image/svg+xml", b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>1</script><circle onload=\"x\" r=\"1\"/></svg>"),
        (b'C', "I/pic.png", "", "image/png", b"\x89PNG\r\n\x1a\nnot really"),
        (b'C', "Guide.pdf", "Guide", "application/pdf", b"%PDF-1.4 inside an encyclopedia"),
        (b'W', "mainPage", "", "", b"C/Main_Page"),
        (b'M', "Title", "", "text/plain", b"Tiny"),
        (b'M', "Language", "", "text/plain", b"eng"),
    ]
}

/// A library with three encyclopedias in the newer layout (zstd, xz, stored) and one in the older.
fn zim_library() -> (PathBuf, Library) {
    let data = temp("zim");
    let lib = data.join("library-data");
    std::fs::create_dir_all(lib.join("collections")).unwrap();
    std::fs::write(lib.join("catalog.json"), "[]").unwrap();
    for (name, kind) in [("tiny_en_zstd.zim", 5), ("tiny_en_xz.zim", 4), ("tiny_en_plain.zim", 1)] {
        write_zim(&lib.join("collections").join(name), &newer_entries(), true, kind, (b'C', ""));
    }
    let old: Vec<Entry> = vec![
        (b'A', "Water", "Water", "text/html", b"<html><head></head><body><p>Old water page</p></body></html>"),
        (b'A', "Start", "Start", "text/html", b"<html><head></head><body><p>Old start</p></body></html>"),
        (b'-', "style.css", "", "text/css", b"p{}"),
    ];
    write_zim(&lib.join("collections/old_en.zim"), &old, false, 5, (b'A', "Start"));
    std::fs::write(lib.join("collections/.hidden.zim"), b"x").unwrap();
    std::fs::write(lib.join("collections/notes.txt"), CANARY).unwrap();
    std::fs::write(lib.join("collections/broken.zim"), b"not a zim at all").unwrap();
    let l = Library::new(Paths::new(repo(), Some(data.clone())));
    (data, l)
}

fn zget(l: &Library, path: &str) -> Response {
    l.route(&Request::get(path)).unwrap()
}

#[test]
fn the_reader_reads_each_kind_of_cluster() {
    let (data, _) = zim_library();
    for name in ["tiny_en_zstd.zim", "tiny_en_xz.zim", "tiny_en_plain.zim"] {
        let z = Zim::open(&data.join("library-data/collections").join(name)).unwrap();
        let i = z.entry_by_path("Rainwater_harvesting").unwrap();
        assert_eq!(z.content(i).unwrap(), (RAIN.to_vec(), "text/html".to_string()), "{name}");
        assert_eq!(z.entry_by_path("A/Rainwater_harvesting"), Some(i), "an old address finds the article in C");
        let r = z.entry_by_path("Rain_barrel").unwrap();
        assert_eq!(z.content(r).unwrap().0, RAIN, "a redirect is followed");
        assert_eq!(z.main_path().as_deref(), Some("Main_Page"));
        assert!(z.entry_by_path("Nothing").is_none());
        assert_eq!(z.content(z.entry_by_path("I/pic.png").unwrap()).unwrap().1, "image/png");
    }
    let old = Zim::open(&data.join("library-data/collections/old_en.zim")).unwrap();
    assert_eq!(old.main_path().as_deref(), Some("A/Start"), "an older file names its namespace");
    assert!(old.entry_by_path("A/Water").is_some() && old.entry_by_path("Water").is_some() && old.entry_by_path("-/style.css").is_some());
    assert!(Zim::open(&data.join("library-data/collections/broken.zim")).is_err());
    cleanup(&data);
}

#[test]
fn encyclopedia_pages_as_served() {
    let (data, l) = zim_library();
    let r = zget(&l, "/zim/tiny_en_zstd.zim/Rainwater_harvesting");
    assert_eq!((r.status, r.header("Content-Type")), (200, Some("text/html")));
    assert_eq!(r.header("Content-Security-Policy"), Some(ZIM_CSP.as_str()));
    assert_eq!(r.header("X-DNS-Prefetch-Control"), Some("off"));
    let b = String::from_utf8(r.body).unwrap();
    assert!(!b.contains("<script") && !b.contains("onclick") && !b.contains("wikipedia.org"), "{b}");
    assert!(b.contains("Collect rainwater in a clean barrel. Rain <a href=\"Seed_saving\">seeds</a>"), "a web link is plain text, a local one stays");
    assert!(b.contains("<style id=\"lb-article\">") && b.find("lb-article").unwrap() < b.find("</head>").unwrap());
    // the bare address goes to the front page; a webview's scheme follows no redirect, so a page moves itself
    let r = zget(&l, "/zim/tiny_en_xz.zim/");
    assert_eq!(r.status, 200);
    assert!(String::from_utf8_lossy(&r.body).contains("url=/zim/tiny_en_xz.zim/Main_Page"));
    assert!(zget(&l, "/zim/tiny_en_xz.zim").body.windows(9).any(|w| w == b"Main_Page"));
    assert!(String::from_utf8_lossy(&zget(&l, "/zim/old_en.zim/").body).contains("url=/zim/old_en.zim/A/Start"));
    assert_eq!(zget(&l, "/zim/old_en.zim/A/Water").status, 200);
    assert_eq!(zget(&l, "/zim/tiny_en_plain.zim/Rain_barrel").status, 200, "a redirect entry shows its article");
    // a drawing loses its scripts and gets no look; a picture is as it came
    let svg = String::from_utf8(zget(&l, "/zim/tiny_en_zstd.zim/I/drop.svg").body).unwrap();
    assert!(!svg.contains("<script") && !svg.contains("onload") && !svg.contains("lb-article"));
    assert_eq!(zget(&l, "/zim/tiny_en_zstd.zim/I/pic.png").body, b"\x89PNG\r\n\x1a\nnot really");
    // a PDF in an encyclopedia is refused by its type
    let r = zget(&l, "/zim/tiny_en_zstd.zim/Guide.pdf");
    assert_eq!((r.status, r.body.as_slice()), (404, PDF_NOTICE));
    // refusals, each with the encyclopedia's policy; nothing names a file path
    for t in ["/zim/none.zim/A/x", "/zim/none.zim/", "/zim/", "/zim/tiny_en_zstd.zim/Nothing", "/zim/.hidden.zim/", "/zim/notes.txt/", "/zim/broken.zim/", "/zim/..%2fcatalog.json/x"] {
        let r = zget(&l, t);
        assert_eq!(r.status, 404, "{t}");
        assert_eq!(r.header("Content-Security-Policy"), Some(ZIM_CSP.as_str()), "{t}");
        assert!(!String::from_utf8_lossy(&r.body).contains(&*data.to_string_lossy()));
        assert!(!r.body.windows(CANARY.len()).any(|w| w == CANARY));
    }
    cleanup(&data);
}

#[test]
fn encyclopedia_search_by_title() {
    // test_libq.py: 'rainwater barrel' finds the article, as the address a guide's zim field takes, and not the other
    let (data, l) = zim_library();
    let hits = body_json(&zget(&l, "/api/collection-search?q=rainwater+barrel"));
    let hits = hits.as_array().unwrap();
    assert!(hits.iter().any(|h| h == &json!({"title": "Rainwater harvesting", "collection": "tiny_en_zstd", "path": "zim/tiny_en_zstd.zim/Rainwater_harvesting"})), "{hits:?}");
    assert!(!hits.iter().any(|h| h["path"].as_str().unwrap().contains("Seed_saving")));
    assert!(!serde_json::to_string(&hits).unwrap().contains(&*data.to_string_lossy()), "no full path of the library");
    assert_eq!(hits.iter().filter(|h| h["collection"] == "tiny_en_xz").count(), 1, "the redirect and its article count once");
    let seeds = body_json(&zget(&l, "/api/collection-search?q=SEED"));
    assert_eq!(seeds.as_array().unwrap().iter().filter(|h| h["title"] == "Seed saving").count(), 3);
    let old = body_json(&zget(&l, "/api/collection-search?q=water"));
    assert!(old.as_array().unwrap().iter().any(|h| h["path"] == "zim/old_en.zim/A/Water"));
    assert_eq!(body_json(&zget(&l, "/api/collection-search?q=zzzz")), json!([]));
    // what the guides may name: the files really here
    assert_eq!(l.zim_names(), vec![".hidden.zim", "broken.zim", "old_en.zim", "tiny_en_plain.zim", "tiny_en_xz.zim", "tiny_en_zstd.zim"]);
    let cat = l.catalog();
    assert!(topics::item_here(&json!({"zim": "zim/tiny_en_xz.zim/Rain_barrel"}), &l.zim_names(), &cat));
    assert_eq!(body_json(&zget(&l, "/api/library"))["zims"].as_array().unwrap().len(), 6);
    cleanup(&data);
}

#[test]
fn search_fails_only_when_every_file_does() {
    let data = temp("zim-broken");
    let lib = data.join("library-data");
    std::fs::create_dir_all(lib.join("collections")).unwrap();
    std::fs::write(lib.join("catalog.json"), "[]").unwrap();
    std::fs::write(lib.join("collections/broken.zim"), b"not a zim at all").unwrap();
    let l = Library::new(Paths::new(repo(), Some(data.clone())));
    let r = zget(&l, "/api/collection-search?q=water");
    assert_eq!(r.status, 500);
    assert_eq!(body_json(&r)["error"], "Encyclopedia search failed.");
    cleanup(&data);
}
