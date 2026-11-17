//! Vector tiles (`/api/map/tile/z/x/y`): the roads, water, land and places of one web-mercator tile, projected to its
//! 512-unit square, from map.sqlite, national.sqlite and water.sqlite, with building outlines from buildings.sqlite at
//! street zoom. The same JSON the original Lastbook sent: `{"z": z, "layers": {layer: [[cls, name, [[x, y, ...], ...]], ...]}}`.

use super::{Map, BUILDINGS, MAPDB, NATIONAL, WATER};
use std::collections::HashSet;
use std::f64::consts::PI;
use std::io::Read;
use std::sync::Arc;

/// Tile coordinate extent: a tile is 512 units square.
pub const EXT: f64 = 512.0;

/// The layers drawn as areas: rings closed and filled.
const POLYGONS: &[&str] = &["water", "wet", "land", "beach", "bldg", "ocean", "island"];

/// A map file's geometry: int32 little-endian, the number of parts, then for each part its number of points and the
/// points as lon, lat deltas in millionths of a degree (tools/build_map.py, `pack`).
pub fn unpack(blob: &[u8]) -> Vec<Vec<(f64, f64)>> {
    let a: Vec<i32> = blob.chunks_exact(4).map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    let mut parts = Vec::new();
    let Some(&count) = a.first() else { return parts };
    let mut i = 1;
    for _ in 0..count.max(0) {
        let Some(&n) = a.get(i) else { break };
        i += 1;
        let (mut x, mut y) = (0i64, 0i64);
        let mut pts = Vec::with_capacity((n.max(0) as usize).min(a.len().saturating_sub(i) / 2));
        for _ in 0..n.max(0) {
            let (Some(&dx), Some(&dy)) = (a.get(i), a.get(i + 1)) else { break };
            x += dx as i64;
            y += dy as i64;
            i += 2;
            pts.push((x as f64 / 1e6, y as f64 / 1e6));
        }
        parts.push(pts);
    }
    parts
}

/// West, south, east, north of a tile, in degrees.
pub fn tile_bounds(z: u32, x: i64, y: i64) -> (f64, f64, f64, f64) {
    let n = 2f64.powi(z as i32);
    let lat = |t: f64| (PI * (1.0 - 2.0 * t / n)).sinh().atan().to_degrees();
    (x as f64 / n * 360.0 - 180.0, lat((y + 1) as f64), (x + 1) as f64 / n * 360.0 - 180.0, lat(y as f64))
}

/// Sutherland-Hodgman clip of a polygon ring (tile units) to a box.
pub fn clip_ring(mut pts: Vec<(f64, f64)>, x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<(f64, f64)> {
    fn nz(d: f64) -> f64 {
        if d == 0.0 {
            1e-9
        } else {
            d
        }
    }
    type Inside = Box<dyn Fn((f64, f64)) -> bool>;
    type Cut = Box<dyn Fn((f64, f64), (f64, f64)) -> (f64, f64)>;
    let ix = |xv: f64| -> Cut { Box::new(move |a: (f64, f64), b: (f64, f64)| (xv, a.1 + (b.1 - a.1) * (xv - a.0) / nz(b.0 - a.0))) };
    let iy = |yv: f64| -> Cut { Box::new(move |a: (f64, f64), b: (f64, f64)| (a.0 + (b.0 - a.0) * (yv - a.1) / nz(b.1 - a.1), yv)) };
    let passes: [(Inside, Cut); 4] = [
        (Box::new(move |p: (f64, f64)| p.0 >= x0), ix(x0)),
        (Box::new(move |p: (f64, f64)| p.0 <= x1), ix(x1)),
        (Box::new(move |p: (f64, f64)| p.1 >= y0), iy(y0)),
        (Box::new(move |p: (f64, f64)| p.1 <= y1), iy(y1)),
    ];
    for (inside, cut) in passes.iter() {
        let mut out = Vec::with_capacity(pts.len());
        for i in 0..pts.len() {
            let (a, b) = (pts[(i + pts.len() - 1) % pts.len()], pts[i]);
            if inside(b) {
                if !inside(a) {
                    out.push(cut(a, b));
                }
                out.push(b);
            } else if inside(a) {
                out.push(cut(a, b));
            }
        }
        pts = out;
        if pts.is_empty() {
            break;
        }
    }
    pts
}

/// One building outline from buildings.sqlite: its box and height (decimetres), and its points in zoom-30 pixels.
pub struct Footprint {
    minx: i64,
    maxx: i64,
    miny: i64,
    maxy: i64,
    pts: Vec<(i64, i64)>,
}

/// A coordinate as the original Lastbook wrote it: rounded to one decimal (half to even on the exact value, as round() does).
fn num(v: f64) -> String {
    format!("{v:.1}")
}

fn text(v: &Option<String>) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "null".into())
}

