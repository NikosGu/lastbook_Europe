//! Windows: the app's own picture of the page, from WebView2's CapturePreview, written as PNG.

use std::path::PathBuf;
use webview2_com::CapturePreviewCompletedHandler;
use webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG;
use windows::Win32::System::Com::{IStream, STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET};
use windows::Win32::UI::Shell::SHCreateMemStream;
use wry::WebViewExtWindows;

fn read_all(stream: &IStream) -> Result<Vec<u8>, String> {
    unsafe {
        stream.Seek(0, STREAM_SEEK_SET, None).map_err(|e| e.to_string())?;
        let mut stat = STATSTG::default();
        stream.Stat(&mut stat, STATFLAG_NONAME).map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; stat.cbSize as usize];
        let mut read = 0u32;
        stream.Read(buf.as_mut_ptr() as _, buf.len() as u32, Some(&mut read)).ok().map_err(|e| e.to_string())?;
        buf.truncate(read as usize);
        Ok(buf)
    }
}

pub fn take(webview: &wry::WebView, path: PathBuf, done: impl FnOnce(Result<PathBuf, String>) + 'static) {
    let Some(stream) = (unsafe { SHCreateMemStream(None) }) else {
        return done(Err("Windows gave no memory stream".into()));
    };
    let target = stream.clone();
    let handler = CapturePreviewCompletedHandler::create(Box::new(move |result| {
        let written = result.map_err(|e| e.to_string()).and_then(|_| read_all(&target)).and_then(|png| {
            std::fs::write(&path, png).map_err(|e| e.to_string())?;
            Ok(path)
        });
        done(written);
        Ok(())
    }));
    if let Err(e) = unsafe { webview.webview().CapturePreview(COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG, &stream, &handler) } {
        eprintln!("lastbook: snapshot failed: {e}");
    }
}
