//! The interface's own files, from `web/`, with the headers the original Lastbook sent: the right type, no sniffing, and no
//! framing of the app's pages by anything else.

use crate::http::Response;
use std::path::{Component, Path};

pub fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("html") | Some("htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("webmanifest") => "application/manifest+json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("txt") => "text/plain; charset=utf-8",
        Some("pdf") => "application/pdf",
        Some("mp4") | Some("m4v") => "video/mp4",
        Some("webm") => "video/webm",
        Some("vtt") => "text/vtt; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// A file under `root`, named by a path from the request. Refuses anything that could leave `root`: `..`, an absolute
/// path, a drive letter, a backslash, a NUL, and any link that resolves outside it.
pub fn serve(root: &Path, rel: &str) -> Response {
    let rel = rel.trim_start_matches('/');
    if rel.is_empty() || rel.contains('\\') || rel.contains('\0') {
        return Response::not_found();
    }
    let candidate = Path::new(rel);
    if candidate.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Response::not_found();
    }
    let full = root.join(candidate);
    let (Ok(real_root), Ok(real)) = (root.canonicalize(), full.canonicalize()) else {
        return Response::not_found();
    };
    if !real.starts_with(&real_root) || !real.is_file() {
        return Response::not_found();
    }
    match std::fs::read(&real) {
        Ok(body) => {
            let ctype = content_type(&real);
            let mut r = Response::new(200, ctype, body).with("X-Content-Type-Options", "nosniff").with("Cache-Control", "no-cache");
            if ctype.starts_with("text/html") {
                r = r.with("Content-Security-Policy", "frame-ancestors 'none'");
            }
            r
        }
        Err(_) => Response::not_found(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serves_inside_and_refuses_outside() {
        let dir = std::env::temp_dir().join(format!("lb-static-{}", std::process::id()));
        let web = dir.join("web");
        std::fs::create_dir_all(&web).unwrap();
        std::fs::write(web.join("app.js"), b"ok").unwrap();
        std::fs::write(dir.join("secret.txt"), b"no").unwrap();
        let r = serve(&web, "/app.js");
        assert_eq!(r.status, 200);
        assert_eq!(r.body, b"ok");
        assert_eq!(r.header("Content-Type"), Some("text/javascript; charset=utf-8"));
        for bad in ["../secret.txt", "/../secret.txt", "a/../../secret.txt", "..\\secret.txt", "", "/", "app.js\0"] {
            assert_eq!(serve(&web, bad).status, 404, "{bad:?}");
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.join("secret.txt"), web.join("link.txt")).unwrap();
            assert_eq!(serve(&web, "link.txt").status, 404);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
