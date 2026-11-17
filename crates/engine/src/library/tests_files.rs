//! The library's files as the window gets them: Range forms on video, PDFs that never reach the webview, every
//! refusal, and the headers that keep library content from running code. Ported from app/tests/test_bridge.py.

use super::files::{file_ok, ZIM_CSP, NO_CODE_CSP, PDF_NOTICE, TOO_LARGE, VIDEO_PIECE};
use super::tests::*;
use crate::http::{Request, Response};
use sha2::{Digest, Sha256};

const A: &str = "/videos/a.mp4";

fn ask(method: &str, path: &str, range: Option<&str>) -> Response {
    let mut r = Request::get(path);
    r.method = method.into();
    if let Some(range) = range {
        r.headers.push(("Range".into(), range.into()));
    }
    engine().handle(&r)
}

fn range(path: &str, rng: &str) -> Response {
    ask("GET", path, Some(rng))
}

fn csp(r: &Response) -> &str {
    r.header("Content-Security-Policy").unwrap_or("")
}

/// The policy of an answer for library content: a sandbox, nothing that runs or loads from elsewhere.
fn no_code(r: &Response, sandbox: &str) {
    let c = csp(r);
    assert!(c.starts_with(sandbox), "{c:?}");
    for word in ["allow-scripts", "allow-popups", "allow-forms", "allow-downloads", "unsafe-eval", "blob:", "http"] {
        assert!(!c.contains(word), "{word} in {c}");
    }
    for rule in ["default-src 'none'", "script-src 'none'", "object-src 'none'", "connect-src 'none'", "form-action 'none'", "base-uri 'none'"] {
        assert!(c.contains(rule), "{rule} not in {c}");
    }
}

fn check_slice(r: &Response, start: usize, end: usize) {
    assert_eq!(r.status, 206, "{}", String::from_utf8_lossy(&r.body[..r.body.len().min(200)]));
    assert_eq!(r.header("Content-Range"), Some(format!("bytes {start}-{end}/{A_SIZE}").as_str()));
    assert_eq!(r.header("Content-Length").unwrap().parse::<usize>().unwrap(), end - start + 1);
    assert_eq!(r.body.len(), end - start + 1);
    assert_eq!(Sha256::digest(&r.body), Sha256::digest(pattern(start, end - start + 1)));
    assert_eq!(r.header("Accept-Ranges"), Some("bytes"));
}

const PIECE: usize = VIDEO_PIECE as usize;

#[test]
fn video_ranges() {
    check_slice(&range(A, "bytes=0-99"), 0, 99);
    check_slice(&range(A, "bytes=1000-1999"), 1000, 1999);
    check_slice(&range(A, "bytes=0-0"), 0, 0);
    // an open range, or a long one, gets one piece; the largest a player asks for comes whole
    check_slice(&range(A, "bytes=0-"), 0, PIECE - 1);
    check_slice(&range(A, "bytes=12345-"), 12345, 12345 + PIECE - 1);
    check_slice(&range(A, &format!("bytes=5-{}", A_SIZE - 1)), 5, 5 + PIECE - 1);
    let largest = 2_380_000;
    assert!(PIECE > largest);
    check_slice(&range(A, &format!("bytes=0-{}", largest - 1)), 0, largest - 1);
    // no Range: the first piece
    check_slice(&ask("GET", A, None), 0, PIECE - 1);
    check_slice(&range(A, &format!("bytes={}-{}", A_SIZE - 10, A_SIZE + 500)), A_SIZE - 10, A_SIZE - 1);
    check_slice(&range(A, &format!("bytes={}-", A_SIZE - 1)), A_SIZE - 1, A_SIZE - 1);
    // a second range is read as the first
    check_slice(&range(A, "bytes=0-9,20-29"), 0, 9);
    check_slice(&range(A, &format!("bytes=0-{A_SIZE},20-29")), 0, PIECE - 1);
    for bad in [format!("bytes={A_SIZE}-"), "bytes=5-3".into(), "bytes=-500".into(), "bytes=abc".into(), "items=0-5".into(), "bytes=".into(), "bytes=99999999999999999999-".into()] {
        assert_eq!(range(A, &bad).status, 416, "{bad}");
    }
}

