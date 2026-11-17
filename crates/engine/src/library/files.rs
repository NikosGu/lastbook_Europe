//! The library's own files: only from its served folders, judged by where a path really leads (links followed), never
//! a hidden file, a sign-in file, a hard link or the library's working folders. Every answer for library content runs
//! none of its code: saved pages and drawings come with a sandbox policy, saved pages are rewritten (saved_pages.rs),
//! a PDF is never handed over (it becomes a page of pictures PDFium has drawn), and a video comes in pieces, by Range.
//! Ported from the original Lastbook's server.py Handler, engine/library.py Handler (file_ok and its checks) and the
//! window bridge's video pieces and PDF viewer (app/bridge.py), which the Rust window does not have.

use super::catalog::Catalog;
use super::pdf::{page_sizes, PdfError};
use super::saved_pages::{self, Local, MAX_PAGE, MAX_READ, MAX_TRANSCRIPT};
use super::Library;
use crate::http::{Request, Response};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, LazyLock, Weak};
use std::time::SystemTime;

/// The library folders a file may be served from. One may itself be a link to another disk (videos-external).
pub const SERVED_FOLDERS: [&str; 9] = ["documents", "pages", "videos", "videos-external", "transcripts", "metadata/videos", "courses", "collections", "assets"];
/// Their addresses.
pub const LIBRARY_PREFIXES: [&str; 9] = ["/documents/", "/pages/", "/videos/", "/videos-external/", "/transcripts/", "/metadata/videos/", "/courses/", "/collections/", "/assets/"];
/// What a browser runs scripts in when it is opened directly. An `<img>` never runs them and shows it as before.
const ACTIVE_SUFFIXES: [&str; 14] = ["", ".html", ".htm", ".shtml", ".xhtml", ".xht", ".svg", ".svgz", ".xml", ".xsl", ".xslt", ".rdf", ".mht", ".mhtml"];
static ACTIVE_TYPES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)html|xml|xsl|svg|hta|component|rfc822|multipart/related").unwrap());
/// No allow-popups, allow-downloads or allow-forms. It covers what a sandbox does not: `<noscript>` frames come alive
/// with scripts off, and `<form>`, `<base>` and `<object>` still load things without any.
pub const NO_CODE_CSP: &str = "default-src 'none'; script-src 'none'; connect-src 'none'; worker-src 'none'; object-src 'none'; manifest-src 'none'; base-uri 'none'; form-action 'none'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; media-src 'self'; frame-src 'self'";
pub static SAVED_PAGE_CSP: LazyLock<String> = LazyLock::new(|| format!("sandbox; {NO_CODE_CSP}"));
/// Same-origin so the reader can follow the entry's title. A page with no scripts can do nothing with it.
pub static ZIM_CSP: LazyLock<String> = LazyLock::new(|| format!("sandbox allow-same-origin; {NO_CODE_CSP}"));
/// The PDF viewer page: a browser loads a picture lazily only where scripting is on, even with no script to run.
static VIEWER_CSP: LazyLock<String> = LazyLock::new(|| format!("sandbox allow-scripts; {NO_CODE_CSP}"));
/// A page that moves itself, for a redirect a webview's custom scheme does not follow. A sandbox would stop the
/// refresh, so this one page says instead that it runs nothing and loads nothing.
const REFRESH_CSP: &str = "default-src 'none'; script-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'self'";

