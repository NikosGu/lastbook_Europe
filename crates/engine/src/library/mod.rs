//! The library: the catalog, topics, search, shelves, videos and transcripts, PDF pages, encyclopedias (ZIM), saved web
//! pages, and the library's own files. Ported from the Python engine's server.py, engine/library.py and
//! engine/saved_pages.py.

use crate::http::{Request, Response};
use crate::paths::Paths;

pub struct Library {
    pub paths: Paths,
}

impl Library {
    pub fn new(paths: Paths) -> Library {
        Library { paths }
    }

    /// The answer to a library request (`/api/...` for the library, `/library-data/...`, `/zim/...`), or None if the
    /// request is not the library's.
    pub fn route(&self, _req: &Request) -> Option<Response> {
        None
    }
}
