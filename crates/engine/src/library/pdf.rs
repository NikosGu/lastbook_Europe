//! PDF pages as pictures, drawn by PDFium and kept in `derived/cache/`. PDFium is loaded when first needed, from the
//! folder in `LASTBOOK_PDFIUM`, else beside the executable (or a Mac app's Frameworks folder). Without it, a page
//! that is not drawn yet cannot be: the answer says so, and the page's text still comes from the text route.
//! Ported from the original Lastbook's server.py (render_page, png_bytes, prewarm_previews).

use super::catalog::{inside, Record};
use super::topics::GuideOrder;
use super::Library;
use pdfium_render::prelude::*;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

static PDFIUM: OnceLock<Option<Pdfium>> = OnceLock::new();
/// PDFium is not thread-safe: one document at a time.
static PDF_LOCK: Mutex<()> = Mutex::new(());

/// Where PDFium is looked for, in order.
fn pdfium_places() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(d) = std::env::var_os("LASTBOOK_PDFIUM").filter(|d| !d.is_empty()) {
        let d = PathBuf::from(d);
        v.push(if d.is_file() { d } else { Pdfium::pdfium_platform_library_name_at_path(&d) });
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        v.push(Pdfium::pdfium_platform_library_name_at_path(&dir));
        v.push(Pdfium::pdfium_platform_library_name_at_path(&dir.join("../Frameworks")));
        v.push(Pdfium::pdfium_platform_library_name_at_path(&dir.join("../Resources")));
    }
    v
}

/// PDFium, loaded once, or None when it is not there.
pub fn pdfium() -> Option<&'static Pdfium> {
    PDFIUM
        .get_or_init(|| {
            for p in pdfium_places().into_iter().filter(|p| p.is_file()) {
                match Pdfium::bind_to_library(&p) {
                    Ok(b) => return Some(Pdfium::new(b)),
                    Err(e) => eprintln!("lastbook: PDFium at {} could not be loaded: {e}", p.display()),
                }
            }
            None
        })
        .as_ref()
}

/// Why a page was not drawn.
#[derive(Debug, PartialEq, Eq)]
pub enum PdfError {
    /// No PDFium here.
    Unavailable,
    /// No such file in the library.
    Missing,
    /// PDFium could not open the file.
    Damaged,
    /// The document has no page with that number.
    NoPage,
}

/// The size of each page in points, at most `max` pages.
pub fn page_sizes(file: &Path, max: usize) -> Result<Vec<(f32, f32)>, PdfError> {
    let pdfium = pdfium().ok_or(PdfError::Unavailable)?;
    let _one = PDF_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let doc = pdfium.load_pdf_from_file(file, None).map_err(|_| PdfError::Damaged)?;
    let pages = doc.pages();
    let n = (pages.len() as usize).min(max);
    (0..n).map(|i| pages.page_size(i as PdfPageIndex).map(|r| (r.width().value, r.height().value)).map_err(|_| PdfError::Damaged)).collect()
}

/// Page n (from 1) drawn with its longer side `longer` pixels, as PNG.
pub fn draw(file: &Path, n: usize, longer: u32) -> Result<Vec<u8>, PdfError> {
    let pdfium = pdfium().ok_or(PdfError::Unavailable)?;
    let _one = PDF_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let doc = pdfium.load_pdf_from_file(file, None).map_err(|_| PdfError::Damaged)?;
    let pages = doc.pages();
    if n == 0 || n > pages.len() as usize {
        return Err(PdfError::NoPage);
    }
    let page = pages.get((n - 1) as PdfPageIndex).map_err(|_| PdfError::Damaged)?;
    let (w, h) = (page.width().value, page.height().value);
    if !(w > 0.0 && h > 0.0) {
        return Err(PdfError::Damaged);
    }
    let bitmap = page.render_with_config(&PdfRenderConfig::new().scale_page_by_factor(longer as f32 / w.max(h))).map_err(|_| PdfError::Damaged)?;
    let (bw, bh) = (bitmap.width() as u32, bitmap.height() as u32);
    let rgba = bitmap.as_rgba_bytes();
    let rgb: Vec<u8> = rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    png_bytes(&rgb, bw, bh).ok_or(PdfError::Damaged)
}