pub const PDF_NOTICE: &[u8] = b"This PDF could not be shown here.";
pub const TOO_LARGE: &[u8] = b"This file is too large to show here.";
/// The most bytes one video answer holds; the player asks for the rest. 2.38 MB is the largest request seen from a
/// player, and a 256 KB cap broke playback. Every answer is held whole in memory on its way to the webview.
pub const VIDEO_PIECE: u64 = 4 * 1024 * 1024;
/// The largest whole file answered at once.
pub const WHOLE_MAX: u64 = 256 * 1024 * 1024;
/// The longer side of a page drawn for the PDF viewer, and the most pages it lists.
const PDF_WIDTH: i64 = 1400;
const PDF_MAX_PAGES: usize = 20000;
const PDF_SIZES_KEPT: usize = 16;
const MEDIA_SUFFIXES: [&str; 13] = [".mp4", ".m4v", ".webm", ".mkv", ".mov", ".ogv", ".ogg", ".oga", ".mp3", ".m4a", ".opus", ".wav", ".flac"];
const RESERVED_NAMES: [&str; 22] = ["con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9"];

static RANGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^bytes=(\d+)-(\d*)").unwrap());
static PDF_PAGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|&)pdf-page=(\d+)(?:&|$)").unwrap());
static SCHEME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z][A-Za-z0-9+.-]*:").unwrap());

/// A file's type, as the original Lastbook gave it for what a library holds.
pub fn mime(path: &str) -> &'static str {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "html" | "htm" | "shtml" => "text/html",
        "xhtml" | "xht" => "application/xhtml+xml",
        "xml" | "xsl" | "xslt" => "text/xml",
        "rdf" => "application/rdf+xml",
        "mht" | "mhtml" => "message/rfc822",
        "svg" | "svgz" => "image/svg+xml",
        "css" => "text/css",
        "js" | "mjs" => "text/javascript",
        "json" => "application/json",
        "txt" => "text/plain",
        "csv" => "text/csv",
        "vtt" => "text/vtt",
        "srt" => "application/x-subrip",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "ico" => "image/vnd.microsoft.icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "pdf" => "application/pdf",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "mov" => "video/quicktime",
        "ogv" => "video/ogg",
        "ogg" | "oga" | "opus" => "audio/ogg",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "wav" => "audio/x-wav",
        "flac" => "audio/flac",
        "zip" => "application/zip",
        "epub" => "application/epub+zip",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        _ => "application/octet-stream",
    }
}

/// NTFS opens "x.", "x " and "x::$DATA" as x, and treats some names as devices; the name checks would not see those.
pub fn segments_ok(path: &str) -> bool {
    if !cfg!(windows) {
        return true;
    }
    path.split('/').all(|s| s == s.trim_end_matches(['.', ' ']) && !s.contains(':') && !s.contains('\\') && !RESERVED_NAMES.contains(&s.split('.').next().unwrap_or("").to_ascii_lowercase().as_str()))
}

/// The file name itself: hidden files, sign-in files, and yt-dlp's .info.json, which can hold request headers.
pub(super) fn name_refused(parts: &[String]) -> bool {
    let name = parts.last().map(|n| n.to_lowercase()).unwrap_or_default();
    name.starts_with('.') || name.starts_with("video-auth") || name.contains("cookie") || name.ends_with(".info.json")
}

/// Folders, below the library's top. Hidden folders stay out everywhere. These four names are the library's own
/// working folders; inside a served folder they are just names (a course has a page called tools).
fn folder_refused(parts: &[String]) -> bool {
    let parts: Vec<String> = parts.iter().map(|p| p.to_lowercase()).collect();
    if parts.iter().any(|p| p.starts_with('.')) {
        return true;
    }
    let joined = parts.join("/");
    if SERVED_FOLDERS.iter().any(|f| joined.starts_with(&format!("{f}/"))) {
        return false;
    }
    parts.iter().any(|p| ["tools", "logs", "work", "runtime"].contains(&p.as_str()))
}

/// Run on what a path really resolves to too: a link can end at "x.info.json:s", which the name check alone would pass.
fn parts_refused(parts: &[String]) -> bool {
    name_refused(parts) || folder_refused(parts) || !segments_ok(&parts.join("/"))
}

/// The parts of `real` below `root`, or None when it is not inside.
fn parts_below(real: &Path, root: &Path) -> Option<Vec<String>> {
    let rest = real.strip_prefix(root).ok()?;
    Some(rest.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect())
}

