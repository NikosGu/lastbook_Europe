//! Where the app keeps its settings and data, and which library it opens: the folder named with --data (or
//! LASTBOOK_DATA) as given, else the library remembered from last time, else the app's own LastbookData, else none (and
//! the window opens on setup). Ported from the original Lastbook's app/main.py.

use crate::dist;
use crate::setup::api::path_too_deep;
use crate::setup::install::DATA_NAME;
use serde_json::json;
use std::path::{Path, PathBuf};

/// The person's home folder.
pub fn home() -> PathBuf {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

/// (settings, this computer's app data): roaming and local on Windows, one place on a Mac, XDG's folders on Linux.
pub fn app_folders() -> (PathBuf, PathBuf) {
    if cfg!(target_os = "linux") {
        let h = home();
        let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or(h.join(".config"));
        let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or(h.join(".local").join("share"));
        return (config.join("lastbook-europe"), data.join("lastbook-europe"));
    }
    if cfg!(windows) {
        let h = home();
        let roaming = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or(h.join("AppData").join("Roaming"));
        let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or(h.join("AppData").join("Local"));
        return (roaming.join(dist::NAME), local.join(dist::NAME));
    }
    let mine = home().join("Library").join("Application Support").join(dist::NAME);
    (mine.clone(), mine)
}

pub fn remembered(settings: &Path) -> Option<PathBuf> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(settings.join("settings.json")).ok()?).ok()?;
    v["library"].as_str().map(PathBuf::from)
}

pub fn remember(settings: &Path, folder: &Path) {
    let _ = std::fs::create_dir_all(settings);
    if let Err(e) = std::fs::write(settings.join("settings.json"), json!({"library": folder.to_string_lossy()}).to_string()) {
        eprintln!("could not remember the library: {e}"); // it still opens this time; it asks again next time
    }
}

fn writable(folder: &Path) -> bool {
    let p = folder.join(format!(".lastbook-write-test-{}", std::process::id()));
    let ok = std::fs::write(&p, b"").is_ok();
    let _ = std::fs::remove_file(&p);
    ok
}

/// (the library, None) if folder is LastbookData or holds it, else (None, why not) in a plain sentence.
pub fn check(folder: &Path) -> (Option<PathBuf>, Option<String>) {
    for f in [folder.to_path_buf(), folder.join(DATA_NAME)] {
        if !f.join("library-data").join("catalog.json").is_file() {
            continue;
        }
        if writable(&f) {
            return (Some(f), None);
        }
        let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        return (None, Some(format!("Lastbook Europe cannot save in “{name}”. Move the LastbookData folder somewhere you can save to, then choose it there.")));
    }
    let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    (None, Some(format!("“{name}” is not a Lastbook Europe library. Choose the folder called LastbookData, inside the library you unzipped.")))
}

/// The library remembered from last time when it cannot be reached now: its folder is not there at all, as when its disk
/// is unplugged or its network share (a NAS) is away. The app then says so, instead of starting setup as if there were none.
pub fn remembered_missing(settings: &Path) -> Option<PathBuf> {
    let r = remembered(settings)?;
    (check(&r).0.is_none() && !r.exists()).then_some(r)
}

/// The remembered selection and app-owned data only.
pub fn places(settings: &Path, data: &Path) -> Vec<PathBuf> {
    remembered(settings).into_iter().chain(std::iter::once(data.join(DATA_NAME))).collect()
}

/// The folder named by --data or LASTBOOK_DATA as given, else the first library found, else None.
pub fn data_folder(arg: Option<&Path>, settings: &Path, data: &Path) -> Option<PathBuf> {
    if let Some(a) = arg {
        return Some(a.to_path_buf());
    }
    if let Some(env) = std::env::var_os("LASTBOOK_DATA").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(env));
    }
    places(settings, data).into_iter().find_map(|f| check(&f).0)
}

pub fn libraries_here(settings: &Path, data: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for f in places(settings, data) {
        if let Some(found) = check(&f).0 {
            if !out.contains(&found) {
                out.push(found);
            }
        }
    }
    out
}