#[test]
fn video_walk_other_types_and_head() {
    let (mut got, mut off, mut n) = (Vec::new(), 0, 0);
    while off < A_SIZE {
        let r = range(A, &format!("bytes={off}-"));
        assert_eq!(r.status, 206);
        off += r.body.len();
        got.extend(r.body);
        n += 1;
    }
    assert_eq!(Sha256::digest(&got), Sha256::digest(pattern(0, A_SIZE)));
    assert_eq!(n, A_SIZE.div_ceil(PIECE));
    let r = range("/videos/b.webm", "bytes=0-9");
    assert_eq!((r.status, r.header("Content-Type")), (206, Some("video/webm")));
    let r = ask("GET", "/videos/huge.mkv", None);
    assert_eq!((r.status, r.body.len(), r.header("Content-Type")), (206, PIECE, Some("video/x-matroska")));
    let r = ask("HEAD", A, None);
    assert_eq!(r.status, 200);
    assert_eq!(r.header("Content-Length"), Some(A_SIZE.to_string().as_str()), "HEAD is not capped");
    assert!(r.body.is_empty());
    assert!(csp(&r).is_empty(), "a video is not a page: its own controls must work");
}

#[test]
fn other_files_whole_and_never_too_large() {
    let r = range("/documents/mid.bin", "bytes=0-9");
    assert_eq!(r.status, 200, "other files ignore Range");
    assert_eq!(r.body.len(), MIB);
    let r = ask("GET", "/documents/giant.bin", None);
    assert_eq!((r.status, r.body.as_slice()), (413, TOO_LARGE));
    assert_eq!(ask("GET", "/documents/plain.txt", None).body, b"plain text file\n");
    let r = ask("GET", "/reader.css", None);
    assert_eq!((r.status, r.body.as_slice()), (200, &b"body{}"[..]));
    assert_eq!(r.header("X-Content-Type-Options"), Some("nosniff"));
    let font = fixture().lib.join("assets/f.woff2");
    std::fs::write(&font, b"wOF2").unwrap();
    assert_eq!(ask("GET", "/assets/f.woff2", None).header("Access-Control-Allow-Origin"), Some("null"), "fonts load by CORS from a sandboxed page");
    assert_eq!(ask("GET", "/documents/پ.txt", None).status, 404, "no such file, and no crash on the way");
}

#[test]
fn a_pdf_never_reaches_the_webview() {
    let mut l = library();
    for (p, rng) in [("/documents/real.pdf", None), ("/documents/real.pdf", Some("bytes=0-99")), ("/documents/real.pdf", Some("bytes=-500")), ("/documents/real.pdf", Some("bytes=0-"))] {
        let r = ask("GET", p, rng);
        assert!(!r.body.windows(4).any(|w| w == b"%PDF"), "{rng:?}");
        assert!(r.header("Accept-Ranges").is_none());
        assert!(r.header("Content-Type").unwrap().starts_with("text/html"));
    }
    l.draw_pdf = false;
    let r = l.route(&Request::get("/documents/real.pdf")).unwrap();
    assert_eq!((r.status, r.body.as_slice()), (503, PDF_NOTICE), "no PDFium here: a notice");
    no_code(&r, "sandbox;");
    if !library().can_draw_pdf() {
        eprintln!("no PDFium here (set LASTBOOK_PDFIUM to its folder): the PDF viewer not checked");
        return;
    }
    let r = ask("GET", "/documents/real.pdf", None);
    assert_eq!(r.status, 200);
    assert!(csp(&r).starts_with("sandbox allow-scripts;"));
    assert!(csp(&r).contains("script-src 'none'"));
    let b = String::from_utf8(r.body.clone()).unwrap();
    assert_eq!(b.matches("class=\"p\"").count(), 3);
    assert!(b.contains("id=\"page=3\"") && b.contains("src=\"?pdf-page=2\"") && b.contains("width=\"300\" height=\"400\""));
    assert!(b.contains("<title>realpdf</title>"));
    assert!(!b.to_lowercase().contains("<script"));
    assert!(!regex::Regex::new(r"\son\w+=").unwrap().is_match(b.split("</style>").nth(1).unwrap()));
    let r = ask("GET", "/documents/real.pdf?pdf-page=2", None);
    assert_eq!((r.status, r.header("Content-Type")), (200, Some("image/png")));
    assert_eq!(&r.body[..8], b"\x89PNG\r\n\x1a\n");
    no_code(&r, "sandbox;");
    assert_eq!(ask("GET", "/documents/real.pdf?pdf-page=9", None).status, 404);
    assert_eq!(ask("GET", "/documents/real.pdf?pdf-page=0", None).status, 404);
    for rng in [None, Some("bytes=0-99")] {
        let r = ask("GET", "/documents/s.pdf", rng);
        assert_eq!((r.status, r.body.as_slice()), (404, PDF_NOTICE), "a damaged PDF shows a notice, not its bytes");
        assert!(csp(&r).contains("sandbox"));
    }
    let r = ask("HEAD", "/documents/real.pdf", None);
    assert_eq!(r.status, 200);
    assert!(r.body.is_empty());
}