/// 8-bit RGB to PNG.
pub fn png_bytes(rgb: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().ok()?;
    writer.write_image_data(rgb).ok()?;
    writer.finish().ok()?;
    Some(out)
}

/// A name for a cache file from an id that may hold anything.
fn cache_name(id: &str) -> String {
    if !id.is_empty() && id.len() <= 100 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        id.to_string()
    } else {
        format!("h{}", &hex::encode(Sha256::digest(id.as_bytes()))[..32])
    }
}

impl Library {
    /// Whether PDF pages can be drawn here.
    pub fn can_draw_pdf(&self) -> bool {
        self.draw_pdf && pdfium().is_some()
    }

    /// Page n of a PDF record as PNG, `width` pixels on its longer side (160 to 1600), from the cache when it has
    /// been drawn before.
    pub fn render_page(&self, r: &Record, n: usize, width: i64) -> Result<Vec<u8>, PdfError> {
        let lib = self.lib().ok_or(PdfError::Missing)?;
        let file = inside(&lib, &r.path).filter(|p| p.is_file()).ok_or(PdfError::Missing)?;
        self.render_file(&r.id, &file, n, width)
    }

    /// Page n of a PDF file as PNG, kept in the cache under `id`.
    pub fn render_file(&self, id: &str, file: &Path, n: usize, width: i64) -> Result<Vec<u8>, PdfError> {
        let width = width.clamp(160, 1600) as u32;
        let cache = self.paths.data.as_ref().ok_or(PdfError::Missing)?.join("derived").join("cache");
        let out = cache.join(format!("{}-{n}-{width}.png", cache_name(id)));
        if let Ok(b) = std::fs::read(&out) {
            return Ok(b);
        }
        if !self.can_draw_pdf() {
            return Err(PdfError::Unavailable);
        }
        let data = draw(file, n, width)?;
        let _ = std::fs::create_dir_all(&cache);
        let tmp = cache.join(format!(".{}-{n}-{width}-{:?}.tmp", cache_name(id), std::thread::current().id()));
        if std::fs::write(&tmp, &data).is_ok() && std::fs::rename(&tmp, &out).is_err() {
            let _ = std::fs::remove_file(&tmp); // Windows: another thread is reading the same page right now
        }
        Ok(data)
    }

    /// Draws a small picture of every page a guide points to, so guide pages show them at once.
    pub fn prewarm_previews(&self) {
        if !self.can_draw_pdf() {
            return;
        }
        let cat = self.catalog();
        for t in self.topics() {
            for i in Library::guide_items(&t, GuideOrder::Previews) {
                if let Some(r) = i["doc"].as_str().and_then(|d| cat.get(d)).filter(|r| r.kind == "pdf") {
                    let page = i["page"].as_u64().filter(|p| *p > 0).unwrap_or(1) as usize;
                    let _ = self.render_page(r, page, 360);
                }
            }
        }
    }

    /// `/api/page/<id>/<n>.png`.
    pub(super) fn page_png(&self, r: &Record, n: usize, width: i64) -> crate::http::Response {
        use super::api::json_status;
        use serde_json::json;
        match self.render_page(r, n, width) {
            Ok(png) => crate::http::Response::new(200, "image/png", png).with("Cache-Control", "max-age=86400").with("X-Content-Type-Options", "nosniff"),
            Err(PdfError::Unavailable) => json_status(503, &json!({"error": "PDF pages cannot be drawn here: PDFium is not installed with this app. The page's text is still available."})),
            Err(PdfError::NoPage) => json_status(404, &json!({"error": "no such page"})),
            Err(PdfError::Missing) => json_status(404, &json!({"error": "the file is not in this library"})),
            Err(PdfError::Damaged) => json_status(500, &json!({"error": "This PDF could not be read."})),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_is_png() {
        let b = png_bytes(&[255, 0, 0, 0, 255, 0], 2, 1).unwrap();
        assert_eq!(&b[..8], b"\x89PNG\r\n\x1a\n");
        assert!(png_bytes(&[1, 2], 2, 1).is_none(), "too few bytes");
        assert_eq!(cache_name("pdfsmall"), "pdfsmall");
        assert!(cache_name("../../etc/passwd").starts_with('h'));
    }
}
