//! The map: vector tiles, terrain images, 3D buildings, help pins, nearby help, resources, the map's own
//! information, and place and address search. Ported from the original Lastbook's server.py and engine/address_search.py;
//! the answers are the ones the original Lastbook gave, so web/map.js reads them unchanged. Europe only, metric only.
//!
//! The map's files are in the data folder, under `derived/geo/` (docs/MAPS.md); addresses in `derived/geo/addresses/`
//! (docs/ADDRESSES.md). Each is read-only, opened once per thread ([`crate::db::open_ro`]), and a missing file is simply
//! a layer the map does not have.

pub mod address;
mod info;
mod pins;
mod places;
pub mod terrain;
pub mod text;
pub mod tiles;

pub use pins::{haversine, help, is_resource, resource_summary, HELP, RESOURCE_KINDS, TILE_POI};

use crate::db;
use crate::http::{Request, Response};
use crate::paths::Paths;
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

const MAPDB: &str = "derived/geo/map.sqlite";
const NATIONAL: &str = "derived/geo/national.sqlite";
const STATUS: &str = "derived/geo/national-status.json";
const WATER: &str = "derived/geo/water.sqlite";
const TERRAIN: &str = "derived/geo/terrain.sqlite";
const TERRAIN_DETAIL: &str = "derived/geo/terrain-detail.sqlite";
const BUILDINGS: &str = "derived/geo/buildings.sqlite";
const AERIAL: &str = "derived/geo/aerial.sqlite";
const AERIAL_DETAIL: &str = "derived/geo/aerial-detail.sqlite";
const AERIAL_COAST: &str = "derived/geo/aerial-coast.sqlite";
const PLACES: &str = "derived/geo/places.sqlite";
const ADDRESSES: &str = "derived/geo/addresses";

/// The files the map's pins come from: service points in map.sqlite and national.sqlite, resources in water.sqlite. A
/// pack with none of them has a map of roads, towns and water but no places of help to show.
pub(crate) const PIN_FILES: [&str; 3] = [MAPDB, NATIONAL, WATER];

/// Kinds the "Nearest" panel asks for when the request names none.
const NEAR_DEFAULT: &str = "hospital,clinic,pharmacy,fire_station,drinking_water";

/// A small cache that forgets the least recently used quarter when full.
struct Lru<K, V> {
    cap: usize,
    tick: u64,
    map: HashMap<K, (V, u64)>,
}

impl<K: Hash + Eq + Clone, V: Clone> Lru<K, V> {
    fn new(cap: usize) -> Lru<K, V> {
        Lru { cap, tick: 0, map: HashMap::new() }
    }

    fn get(&mut self, k: &K) -> Option<V> {
        self.tick += 1;
        let t = self.tick;
        self.map.get_mut(k).map(|e| {
            e.1 = t;
            e.0.clone()
        })
    }

    fn put(&mut self, k: K, v: V) {
        if self.map.len() >= self.cap {
            let mut ages: Vec<u64> = self.map.values().map(|e| e.1).collect();
            ages.sort_unstable();
            let cut = ages[ages.len() / 4];
            self.map.retain(|_, e| e.1 > cut);
        }
        self.tick += 1;
        self.map.insert(k, (v, self.tick));
    }
}

/// A tile and the data version it was made under.
type TileKey = (u32, i64, i64, String);
/// A zoom-14 building bucket and the data version.
type BucketKey = (i64, i64, String);

pub struct Map {
    pub paths: Paths,
    tiles: Mutex<Lru<TileKey, Arc<Vec<u8>>>>,
    buckets: Mutex<Lru<BucketKey, Arc<Vec<tiles::Footprint>>>>,
    pins_cache: Mutex<Lru<String, Value>>,
    pack: OnceLock<Option<Value>>,
    contents: OnceLock<Value>,
    coverage_cache: Mutex<Option<(Instant, Value)>>,
}

impl Map {
    pub fn new(paths: Paths) -> Map {
        Map {
            paths,
            tiles: Mutex::new(Lru::new(2048)),
            buckets: Mutex::new(Lru::new(192)),
            pins_cache: Mutex::new(Lru::new(256)),
            pack: OnceLock::new(),
            contents: OnceLock::new(),
            coverage_cache: Mutex::new(None),
        }
    }

    /// A file of the data folder, by its path inside it; None when there is no data folder.
    fn path(&self, rel: &str) -> Option<PathBuf> {
        self.paths.data.as_ref().map(|d| d.join(rel))
    }

    fn exists(&self, rel: &str) -> bool {
        self.path(rel).is_some_and(|p| p.is_file())
    }

    /// A map file, read-only, or None if it is not there.
    fn db(&self, rel: &str) -> Option<Rc<Connection>> {
        self.db_at(&self.path(rel)?)
    }

    fn db_at(&self, path: &Path) -> Option<Rc<Connection>> {
        db::open_ro(path)
    }