#[test]
fn path_attacks_never_reach_the_canary() {
    let attacks = [
        "/../canary-root.txt", "/documents/../canary-root.txt", "/%2e%2e/canary-root.txt", "/documents/%2e%2e/%2e%2e/outside-canary.txt", "/canary-root.txt",
        "/video-auth.json", "/documents/video-auth-copy.json", "/documents/x.info.json", "/documents/X.INFO.JSON", "/documents/.hidden", "/tools/secret.txt",
        "/logs/x.log", "/catalog.json", "/search.sqlite", "/documents/hl.txt", "/pages/hl2.txt", "/zim/../canary-root.txt", "/zim/%2e%2e%2fcanary-root.txt/x",
        "/ui/../server.py", "/ui/%2e%2e/server.py", "/documents%2f..%2f..%2fcanary-root.txt", "/documents/..\\canary-root.txt",
        "/documents/plain.txt/../../canary-root.txt", "/assets/../tools/secret.txt", "/documents/plain.txt/", "/documents/a\0b",
    ];
    for t in attacks {
        for m in ["GET", "HEAD"] {
            let r = ask(m, t, None);
            assert!(r.status >= 400, "{m} {t}: {}", r.status);
            assert!(!r.body.windows(CANARY.len()).any(|w| w == CANARY), "{t}");
        }
    }
    let r = ask("GET", "/?x=../canary-root.txt", None);
    assert_eq!(r.status, 200);
    assert!(!r.body.windows(CANARY.len()).any(|w| w == CANARY));
    // a hard link looks like an ordinary file: refused; a plain file is served
    assert_eq!(ask("GET", "/documents/hl.txt", None).status, 404);
    assert_eq!(ask("GET", "/pages/hl2.txt", None).status, 404);
    assert_eq!(ask("GET", "/documents/plain.txt", None).status, 200);
    let f = fixture();
    if f.linked {
        for m in ["GET", "HEAD"] {
            let r = ask(m, "/documents/out.txt", None);
            assert!(r.status >= 400, "a link out of the library");
            assert!(!r.body.windows(CANARY.len()).any(|w| w == CANARY));
        }
    } else {
        eprintln!("no right to make symbolic links here: the link out of the library is not checked");
    }
    let root = f.lib.canonicalize().unwrap();
    assert!(!file_ok(&root, &root.join("canary-root.txt")));
    assert!(file_ok(&root, &root.join("reader.css")));
}

#[test]
fn saved_pages_and_drawings_run_no_code() {
    let r = ask("GET", "/pages/s.html", None);
    assert_eq!(r.status, 200);
    no_code(&r, "sandbox;");
    assert!(!csp(&r).contains("allow-same-origin"));
    assert_eq!(r.header("X-DNS-Prefetch-Control"), Some("off"));
    no_code(&ask("GET", "/pages/a.svg", None), "sandbox;");
    let d = fixture().lib.join("pages/kinds");
    std::fs::create_dir_all(d.join("folder")).unwrap();
    std::fs::write(d.join("folder/index.html"), b"<p>in a folder").unwrap();
    std::fs::write(d.join("nosuffix"), b"<script>1</script>").unwrap();
    std::fs::write(d.join("upper.HTML"), b"<script>1</script>").unwrap();
    std::fs::write(d.join("x.xhtml"), b"<html xmlns=\"http://www.w3.org/1999/xhtml\"><script>1</script></html>").unwrap();
    for t in ["/pages/kinds/nosuffix", "/pages/kinds/upper.HTML", "/pages/kinds/x.xhtml", "/pages/kinds/folder/", "/pages/kinds/folder/index.html", "/pages/kinds/none.html", "/pages/kinds/none", "/%70ages/kinds/nosuffix"] {
        let r = ask("GET", t, None);
        assert!(r.status == 200 || r.status == 404, "{t}: {}", r.status);
        no_code(&r, "sandbox;");
    }
    // a folder with no slash: a page that moves itself, which holds no code either and cannot be sandboxed
    let r = ask("GET", "/pages/kinds/folder", None);
    assert_eq!(r.status, 200);
    assert_eq!(csp(&r), "default-src 'none'; script-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'self'");
    let b = String::from_utf8_lossy(&r.body);
    assert!(b.contains("http-equiv=\"refresh\"") && b.contains("url=/pages/kinds/folder/") && !b.contains("<script"));
    assert_eq!(ZIM_CSP.as_str(), format!("sandbox allow-same-origin; {NO_CODE_CSP}"));
}