/// How many names this file has. A hard link looks like an ordinary file at an ordinary path, so where else it lives
/// cannot be seen.
#[cfg(unix)]
fn links(p: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(p).map(|m| m.nlink()).unwrap_or(u64::MAX)
}

#[cfg(windows)]
fn links(p: &Path) -> u64 {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION};
    let Ok(f) = std::fs::File::open(p) else { return u64::MAX };
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(f.as_raw_handle() as _, &mut info) } == 0 {
        return u64::MAX;
    }
    info.nNumberOfLinks as u64
}

/// May this path be read, judged by where it really leads? `root` is the library folder, as resolved.
pub fn file_ok(root: &Path, path: &Path) -> bool {
    if path.is_file() && links(path) > 1 {
        return false;
    }
    let Some(asked) = parts_below(path, root) else { return false };
    if parts_refused(&asked) {
        return false;
    }
    let Ok(real) = path.canonicalize() else { return false };
    for folder in SERVED_FOLDERS {
        if let Some(parts) = root.join(folder).canonicalize().ok().and_then(|f| parts_below(&real, &f)) {
            let mut all: Vec<String> = folder.split('/').map(str::to_string).collect();
            all.extend(parts);
            return !parts_refused(&all);
        }
    }
    match parts_below(&real, root) {
        Some(parts) => !parts_refused(&parts) && (parts.is_empty() || parts == ["reader.css"]),
        None => false,
    }
}

/// An address the library may answer with a file: one of its folders (or reader.css), with no `..`, no NUL and no
/// .info.json.
pub fn path_ok(path: &str) -> bool {
    !path.contains('\0')
        && (path == "/reader.css" || LIBRARY_PREFIXES.iter().any(|p| path.starts_with(p)))
        && !path.contains("/..")
        && !path.to_lowercase().ends_with(".info.json")
        && segments_ok(path)
}

/// A request path as a file under the library folder: only plain names (no `..`, no drive such as `C:`), and whether
/// it ended in `/`.
fn translate(root: &Path, path: &str) -> (PathBuf, bool) {
    let mut p = root.to_path_buf();
    for word in path.split('/') {
        let plain = Path::new(word).components().collect::<Vec<_>>();
        if word.contains('\\') || !matches!(plain.as_slice(), [Component::Normal(_)]) {
            continue;
        }
        p.push(word);
    }
    (p, path.ends_with('/'))
}

/// The suffix of a request path, as the original Lastbook took it: "" for a folder or a name without one.
fn suffix(path: &str) -> String {
    let name = path.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    match name.rfind('.') {
        Some(i) if i > 0 && i + 1 < name.len() => name[i..].to_lowercase(),
        _ => String::new(),
    }
}

/// The headers every answer for library content has (server.py end_headers).
pub fn content_headers(mut r: Response, path: &str) -> Response {
    r = r.with("X-Content-Type-Options", "nosniff");
    if r.header("Content-Security-Policy").is_some() {
        return r;
    }
    let ctype = r.header("Content-Type").unwrap_or("").to_string();
    let sfx = suffix(path);
    if r.status >= 300 || ACTIVE_SUFFIXES.contains(&sfx.as_str()) || ACTIVE_TYPES.is_match(&ctype) {
        r = r.with("Content-Security-Policy", &SAVED_PAGE_CSP).with("X-DNS-Prefetch-Control", "off"); // a policy does not cover the names a browser looks up ahead of a click
    } else if [".woff2", ".woff", ".ttf", ".otf"].contains(&sfx.as_str()) {
        r = r.with("Access-Control-Allow-Origin", "null"); // the sandbox leaves a saved page's origin null, and fonts always load by CORS
    }
    r
}

/// A refusal under a library address.
fn not_found() -> Response {
    Response::new(404, "text/html; charset=utf-8", "<!doctype html><title>Not found</title><p>Not found.")
}