/// (folder, why or None): where setup puts LastbookData unless the person chooses. The app's own data folder; on Windows,
/// when a long user name makes it too deep for the library's longest file, <system drive>:\Lastbook Europe instead, made
/// and closed to other accounts. If even that cannot be made, the per-user folder stays and setup asks the person to choose.
pub fn default_choice(data: &Path) -> (PathBuf, Option<String>) {
    if cfg!(windows) && path_too_deep(&data.join(DATA_NAME), true) {
        let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        let short = PathBuf::from(format!("{drive}\\")).join(dist::NAME);
        if private_folder(&short) {
            let why = format!("Saves to {} because your user folder’s path is too long for the library’s longest file names.", short.display());
            return (short, Some(why));
        }
    }
    (data.to_path_buf(), None)
}

/// Makes the folder if it is not there and says whether this user can save in it. On Windows a folder it makes is closed to
/// other accounts with Windows' own icacls (this account, SYSTEM and Administrators, by SID), or not used.
pub fn private_folder(path: &Path) -> bool {
    let made = !path.exists();
    if std::fs::create_dir_all(path).is_err() {
        return false;
    }
    #[cfg(windows)]
    if made {
        use std::os::windows::process::CommandExt;
        const NO_WINDOW: u32 = 0x08000000;
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
        let sid = std::process::Command::new(Path::new(&root).join("System32").join("whoami.exe")).args(["/user", "/fo", "csv", "/nh"])
            .creation_flags(NO_WINDOW).output().ok()
            .and_then(|o| String::from_utf8_lossy(&o.stdout).split('"').find(|p| p.starts_with("S-1-")).map(str::to_string));
        let closed = sid.is_some_and(|sid| {
            std::process::Command::new(Path::new(&root).join("System32").join("icacls.exe"))
                .arg(path).args(["/inheritance:r", "/grant:r", &format!("*{sid}:(OI)(CI)F"), "*S-1-5-18:(OI)(CI)F", "*S-1-5-32-544:(OI)(CI)F"])
                .creation_flags(NO_WINDOW).status().map(|s| s.success()).unwrap_or(false)
        });
        if !closed {
            eprintln!("setup: could not close the folder to other accounts, so it is not used");
            let _ = std::fs::remove_dir(path);
            return false;
        }
    }
    let _ = made;
    writable(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_library() {
        let d = std::env::temp_dir().join(format!("lb-appdirs-{}", std::process::id()));
        let settings = d.join("settings");
        let data = d.join("data");
        std::fs::create_dir_all(&settings).unwrap();
        assert_eq!(check(&d).0, None);
        assert!(check(&d).1.unwrap().contains("not a Lastbook Europe library"));
        let lib = data.join(DATA_NAME);
        std::fs::create_dir_all(lib.join("library-data")).unwrap();
        std::fs::write(lib.join("library-data/catalog.json"), b"{}").unwrap();
        assert_eq!(check(&data).0.as_deref(), Some(lib.as_path()));
        assert_eq!(data_folder(None, &settings, &data).as_deref(), Some(lib.as_path()));
        let other = d.join("elsewhere").join(DATA_NAME);
        std::fs::create_dir_all(other.join("library-data")).unwrap();
        std::fs::write(other.join("library-data/catalog.json"), b"{}").unwrap();
        remember(&settings, &other);
        assert_eq!(data_folder(None, &settings, &data).as_deref(), Some(other.as_path()));
        assert_eq!(libraries_here(&settings, &data), vec![other.clone(), lib.clone()]);
        assert_eq!(data_folder(Some(Path::new("/given")), &settings, &data).as_deref(), Some(Path::new("/given")));
        assert_eq!(remembered_missing(&settings), None, "the remembered library is here");
        let gone = d.join("unplugged-disk").join(DATA_NAME);
        remember(&settings, &gone);
        assert_eq!(remembered_missing(&settings).as_deref(), Some(gone.as_path()));
        assert_eq!(data_folder(None, &settings, &data).as_deref(), Some(lib.as_path()), "the app's own library opens meanwhile");
        if !path_too_deep(&data.join(DATA_NAME), cfg!(windows)) {
            assert_eq!(default_choice(&data).0, data); // a deep one (Windows' long temporary folders) moves to C:\\Lastbook Europe
        }
        std::fs::remove_dir_all(&d).unwrap();
    }
}
