//! The page as the person sees it, written as PNG: WKWebView's own snapshot of what it shows, so no screen-recording
//! permission is needed and nothing outside the app's window is captured.

use block2::RcBlock;
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
use objc2_foundation::{NSDictionary, NSError};
use std::cell::RefCell;
use std::path::PathBuf;
use wry::WebViewExtMacOS;

pub fn take(webview: &wry::WebView, path: PathBuf, done: impl FnOnce(Result<PathBuf, String>) + 'static) {
    let wk = webview.webview();
    let done = RefCell::new(Some(done));
    let block = RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
        let result = unsafe { write_png(image, error, &path) };
        if let Some(done) = done.borrow_mut().take() {
            done(result);
        }
    });
    unsafe { wk.takeSnapshotWithConfiguration_completionHandler(None, &block) };
}

unsafe fn write_png(image: *mut NSImage, error: *mut NSError, path: &PathBuf) -> Result<PathBuf, String> {
    if image.is_null() {
        let why = if error.is_null() { "no image".to_string() } else { (*error).localizedDescription().to_string() };
        return Err(why);
    }
    let image = &*image;
    let tiff = image.TIFFRepresentation().ok_or("the snapshot has no image data")?;
    let rep = NSBitmapImageRep::imageRepWithData(&tiff).ok_or("the snapshot could not be read")?;
    let png = rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()).ok_or("the snapshot could not be made a PNG")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, png.to_vec()).map_err(|e| e.to_string())?;
    Ok(path.clone())
}
