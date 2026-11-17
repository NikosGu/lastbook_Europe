//! SQLite for the engine's readers: the library's and the map's files are opened read-only, one connection per file and
//! thread, kept for the next request on that thread. SQLite is built in (rusqlite's bundled build, with FTS5 and R-tree).

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
        let c = Connection::open_with_flags(path, flags).ok()?;
        let _ = c.busy_timeout(std::time::Duration::from_secs(5));
        let c = Rc::new(c);
        open.insert(path.to_path_buf(), (c.clone(), modified));
        Some(c)
    })
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
        drop(c);
        close_all(); // Windows cannot delete a file that is still open
        std::fs::remove_dir_all(&d).unwrap();
    }
}
