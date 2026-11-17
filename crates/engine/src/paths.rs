//! Where things are: the program's own files (`web/`, `content/`), and the data folder that holds the library.
//!
//! The program's files sit beside the executable in a packaged app. Run from the source tree (`cargo run`), they are the
//! repository's own `web/` and `content/`. `LASTBOOK_APP_DIR` names another folder, for tests.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Paths {
    /// Holds `web/` and `content/`.
    pub app: PathBuf,
    /// The data folder (`LastbookData`): `library-data/`, `derived/`, `pack.json`. None until setup has made one.
    pub data: Option<PathBuf>,
}

impl Paths {
    pub fn new(app: PathBuf, data: Option<PathBuf>) -> Paths {
        Paths { app, data }
    }

    /// The program's files as found from the running executable, and the data folder given.
    pub fn discover(data: Option<PathBuf>) -> Paths {
        Paths::new(app_dir(), data)
    }

    pub fn web(&self) -> PathBuf {
        self.app.join("web")
    }

    pub fn content(&self) -> PathBuf {
        self.app.join("content")
    }
}

/// The folder holding `web/`: `LASTBOOK_APP_DIR`, else the first folder above the executable that has `web/index.html`
/// (the source tree, or a packaged app's resources), else the source tree this crate was built from.
pub fn app_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("LASTBOOK_APP_DIR") {
        return PathBuf::from(dir);
    }
    if let Ok(exe) = std::env::current_exe() {
        for dir in exe.ancestors().skip(1) {
            for candidate in [dir.to_path_buf(), dir.join("Resources"), dir.join("resources")] {
                if candidate.join("web").join("index.html").is_file() {
                    return candidate;
                }
            }
        }
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}