/// Points kept when they move more than `tol` from the last kept one.
fn thin(pts: impl Iterator<Item = (f64, f64)>, tol: f64) -> Vec<f64> {
    let mut q = Vec::new();
    let mut last: Option<(f64, f64)> = None;
    for (px, py) in pts {
        if last.is_none_or(|l| (px - l.0).abs() > tol || (py - l.1).abs() > tol) {
            q.push(px);
            q.push(py);
            last = Some((px, py));
        }
    }
    q
}

fn coords(q: &[f64]) -> String {
    format!("[{}]", q.iter().map(|v| num(*v)).collect::<Vec<_>>().join(","))
}

impl Map {
    /// A vector tile's JSON. Zoomed-out tiles cover huge areas and are slow to make: once the national map is complete
    /// they are kept on disk (`derived/geo/tilecache/<version>/`), the rest in memory.
    pub fn tile(&self, z: u32, x: i64, y: i64) -> Arc<Vec<u8>> {
        let version = self.data_version();
        if z > 9 || self.national_status().get("state").and_then(|s| s.as_str()) != Some("complete") {
            return self.tile_cached(z, x, y, &version);
        }
        let Some(path) = self.path(&format!("derived/geo/tilecache/{version}/{z}/{x}/{y}.json")) else {
            return self.tile_cached(z, x, y, &version);
        };
        if let Ok(b) = std::fs::read(&path) {
            return Arc::new(b);
        }
        let data = self.tile_cached(z, x, y, &version);
        if let Some(dir) = path.parent() {
            static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let tmp = path.with_extension(format!("{}.{}.tmp", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
            if std::fs::create_dir_all(dir).is_ok() && std::fs::write(&tmp, data.as_slice()).is_ok() && std::fs::rename(&tmp, &path).is_err() {
                let _ = std::fs::remove_file(&tmp);
            }
        }
        data
    }

    fn tile_cached(&self, z: u32, x: i64, y: i64, version: &str) -> Arc<Vec<u8>> {
        let key = (z, x, y, version.to_string());
        if let Some(t) = self.tiles.lock().unwrap().get(&key) {
            return t;
        }
        let t = Arc::new(self.make_tile(z, x, y, version));
        self.tiles.lock().unwrap().put(key, t.clone());
        t
    }

    fn make_tile(&self, z: u32, x: i64, y: i64, version: &str) -> Vec<u8> {
        let (map, national) = (self.db(MAPDB), self.db(NATIONAL));
        if map.is_none() && national.is_none() {
            return br#"{"error":"map not built"}"#.to_vec();
        }
        let (lon0, lat0, lon1, lat1) = tile_bounds(z, x, y);
        let pad = (lon1 - lon0) * 0.04;
        let n = 2f64.powi(z as i32);
        let proj = |lon: f64, lat: f64| {
            let s = lat.clamp(-85.0, 85.0).to_radians().sin();
            (((lon + 180.0) / 360.0 * n - x as f64) * EXT, ((0.5 - ((1.0 + s) / (1.0 - s)).ln() / (4.0 * PI)) * n - y as f64) * EXT)
        };
        type Feature = (String, Option<String>, Option<String>, Vec<u8>); // layer, class, name, geometry
        let mut rows: Vec<Feature> = Vec::new();
        // a feature shows from its minz to its maxz: the zoomed-out base gives way to the region's detail
        let sql = "SELECT f.layer,f.cls,f.name,f.geom FROM feat_idx i JOIN feat f ON f.id=i.id \
                   WHERE i.maxx>=?1 AND i.minx<=?2 AND i.maxy>=?3 AND i.miny<=?4 AND i.minz<=?5 AND i.maxz>=?5";
        let water = self.db(WATER);
        for (c, ordered) in [(map, true), (national, true), (water, false)] {
            let Some(c) = c else { continue };
            let q = if ordered { format!("{sql} ORDER BY f.id") } else { sql.to_string() };
            let Ok(mut st) = c.prepare(&q) else { continue };
            let found = st.query_map(rusqlite::params![lon0 - pad, lon1 + pad, lat0 - pad, lat1 + pad, z], |r| {
                Ok((r.get::<_, Option<String>>(0)?.unwrap_or_default(), r.get(1)?, r.get(2)?, r.get::<_, Option<Vec<u8>>>(3)?.unwrap_or_default()))
            });
            if let Ok(found) = found {
                rows.extend(found.flatten());
            }
        }
        let tol = if z <= 6 { 2.0 } else if z <= 9 { 1.2 } else if z < 15 { 0.7 } else { 0.4 };
        let mut layers: Vec<(String, Vec<String>)> = Vec::new();
        for (layer, cls, name, geom) in rows {
            if layer == "bldg" && z < 15 {
                continue;
            }
            let polygon = POLYGONS.contains(&layer.as_str());
            let mut geo = Vec::new();
            for part in unpack(&geom) {
                let mut pts: Vec<(f64, f64)> = part.iter().map(|&(lo, la)| proj(lo, la)).collect();
                if polygon && pts.len() > 200 {
                    pts = clip_ring(pts, -16.0, -16.0, EXT + 16.0, EXT + 16.0);
                }
                let mut q = thin(pts.iter().copied(), tol);
                if polygon && q.len() < 6 {
                    continue;
                }
                if !polygon && layer != "place" && q.len() < 4 && z < 12 {
                    continue; // a line under a pixel at this zoom
                }
                if layer == "place" {
                    let Some(p) = pts.first() else { continue };
                    q = vec![p.0, p.1];
                }
                if !q.is_empty() {
                    geo.push(coords(&q));
                }
            }
            if !geo.is_empty() {
                add(&mut layers, &layer, format!("[{},{},[{}]]", text(&cls), text(&name), geo.join(",")));
            }
        }
        if (15..30).contains(&z) {
            self.buildings(&mut layers, z, x, y, tol, version);
        }
        let body: Vec<String> = layers.iter().map(|(l, f)| format!("{}:[{}]", serde_json::to_string(l).unwrap_or_default(), f.join(","))).collect();
        format!("{{\"z\":{z},\"layers\":{{{}}}}}", body.join(",")).into_bytes()
    }

    /// Building outlines for a tile at zoom 15 or more, from the zoom-14 buckets they are filed under.
    fn buildings(&self, layers: &mut Vec<(String, Vec<String>)>, z: u32, x: i64, y: i64, tol: f64, version: &str) {
        if !self.exists(BUILDINGS) {
            return;
        }
        let span: i64 = 1 << (30 - z);
        let pad = span / 25;
        let (x0, y0) = (x * span - pad, y * span - pad);
        let (x1, y1) = (x0 + span + 2 * pad, y0 + span + 2 * pad);
        let k = EXT / span as f64;
        let at = match layers.iter().position(|(l, _)| l == "bldg") {
            Some(i) => i,
            None => {
                layers.push(("bldg".into(), Vec::new()));
                layers.len() - 1
            }
        };
        let reach: i64 = 1 << 12; // outlines reaching further than ~150 m into another bucket are filed there too
        let mut seen = HashSet::new();
        for bx in (x0 - reach) >> 16..=(x1 + reach) >> 16 {
            for by in (y0 - reach) >> 16..=(y1 + reach) >> 16 {
                let Some(bucket) = self.bucket(bx, by, version) else { return };
                for f in bucket.iter() {
                    if f.maxx < x0 || f.minx > x1 || f.maxy < y0 || f.miny > y1 || !seen.insert(f.pts[0]) {
                        continue;
                    }
                    let q = thin(f.pts.iter().map(|&(px, py)| ((px - x * span) as f64 * k, (py - y * span) as f64 * k)), tol);
                    if q.len() >= 6 {
                        layers[at].1.push(format!("[\"building\",null,[{}]]", coords(&q)));
                    }
                }
            }
        }
    }

    /// The outlines filed under zoom-14 tile x14/y14, or None if buildings.sqlite cannot be read.
    fn bucket(&self, x14: i64, y14: i64, version: &str) -> Option<Arc<Vec<Footprint>>> {
        let key = (x14, y14, version.to_string());
        if let Some(b) = self.buckets.lock().unwrap().get(&key) {
            return Some(b);
        }
        let c = self.db(BUILDINGS)?;
        let mut st = c.prepare("SELECT data FROM bldg WHERE x=?1 AND y=?2").ok()?;
        let blobs: Vec<Vec<u8>> = st.query_map([x14, y14], |r| r.get(0)).ok()?.flatten().collect();
        let mut out = Vec::new();
        for blob in blobs {
            let a = ints(&inflate(&blob));
            let mut i = 0;
            while i + 4 <= a.len() {
                let (n, mut px, mut py) = (a[i].max(1) as usize, a[i + 2] as i64, a[i + 3] as i64);
                i += 4;
                let mut pts = vec![(px, py)];
                for _ in 1..n {
                    let (Some(&dx), Some(&dy)) = (a.get(i), a.get(i + 1)) else { break };
                    px += dx as i64;
                    py += dy as i64;
                    i += 2;
                    pts.push((px, py));
                }
                let xs = pts.iter().map(|p| p.0);
                let ys = pts.iter().map(|p| p.1);
                out.push(Footprint { minx: xs.clone().min().unwrap_or(0), maxx: xs.max().unwrap_or(0), miny: ys.clone().min().unwrap_or(0), maxy: ys.max().unwrap_or(0), pts });
            }
        }
        let out = Arc::new(out);
        self.buckets.lock().unwrap().put(key, out.clone());
        Some(out)
    }

    /// Every outline with a height centred in zoom-14 tile x14/y14, for the 3D view, as little-endian int32: the number of
    /// ints in the footprint records, the footprint records `[npts, height_dm, X0, Y0, dX1, dY1, ...]`, then the
    /// OpenStreetMap records `[npts, height_dm, min_height_dm, is_part, X0, Y0, ...]` (tools/build_osm_heights.py).
    pub fn bldg3d(&self, x14: i64, y14: i64) -> Vec<u8> {
        let Some(c) = self.db(BUILDINGS) else { return Vec::new() };
        let rows = |table: &str| -> Vec<u8> {
            let Ok(mut st) = c.prepare(&format!("SELECT data FROM {table} WHERE x=?1 AND y=?2")) else { return Vec::new() };
            let blobs: Vec<Vec<u8>> = st.query_map([x14, y14], |r| r.get(0)).map(|r| r.flatten().collect()).unwrap_or_default();
            blobs.iter().flat_map(|b| inflate(b)).collect()
        };
        let ms = rows("bldg");
        let mut out = ((ms.len() / 4) as i32).to_le_bytes().to_vec();
        out.extend(ms);
        out.extend(rows("bldg_osm"));
        out
    }
}

fn add(layers: &mut Vec<(String, Vec<String>)>, layer: &str, feature: String) {
    match layers.iter_mut().find(|(l, _)| l == layer) {
        Some((_, f)) => f.push(feature),
        None => layers.push((layer.to_string(), vec![feature])),
    }
}

/// zlib-compressed bytes, uncompressed; nothing if they are not zlib.
fn inflate(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    if flate2::read::ZlibDecoder::new(b).read_to_end(&mut out).is_err() {
        out.clear();
    }
    out.truncate(out.len() / 4 * 4);
    out
}

fn ints(b: &[u8]) -> Vec<i32> {
    b.chunks_exact(4).map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

/// Geometry packed as the map builders pack it, for tests and builders.
pub fn pack(parts: &[Vec<(f64, f64)>]) -> Vec<u8> {
    let mut a = vec![parts.len() as i32];
    for p in parts {
        a.push(p.len() as i32);
        let (mut lx, mut ly) = (0i64, 0i64);
        for &(x, y) in p {
            let (qx, qy) = ((x * 1e6).round() as i64, (y * 1e6).round() as i64);
            a.push((qx - lx) as i32);
            a.push((qy - ly) as i32);
            (lx, ly) = (qx, qy);
        }
    }
    a.iter().flat_map(|v| v.to_le_bytes()).collect()
}