#[test]
fn a_library_page_may_be_called_tools() {
    let lib = &fixture().lib;
    for rel in ["courses/c1/resources/tools/index.html", "pages/work/index.html", "documents/logs/index.html", "assets/runtime/index.html", "metadata/videos/tools/index.html", "collections/tools/index.html"] {
        let p = lib.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"<!doctype html><title>t</title><script>1</script><p>a real page").unwrap();
        for t in [format!("/{rel}"), format!("/{}", &rel[..rel.len() - "index.html".len()])] {
            let r = ask("GET", &t, None);
            assert_eq!(r.status, 200, "{t}");
            assert!(String::from_utf8_lossy(&r.body).contains("a real page"), "{t}");
            no_code(&r, "sandbox;");
        }
    }
    let p = lib.join("courses/c1/resources/tools/index.xml");
    std::fs::write(&p, b"<?xml version=\"1.0\"?><x/>").unwrap();
    no_code(&ask("GET", "/courses/c1/resources/tools/index.xml", None), "sandbox;");
    std::fs::create_dir_all(lib.join("courses/c1/tools/.Data")).unwrap();
    std::fs::write(lib.join("courses/c1/tools/.Data/x.txt"), CANARY).unwrap();
    std::fs::create_dir_all(lib.join("courses/c1/.git")).unwrap();
    std::fs::write(lib.join("courses/c1/.git/config"), CANARY).unwrap();
    for t in ["/courses/c1/tools/.Data/x.txt", "/courses/c1/.git/config", "/tools/secret.txt", "/TOOLS/secret.txt", "/logs/x.log", "/courses/../tools/secret.txt", "/courses/c1/%2e%2e/%2e%2e/tools/secret.txt"] {
        let r = ask("GET", t, None);
        assert!(r.status >= 400, "{t}");
        assert!(!r.body.windows(CANARY.len()).any(|w| w == CANARY), "{t}");
    }
    // a link from a served folder to the library's own tools folder
    #[cfg(unix)]
    {
        let link = lib.join("courses/c1/tools-link");
        if !link.exists() {
            std::os::unix::fs::symlink(lib.join("tools"), &link).unwrap();
        }
        let r = ask("GET", "/courses/c1/tools-link/secret.txt", None);
        assert!(r.status >= 400);
        assert!(!r.body.windows(CANARY.len()).any(|w| w == CANARY));
    }
}

#[test]
fn saved_page_rewriting_as_served() {
    let d = fixture().lib.join("pages/unfold");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("talk.vtt"), b"WEBVTT\n\n00:01.000 --> 00:02.000\nHello <b>there</b> & welcome\n\n00:02.000 --> 00:03.000\nHello <b>there</b> & welcome\n").unwrap();
    let page = concat!(
        "<!doctype html><html><head><title>t</title><link rel=preconnect href=\"http://x.invalid\"></head><body><a href=\"x.pdf\" target=\"_blank\" download>f</a>",
        "<a href=\"https://e.example/\" target=\"_blank\">w</a><button data-toggle=\"offcanvas\" data-target=\"#menu\">m</button><video><track src=\"talk.vtt\"></video>",
        "<video><track src=\"../../../secret.vtt\"></video></body></html>"
    );
    std::fs::write(d.join("ocw.html"), page).unwrap();
    std::fs::write(d.join("index.html"), page).unwrap();
    let local = regex::Regex::new(r#"<a href="x.pdf"\s*>"#).unwrap();
    for t in ["/pages/unfold/ocw.html", "/pages/unfold/", "/pages/unfold/index.html"] {
        let r = ask("GET", t, None);
        assert_eq!(r.status, 200, "{t}");
        no_code(&r, "sandbox;");
        assert_eq!(r.header("Content-Type"), Some("text/html; charset=utf-8"));
        let b = String::from_utf8(r.body).unwrap();
        assert!(b.to_lowercase().find("<head>").unwrap() < b.find("<style>").unwrap());
        assert!(b.contains(".collapse") && !b.contains("x.invalid") && !b.contains("<script"));
        assert!(local.is_match(&b));
        assert!(b.contains("https://e.example/\" target=\"_blank\""));
        assert!(b.contains("<a href=\"#menu\""));
        assert_eq!(b.matches("Hello there &amp; welcome").count(), 1, "{t}");
    }
    assert_eq!(ask("GET", "/pages/s.html", None).status, 200);
    // the transcript a page names stays in the library
    let l = library();
    let root = fixture().lib.canonicalize().unwrap();
    assert!(l.saved_page_part(&root, "/pages/unfold/ocw.html", "talk.vtt").is_some());
    for src in ["../../../canary-root.txt", "https://evil.example/a.vtt", "//evil.example/a.vtt", "/api/health", "../../tools/secret.txt", "%2e%2e/%2e%2e/canary-root.txt"] {
        assert!(l.saved_page_part(&root, "/pages/unfold/ocw.html", src).is_none(), "{src}");
    }
}

#[test]
fn the_app_page_may_not_sit_in_a_frame() {
    let r = ask("GET", "/", None);
    assert_eq!(r.status, 200);
    assert_eq!(csp(&r), "frame-ancestors 'none'");
    assert!(r.header("Server").is_none() && r.header("Date").is_none());
}