fn notice(status: u16, body: &[u8]) -> Response {
    Response::new(status, "text/html; charset=utf-8", body.to_vec()).with("Content-Security-Policy", &SAVED_PAGE_CSP)
}

/// Text for HTML, quotes included.
fn esc(s: &str) -> String {
    saved_pages::escape(s)
}

/// A path for an address: each part percent-encoded but `/`.
pub fn quote(path: &str) -> String {
    let mut out = String::new();
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"/_.-~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// A redirect as a page that moves itself (a webview's custom scheme follows no redirect).
pub fn refresh(location: &str) -> Response {
    let body = format!("<!doctype html><meta http-equiv=\"refresh\" content=\"0;url={}\">", esc(location));
    Response::new(200, "text/html; charset=utf-8", body).with("Content-Security-Policy", REFRESH_CSP).with("X-Content-Type-Options", "nosniff")
}

/// Removes `.` and `..` from a path, as an address is resolved.
fn remove_dots(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let parts: Vec<&str> = path.split('/').collect();
    for (i, p) in parts.iter().enumerate() {
        match *p {
            "." => {}
            ".." => {
                if out.len() > 1 {
                    out.pop();
                }
            }
            _ => out.push(p),
        }
        if (*p == "." || *p == "..") && i == parts.len() - 1 {
            out.push("");
        }
    }
    out.join("/")
}

/// Sizes of PDFs' pages, by file, modified time and size.
pub(super) type PdfSizes = HashMap<(PathBuf, Option<SystemTime>, u64), Arc<Vec<(f32, f32)>>>;

impl Library {
    /// The catalog's original addresses (source_url, final_url) and the library's copy of each.
    fn links(&self, cat: &Arc<Catalog>) -> Arc<HashMap<String, String>> {
        let mut cache = self.links_cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((c, map)) = cache.as_ref() {
            if Weak::ptr_eq(c, &Arc::downgrade(cat)) {
                return map.clone();
            }
        }
        let mut map = HashMap::new();
        for r in &cat.records {
            for k in ["source_url", "final_url"] {
                if r.truthy(k) {
                    map.insert(r.str(k).to_string(), r.path.clone());
                }
            }
        }
        let map = Arc::new(map);
        *cache = Some((Arc::downgrade(cat), map.clone()));
        map
    }

    /// A library address that is a file: GET and HEAD.
    pub(super) fn serve_file(&self, req: &Request) -> Response {
        let path = req.path.as_str();
        content_headers(self.file_answer(req), path)
    }

    fn file_answer(&self, req: &Request) -> Response {
        let path = req.path.as_str();
        let Some(root) = self.lib().and_then(|l| l.canonicalize().ok()) else { return not_found() };
        if !path_ok(path) {
            return not_found();
        }
        let (full, trailing) = translate(&root, path);
        if !file_ok(&root, &full) {
            return not_found();
        }
        if full.is_dir() {
            if !path.ends_with('/') {
                let q = if req.query.is_empty() { String::new() } else { format!("?{}", req.query) };
                return refresh(&format!("{}/{q}", quote(path)));
            }
            for index in ["index.html", "index.htm"] {
                let p = full.join(index);
                if p.exists() {
                    if !file_ok(&root, &p) {
                        return not_found();
                    }
                    return self.send(req, &root, &p);
                }
            }
            return not_found(); // no folder listings
        }
        if trailing || !full.is_file() {
            return not_found();
        }
        self.send(req, &root, &full)
    }

    fn send(&self, req: &Request, root: &Path, file: &Path) -> Response {
        let name = file.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        let size = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
        if (name.ends_with(".html") || name.ends_with(".htm")) && size as usize <= MAX_READ {
            return self.saved_page(req, root, file);
        }
        if name.ends_with(".pdf") {
            return self.pdf_answer(req, file);
        }
        if MEDIA_SUFFIXES.iter().any(|s| name.ends_with(s)) {
            return media(req, file, size);
        }
        if size > WHOLE_MAX {
            return notice(413, TOO_LARGE);
        }
        match std::fs::read(file) {
            Ok(body) => Response::new(200, mime(&name), body).with("Cache-Control", "no-cache"),
            Err(_) => not_found(),
        }
    }

    /// A saved page, rewritten as it is served.
    fn saved_page(&self, req: &Request, root: &Path, file: &Path) -> Response {
        let Ok(data) = std::fs::read(file) else { return not_found() };
        let cat = self.catalog();
        let links = self.links(&cat);
        let page_path = req.path.clone();
        let read = |src: &str| self.saved_page_part(root, &page_path, src);
        let link = |u: &str| links.get(u).cloned();
        let small = data.len() <= MAX_PAGE;
        let mut out = saved_pages::rewrite(&data, Some(&Local { read: &read, link: &link }));
        if small && req.path.contains("/documents/lastbook/") {
            out = saved_pages::own_look(&out);
        }
        let ctype = if std::str::from_utf8(&out).is_ok() { "text/html; charset=utf-8" } else { "text/html" };
        Response::new(200, ctype, out).with("Cache-Control", "no-cache")
    }

    /// A transcript a saved page names, by the same rules as asking for it directly.
    pub fn saved_page_part(&self, root: &Path, page: &str, src: &str) -> Option<Vec<u8>> {
        let src = src.split(['?', '#']).next().unwrap_or("");
        if SCHEME.is_match(src) || src.starts_with("//") {
            return None;
        }
        let target = if src.starts_with('/') { src.to_string() } else { format!("{}{src}", &page[..page.rfind('/').map(|i| i + 1).unwrap_or(0)]) };
        let target = crate::http::percent_decode(&remove_dots(&target));
        if !target.starts_with('/') || target.starts_with("//") || target.contains("/..") || !LIBRARY_PREFIXES.iter().any(|p| target.starts_with(p)) || !segments_ok(&target) {
            return None;
        }
        let (real, _) = translate(root, &target);
        let ok = real.is_file() && file_ok(root, &real) && std::fs::metadata(&real).map(|m| m.len() as usize <= MAX_TRANSCRIPT).unwrap_or(false);
        if ok {
            std::fs::read(&real).ok()
        } else {
            None
        }
    }

    /// A PDF is never handed over, whoever asks: with `?pdf-page=N` it is that page as a picture, otherwise a page
    /// listing all its pages as pictures, which runs nothing.
    fn pdf_answer(&self, req: &Request, file: &Path) -> Response {
        let cat = self.catalog();
        let rel = req.path.trim_start_matches('/');
        let rec = cat.by_path(rel).filter(|r| r.kind == "pdf");
        if let Some(m) = PDF_PAGE.captures(&req.query) {
            let id = rec.map(|r| r.id.clone()).unwrap_or_else(|| format!("file-{}", &hex::encode(Sha256::digest(req.path.as_bytes()))[..32]));
            let n = m[1].parse::<usize>().unwrap_or(0);
            return match self.render_file(&id, file, n, PDF_WIDTH) {
                Ok(png) => Response::new(200, "image/png", png).with("Content-Security-Policy", &SAVED_PAGE_CSP).with("Cache-Control", "max-age=86400"),
                Err(PdfError::Unavailable) => notice(503, PDF_NOTICE),
                Err(_) => notice(404, PDF_NOTICE),
            };
        }
        if !self.can_draw_pdf() {
            return notice(503, PDF_NOTICE);
        }
        let sizes = match self.pdf_sizes(file) {
            Ok(s) => s,
            Err(PdfError::Unavailable) => return notice(503, PDF_NOTICE),
            Err(_) => return notice(404, PDF_NOTICE), // a damaged or locked PDF
        };
        let title = esc(rec.map(|r| r.title.as_str()).filter(|t| !t.is_empty()).unwrap_or(&file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
        let lang = rec.and_then(|r| r.langs.first().cloned()).unwrap_or_else(|| "en".into());
        let rows: String = sizes
            .iter()
            .enumerate()
            .map(|(i, (w, h))| {
                let (w, h) = ((*w as f64).round_ties_even().max(1.0), (*h as f64).round_ties_even().max(1.0));
                format!("<div class=\"p\" id=\"page={n}\"><img loading=\"lazy\" decoding=\"async\" alt=\"Page {n}\" width=\"{w}\" height=\"{h}\" src=\"?pdf-page={n}\"></div>", n = i + 1)
            })
            .collect();
        let body = format!(
            "<!doctype html><html lang=\"{}\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><title>{title}</title><style>\
             html{{background:#525659}}body{{margin:0;padding:8px}}.p{{max-width:1000px;margin:0 auto 8px}}.p img{{display:block;width:100%;height:auto;background:#fff}}\
             </style></head><body>{rows}</body></html>",
            esc(&lang)
        );
        Response::new(200, "text/html; charset=utf-8", body).with("Content-Security-Policy", &VIEWER_CSP)
    }

    /// A PDF's page sizes, kept for the last few files.
    fn pdf_sizes(&self, file: &Path) -> Result<Arc<Vec<(f32, f32)>>, PdfError> {
        let m = std::fs::metadata(file).map_err(|_| PdfError::Missing)?;
        let key = (file.to_path_buf(), m.modified().ok(), m.len());
        if let Some(s) = self.pdf_sizes.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
            return Ok(s.clone());
        }
        let sizes = Arc::new(page_sizes(file, PDF_MAX_PAGES)?);
        let mut kept = self.pdf_sizes.lock().unwrap_or_else(|e| e.into_inner());
        if kept.len() >= PDF_SIZES_KEPT {
            kept.clear();
        }
        kept.insert(key, sizes.clone());
        Ok(sizes)
    }
}

/// A video or sound file, in pieces: a Range answer holds at most one piece, and one asked for with no Range gets the
/// first. HEAD gives its whole length.
fn media(req: &Request, file: &Path, size: u64) -> Response {
    let ctype = mime(&file.to_string_lossy());
    if req.method == "HEAD" {
        return Response::new(200, ctype, Vec::new()).with("Content-Length", &size.to_string()).with("Accept-Ranges", "bytes");
    }
    let refuse = || Response::new(416, "text/plain; charset=utf-8", "Requested range not satisfiable").with("Content-Range", &format!("bytes */{size}"));
    let (start, end) = match req.header("Range") {
        None => (0, VIDEO_PIECE - 1),
        Some(asked) => {
            // read as the original Lastbook read it: from the start, so a second range is ignored
            let Some(m) = RANGE.captures(asked) else { return refuse() };
            let Ok(start) = m[1].parse::<u64>() else { return refuse() };
            let end = if m[2].is_empty() { None } else { m[2].parse::<u64>().ok() };
            match end {
                Some(e) if e < start || e - start < VIDEO_PIECE => (start, e),
                _ => (start, start.saturating_add(VIDEO_PIECE - 1)),
            }
        }
    };
    if size == 0 {
        return refuse();
    }
    let end = end.min(size - 1);
    if start > end {
        return refuse();
    }
    let n = (end - start + 1) as usize;
    let mut body = vec![0u8; n];
    let read = std::fs::File::open(file).and_then(|mut f| {
        f.seek(SeekFrom::Start(start))?;
        f.read_exact(&mut body)
    });
    if read.is_err() {
        return refuse();
    }
    Response::new(206, ctype, body)
        .with("Accept-Ranges", "bytes")
        .with("Content-Range", &format!("bytes {start}-{end}/{size}"))
        .with("Content-Length", &n.to_string())
}
