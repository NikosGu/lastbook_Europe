//! The map: vector tiles, terrain and crop images, 3D buildings, help pins, nearby help, resources, the map's own
//! information, and place and address search. Ported from the Python engine's server.py and engine/address_search.py.

use crate::http::{Request, Response};
use crate::paths::Paths;

pub struct Map {
    pub paths: Paths,
}

impl Map {
    pub fn new(paths: Paths) -> Map {
        Map { paths }
    }

    /// The answer to a map request (`/api/map/...`, `/api/places`), or None if the request is not the map's.
    pub fn route(&self, _req: &Request) -> Option<Response> {
        None
    }
}