    /// The installed address indexes, one per region (`derived/geo/addresses/<REGION>.sqlite`), by name.
    pub fn address_files(&self) -> Vec<PathBuf> {
        let Some(dir) = self.path(ADDRESSES) else { return Vec::new() };
        let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "sqlite") && p.is_file()).collect())
            .unwrap_or_default();
        out.sort();
        out
    }

    /// The answer to a map request (`/api/map/...`, `/api/places`), or None if the request is not the map's.
    pub fn route(&self, req: &Request) -> Option<Response> {
        if req.method != "GET" && req.method != "HEAD" {
            return None;
        }
        if req.path == "/api/places" {
            return Some(self.places_route(req));
        }
        let rest = req.path.strip_prefix("/api/map/")?;
        let parts: Vec<&str> = rest.split('/').collect();
        let r = match parts.as_slice() {
            ["terrain", z, x, y] => match (int(z), int(x), y.strip_suffix(".png").and_then(int)) {
                (Some(z), Some(x), Some(y)) => match self.terrain_tile(z, x, y) {
                    // elevation detail may still be building: browsers must not remember those gaps
                    None => empty(if z >= 13 { "no-store" } else { "max-age=3600" }),
                    Some(b) => image("image/png", b, "max-age=86400"),
                },
                _ => Response::not_found(),
            },
            ["aerial", z, x, y] => match (int(z), int(x), int(y)) {
                (Some(z), Some(x), Some(y)) => match self.aerial_tile(z, x, y) {
                    None => empty("no-store"),
                    Some(b) => {
                        let jpeg = b.starts_with(&[0xff, 0xd8, 0xff]);
                        let age = if z >= 17 && self.aerial_coast_building() { "max-age=60" } else { "max-age=604800" };
                        image(if jpeg { "image/jpeg" } else { "image/png" }, b, age)
                    }
                },
                _ => Response::not_found(),
            },
            ["bldg3d", x, y] => match (int(x), int(y)) {
                (Some(x), Some(y)) => image("application/octet-stream", self.bldg3d(x, y), "max-age=86400"),
                _ => Response::not_found(),
            },
            ["tile", z, x, y] => match (int(z), int(x), int(y)) {
                (Some(z), Some(x), Some(y)) => {
                    if !(3..=18).contains(&z) {
                        Response::json(&json!({"z": z, "layers": {}}))
                    } else {
                        let body = self.tile(z as u32, x, y);
                        // ?v= carries the data version: that answer can be kept
                        let cache = if param(req, "v").is_some() { "max-age=86400" } else { "max-age=60" };
                        Response::new(200, "application/json", body.as_slice().to_vec()).with("Cache-Control", cache)
                    }
                }
                _ => Response::not_found(),
            },
            [which @ ("pins" | "members")] => {
                let kinds = param(req, "kinds").unwrap_or_default();
                let kinds: Vec<&str> = kinds.split(',').filter(|k| help(k).is_some()).collect();
                match (bbox(req), *which) {
                    (Some([w, s, e, n]), "pins") => match num(req, "zoom").unwrap_or(Some(10.0)) {
                        Some(zoom) => Response::json(&self.pins(w, s, e, n, zoom, &kinds)),
                        None => bad(),
                    },
                    (Some([w, s, e, n]), _) => match (num(req, "lat"), num(req, "lon")) {
                        (Some(Some(lat)), Some(Some(lon))) => Response::json(&self.members(w, s, e, n, &kinds, lat, lon, 60)),
                        _ => bad(),
                    },
                    _ => bad(),
                }
            }
            ["near"] => {
                let cls = param(req, "cls").unwrap_or_else(|| NEAR_DEFAULT.into());
                let cls: Vec<&str> = cls.split(',').filter(|k| help(k).is_some()).collect();
                let limit = param(req, "limit").map(|l| l.trim().parse::<i64>().ok()).unwrap_or(Some(5));
                match (num(req, "lat"), num(req, "lon"), limit) {
                    (Some(Some(lat)), Some(Some(lon)), Some(limit)) => Response::json(&json!(self.near(lat, lon, &cls, limit.clamp(1, 50) as usize))),
                    _ => bad(),
                }
            }
            ["info"] => Response::json(&self.map_info()),
            _ => Response::not_found(),
        };
        Some(r)
    }
}

/// A query parameter as the original Lastbook read it: the first value, and a blank one counts as absent.
fn param(req: &Request, name: &str) -> Option<String> {
    req.param(name).filter(|v| !v.is_empty())
}

/// None when absent; Some(None) when present and not a finite number.
fn num(req: &Request, name: &str) -> Option<Option<f64>> {
    param(req, name).map(|v| v.trim().parse::<f64>().ok().filter(|f| f.is_finite()))
}

fn bbox(req: &Request) -> Option<[f64; 4]> {
    let mut b = [0.0; 4];
    for (i, k) in ["west", "south", "east", "north"].iter().enumerate() {
        b[i] = num(req, k)??;
    }
    Some(b)
}

/// A tile coordinate from the path: digits only, as the original Lastbook's routes matched them.
fn int(s: &str) -> Option<i64> {
    if s.is_empty() || s.len() > 12 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

fn image(ctype: &str, body: Vec<u8>, cache: &str) -> Response {
    Response::new(200, ctype, body).with("Cache-Control", cache).with("X-Content-Type-Options", "nosniff")
}

fn empty(cache: &str) -> Response {
    Response { status: 204, headers: vec![("Cache-Control".into(), cache.into())], body: Vec::new() }
}

fn bad() -> Response {
    Response::json_status(400, &json!({"error": "bad request"}))
}

#[cfg(test)]
mod tests;
