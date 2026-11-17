//! SQLite for the engine's readers: the library's and the map's files are opened read-only, one connection per file and
//! thread, kept for the next request on that thread. SQLite is built in (rusqlite's bundled build, with FTS5 and R-tree).
//!
//! The files are opened immutable: SQLite then takes no locks and reads no journal. A library can live on a network share
//! (a NAS through SMB or NFS), where file locks are unreliable, and these files never change while the app runs (setup
//! replaces them only when the app starts again).

use rusqlite::{Connection, OpenFlags};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

thread_local! {
    static OPEN: RefCell<HashMap<PathBuf, (Rc<Connection>, Option<std::time::SystemTime>)>> = RefCell::new(HashMap::new());
}

/// A read-only connection to the file, or None if it is not there or not SQLite. A file replaced since (its modified
/// time changed) is opened again.
pub fn open_ro(path: &Path) -> Option<Rc<Connection>> {
    let modified = std::fs::metadata(path).ok().filter(|m| m.is_file())?.modified().ok();
    OPEN.with(|open| {
        let mut open = open.borrow_mut();
        if let Some((c, m)) = open.get(path) {
            if *m == modified {
                return Some(c.clone());
            }
        }
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_URI;
        let c = Connection::open_with_flags(uri(path), flags).ok()?;
        let _ = c.busy_timeout(std::time::Duration::from_secs(5));
        let c = Rc::new(c);
        open.insert(path.to_path_buf(), (c.clone(), modified));
        Some(c)
    })
}

/// An SQLite file: URI for the path, read-only and immutable. Unix paths, Windows drives (C:\...) and network paths
/// (\\server\share\...) all become file: URIs; characters SQLite reads in a URI are escaped.
pub fn uri(path: &Path) -> String {
    let s = path.to_string_lossy();
    let s = s.strip_prefix(r"\\?\UNC\").map(|r| format!(r"\\{r}")).unwrap_or_else(|| s.trim_start_matches(r"\\?\").to_string());
    // a Windows path (a drive, or \\server\share) takes forward slashes; elsewhere a backslash is part of a name
    let windows = s.starts_with(r"\\") || (s.as_bytes().get(1) == Some(&b':') && s.as_bytes()[0].is_ascii_alphabetic());
    let s = if windows { s.replace('\\', "/") } else { s };
    let mut esc = String::with_capacity(s.len() + 16);
    for ch in s.chars() {
        match ch {
            '%' => esc.push_str("%25"),
            '?' => esc.push_str("%3f"),
            '#' => esc.push_str("%23"),
            ' ' => esc.push_str("%20"),
            c => esc.push(c),
        }
    }
    let prefix = if esc.starts_with("//") {
        "file://" // a network path: file:////server/share/...
    } else if esc.as_bytes().get(1) == Some(&b':') {
        "file:///" // a Windows drive: file:///C:/...
    } else {
        "file://"
    };
    format!("{prefix}{esc}?mode=ro&immutable=1")
}

/// Forget this thread's connections (after a file was swapped).
pub fn close_all() {
    OPEN.with(|open| open.borrow_mut().clear());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_read_only_with_fts5_and_rtree() {
        let d = std::env::temp_dir().join(format!("lb-db-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("t.sqlite");
        let _ = std::fs::remove_file(&f);
        {
            let c = Connection::open(&f).unwrap();
            c.execute_batch("CREATE VIRTUAL TABLE t USING fts5(body); INSERT INTO t VALUES ('acqua potabile');
                             CREATE VIRTUAL TABLE r USING rtree(id, x0, x1, y0, y1); INSERT INTO r VALUES (1, 9.4, 9.6, 47.0, 47.3);").unwrap();
        }
        let c = open_ro(&f).unwrap();
        let n: i64 = c.query_row("SELECT count(*) FROM t WHERE t MATCH 'acqua'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        let n: i64 = c.query_row("SELECT count(*) FROM r WHERE x0 <= 9.5 AND x1 >= 9.5", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        assert!(c.execute("INSERT INTO t VALUES ('x')", []).is_err(), "read-only");
        assert!(open_ro(&d.join("missing.sqlite")).is_none());
        // a name SQLite would read as part of a URI still opens
        let odd = d.join("a #1? 100%.sqlite");
        std::fs::copy(&f, &odd).unwrap();
        assert!(open_ro(&odd).is_some(), "{}", uri(&odd));
        assert_eq!(uri(Path::new(r"C:\Lastbook Europe\geo.sqlite")), "file:///C:/Lastbook%20Europe/geo.sqlite?mode=ro&immutable=1");
        assert_eq!(uri(Path::new(r"\\nas\share\geo.sqlite")), "file:////nas/share/geo.sqlite?mode=ro&immutable=1");
        assert_eq!(uri(Path::new(r"\\?\UNC\nas\share\geo.sqlite")), "file:////nas/share/geo.sqlite?mode=ro&immutable=1");
        assert_eq!(uri(Path::new("/Volumes/NAS/geo.sqlite")), "file:///Volumes/NAS/geo.sqlite?mode=ro&immutable=1");
        assert_eq!(uri(Path::new(r"/home/a\b.sqlite")), r"file:///home/a\b.sqlite?mode=ro&immutable=1", "a backslash in a Unix name stays");
        drop(c);
        close_all(); // Windows cannot delete a file that is still open
        std::fs::remove_dir_all(&d).unwrap();
    }
}
