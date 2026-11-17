//! Linux: the app's own picture of the page, from WebKitGTK's snapshot of the visible part, written as PNG.

use std::path::PathBuf;
use webkit2gtk::{SnapshotOptions, SnapshotRegion, WebViewExt};
use wry::WebViewExtUnix;

pub fn take(webview: &wry::WebView, path: PathBuf, done: impl FnOnce(Result<PathBuf, String>) + 'static) {
    webview.webview().snapshot(SnapshotRegion::Visible, SnapshotOptions::NONE, None::<&webkit2gtk::gio::Cancellable>, move |result| {
        let written = result.map_err(|e| e.to_string()).and_then(|surface| {
            let image = cairo::ImageSurface::try_from(surface).map_err(|_| "the snapshot is not an image".to_string())?;
            let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
            image.write_to_png(&mut file).map_err(|e| e.to_string())?;
            Ok(path)
        });
        done(written);
    });
}
