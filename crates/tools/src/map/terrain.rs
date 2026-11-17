//! Elevation tiles, `derived/geo/terrain.sqlite`, from Copernicus DEM GLO-30 GeoTIFFs (one per degree, 1 arc-second): the
//! 256-pixel web-mercator tiles the map's relief, contours and 3D view read, in terrain-RGB PNG
//! (metres = -10000 + (R * 65536 + G * 256 + B) * 0.1), written with the engine's own `terrain::terrain_png`.
//!
//!   tile(z, x, y, png)    zoom 9 to 12 over the region's boxes
//!   meta(key, value)
//!
//! A tile is written only where every one of its pixels has data: half a tile would show a cliff at the edge of the DEM.
//! A pixel is the mean of the DEM over its footprint (one sample at zoom 12, up to 8 by 8 at zoom 9).
//!
//! The tiles are written a zoom-9 tile at a time, with every tile under it. Only the DEM files under that tile are read,
//! and they are kept while they fit in [`DEM_MEMORY`], the longest unused let go first: Norway's 300 files would take
//! 7 GB at once. The tiles under one zoom-9 tile are made on up to four processors.

use crate::map::geo;
use lastbook_engine::map::terrain::terrain_png;
use rusqlite::{params, Connection};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::f64::consts::PI;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tiff::decoder::{Decoder, DecodingResult, Limits};
use tiff::tags::Tag;

/// Zooms written: 12 is about 25 m a pixel in central Europe, close to the DEM's 30 m.
pub const ZOOMS: std::ops::RangeInclusive<u32> = 9..=12;
/// The DEM files held in memory at most while the tiles are written: some 40 files of 52 MB south of 50°N.
pub const DEM_MEMORY: usize = 2 << 30;
/// Processors the tiles are made on, at most.
const THREADS: usize = 4;

/// Where a GeoTIFF's pixels are: `w` by `h`, the first pixel's centre at (`west`, `north`), `dx` and `dy` degrees apart.
#[derive(Debug, Clone, Copy)]
pub struct Grid {
    pub west: f64,
    pub north: f64,
    pub dx: f64,
    pub dy: f64,
    pub w: usize,
    pub h: usize,
}

impl Grid {
    /// The box the file answers for: its outer pixels' centres, and two pixels more.
    fn reach(&self) -> [f64; 4] {
        [self.west - 2.0 * self.dx, self.north - (self.h as f64 + 1.0) * self.dy, self.west + (self.w as f64 + 1.0) * self.dx, self.north + 2.0 * self.dy]
    }
}

/// One GeoTIFF of heights in WGS 84: `w` by `h` pixels, the first pixel's centre at (`west`, `north`).
pub struct Dem {
    pub west: f64,
    pub north: f64,
    pub dx: f64,
    pub dy: f64,
    pub w: usize,
    pub h: usize,
    pub data: Vec<f32>,
}

/// A GeoTIFF opened, its heights not read yet, and where its pixels are.
fn open(path: &Path) -> Result<(Decoder<BufReader<File>>, Grid), String> {
    let e = |x: tiff::TiffError| format!("{}: {x}", path.display());
    let f = BufReader::new(File::open(path).map_err(|x| format!("{}: {x}", path.display()))?);
    let mut d = Decoder::new(f).map_err(e)?.with_limits(Limits::unlimited());
    let (w, h) = d.dimensions().map_err(e)?;
    let scale = d.get_tag_f64_vec(Tag::ModelPixelScaleTag).map_err(e)?;
    let tie = d.get_tag_f64_vec(Tag::ModelTiepointTag).map_err(e)?;
    let keys: Vec<u16> = d.find_tag_unsigned_vec(Tag::GeoKeyDirectoryTag).map_err(e)?.unwrap_or_default();
    // GTRasterTypeGeoKey (1025): 2 means the tie point is a pixel's centre, 1 (the default) its corner
    let point = keys.chunks(4).skip(1).any(|k| k.len() == 4 && k[0] == 1025 && k[1] == 0 && k[3] == 2);
    if scale.len() < 2 || tie.len() < 6 {
        return Err(format!("{}: not a GeoTIFF with a pixel scale and a tie point", path.display()));
    }
    let (dx, dy) = (scale[0], scale[1]);
    let (mut west, mut north) = (tie[3] - tie[0] * dx, tie[4] + tie[1] * dy);
    if !point {
        west += dx / 2.0;
        north -= dy / 2.0;
    }
    Ok((d, Grid { west, north, dx, dy, w: w as usize, h: h as usize }))
}

impl Dem {
    pub fn read(path: &Path) -> Result<Dem, String> {
        let (mut d, g) = open(path)?;
        let data = match d.read_image().map_err(|x| format!("{}: {x}", path.display()))? {
            DecodingResult::F32(v) => v,
            DecodingResult::I16(v) => v.into_iter().map(f32::from).collect(),
            DecodingResult::U16(v) => v.into_iter().map(f32::from).collect(),
            DecodingResult::F64(v) => v.into_iter().map(|x| x as f32).collect(),
            _ => return Err(format!("{}: heights must be numbers", path.display())),
        };
        if data.len() != g.w * g.h {
            return Err(format!("{}: one band of heights expected", path.display()));
        }
        Ok(Dem { west: g.west, north: g.north, dx: g.dx, dy: g.dy, w: g.w, h: g.h, data })
    }

    /// The height at a point, bilinear between the four pixels around it, if they are all here and valid.
    fn at(&self, lon: f64, lat: f64) -> Option<f32> {
        let fx = (lon - self.west) / self.dx;
        let fy = (self.north - lat) / self.dy;
        if fx < 0.0 || fy < 0.0 {
            return None;
        }
        let (i, j) = (fx.floor() as usize, fy.floor() as usize);
        if i + 1 >= self.w || j + 1 >= self.h {
            return None;
        }
        let (a, b) = ((fx - i as f64) as f32, (fy - j as f64) as f32);
        let p = |i: usize, j: usize| self.data[j * self.w + i];
        let q = [p(i, j), p(i + 1, j), p(i, j + 1), p(i + 1, j + 1)];
        if q.iter().any(|v| !v.is_finite() || *v < -1000.0) {
            return None; // no data
        }
        Some(q[0] * (1.0 - a) * (1.0 - b) + q[1] * a * (1.0 - b) + q[2] * (1.0 - a) * b + q[3] * a * b)
    }

    /// The nearest pixel, for a point between two files' pixels.
    fn nearest(&self, lon: f64, lat: f64) -> Option<f32> {
        let i = ((lon - self.west) / self.dx).round();
        let j = ((self.north - lat) / self.dy).round();
        if i < 0.0 || j < 0.0 || i as usize >= self.w || j as usize >= self.h {
            return None;
        }
        Some(self.data[j as usize * self.w + i as usize]).filter(|v| v.is_finite() && *v >= -1000.0)
    }
}

/// The height at a point from whichever file has it.
pub fn height(dems: &[&Dem], lon: f64, lat: f64) -> Option<f32> {
    dems.iter().find_map(|d| d.at(lon, lat)).or_else(|| dems.iter().find_map(|d| d.nearest(lon, lat)))
}

fn lat_of(y: f64, n: f64) -> f64 {
    (PI * (1.0 - 2.0 * y / n)).sinh().atan().to_degrees()
}

/// A tile's box, [west, south, east, north].
fn tile_box(z: u32, x: i64, y: i64) -> [f64; 4] {
    let n = 2f64.powi(z as i32);
    [x as f64 / n * 360.0 - 180.0, lat_of((y + 1) as f64, n), (x + 1) as f64 / n * 360.0 - 180.0, lat_of(y as f64, n)]
}

/// The DEM files a terrain is made from: where their pixels are, read at once, and their heights, read when a tile needs
/// them and let go, the longest unused first, when they hold more than `budget` bytes.
pub struct Dems {
    files: Vec<(PathBuf, Grid)>,
    held: HashMap<usize, (Arc<Dem>, u64)>,
    bytes: usize,
    budget: usize,
    clock: u64,
    /// Files read, a file let go and needed again counted again.
    pub reads: usize,
}

impl Dems {
    pub fn open(paths: &[PathBuf], budget: usize) -> Result<Dems, String> {
        let files = paths.iter().map(|p| open(p).map(|(_, g)| (p.clone(), g))).collect::<Result<Vec<_>, _>>()?;
        Ok(Dems { files, held: HashMap::new(), bytes: 0, budget, clock: 0, reads: 0 })
    }

    /// The files that reach into `area`, [west, south, east, north], in the order they were given.
    fn near(&mut self, area: [f64; 4]) -> Result<Vec<Arc<Dem>>, String> {
        self.clock += 1;
        let mut out = Vec::new();
        for (i, (path, grid)) in self.files.iter().enumerate() {
            let r = grid.reach();
            if r[0] > area[2] || r[2] < area[0] || r[1] > area[3] || r[3] < area[1] {
                continue;
            }
            if let Some(held) = self.held.get_mut(&i) {
                held.1 = self.clock;
                out.push(held.0.clone());
                continue;
            }
            let d = Arc::new(Dem::read(path)?);
            self.reads += 1;
            self.bytes += d.data.len() * 4;
            self.held.insert(i, (d.clone(), self.clock));
            out.push(d);
        }
        // let go of the longest unused, never of those just asked for
        while self.bytes > self.budget {
            let Some(oldest) = self.held.iter().filter(|(_, h)| h.1 < self.clock).min_by_key(|(_, h)| h.1).map(|(i, _)| *i) else { break };
            if let Some((d, _)) = self.held.remove(&oldest) {
                self.bytes -= d.data.len() * 4;
            }
        }
        Ok(out)
    }
}

/// The tiles' terrain-RGB PNGs, made on up to [`THREADS`] processors: None for a tile the files do not cover whole.
fn render(dems: &[&Dem], tiles: &[(u32, i64, i64)]) -> Vec<Option<Vec<u8>>> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(THREADS).min(tiles.len()).max(1);
    let next = AtomicUsize::new(0);
    let made = Mutex::new(Vec::with_capacity(tiles.len()));
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                let mut mine = Vec::new();
                while let Some(&(z, x, y)) = tiles.get(next.fetch_add(1, Ordering::Relaxed)) {
                    mine.push(((z, x, y), tile(dems, z, x, y).map(|h| terrain_png(&h))));
                }
                made.lock().unwrap_or_else(|e| e.into_inner()).extend(mine);
            });
        }
    });
    let mut made: HashMap<(u32, i64, i64), Option<Vec<u8>>> = made.into_inner().unwrap_or_else(|e| e.into_inner()).into_iter().collect();
    tiles.iter().map(|t| made.remove(t).flatten()).collect()
}

/// A tile's 256 by 256 heights, or None if any pixel has no data.
pub fn tile(dems: &[&Dem], z: u32, x: i64, y: i64) -> Option<Vec<f32>> {
    let n = 2f64.powi(z as i32);
    let k = 1usize << (12u32.saturating_sub(z)).min(3);
    let mut out = Vec::with_capacity(65536);
    for py in 0..256 {
        for px in 0..256 {
            let mut sum = 0.0f64;
            for sy in 0..k {
                for sx in 0..k {
                    let fx = x as f64 + (px as f64 + (sx as f64 + 0.5) / k as f64) / 256.0;
                    let fy = y as f64 + (py as f64 + (sy as f64 + 0.5) / k as f64) / 256.0;
                    sum += height(dems, fx / n * 360.0 - 180.0, lat_of(fy, n))? as f64;
                }
            }
            out.push((sum / (k * k) as f64) as f32);
        }
    }
    Some(out)
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Counts {
    pub tiles: usize,
    pub skipped: usize,
}

/// Writes terrain.sqlite: every tile of `zooms` that touches the boxes and that the DEM covers whole, the tiles under
/// each tile of the lowest zoom together, from the files under it.
pub fn write(path: &Path, dems: &mut Dems, boxes: &[[f64; 4]], zooms: std::ops::RangeInclusive<u32>, source: &str, licence: &str) -> Result<Counts, String> {
    let db = |e: rusqlite::Error| format!("{}: {e}", path.display());
    let _ = std::fs::remove_file(path);
    let c = Connection::open(path).map_err(db)?;
    c.execute_batch("PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF;
        CREATE TABLE tile(z INT, x INT, y INT, png BLOB, PRIMARY KEY(z,x,y)) WITHOUT ROWID;
        CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT);
        BEGIN;").map_err(db)?;
    // each zoom's tiles over the boxes, by the lowest zoom's tile that holds them, row by row from the north
    let z0 = *zooms.start();
    let mut under: BTreeMap<(i64, i64), Vec<(u32, i64, i64)>> = BTreeMap::new();
    for z in zooms {
        let mut done = HashSet::new();
        for b in boxes {
            let (x0, x1, y0, y1) = geo::tile_range(z, b);
            for x in x0..=x1 {
                for y in y0..=y1 {
                    if done.insert((x, y)) {
                        under.entry((y >> (z - z0), x >> (z - z0))).or_default().push((z, x, y));
                    }
                }
            }
        }
    }
    let mut n = Counts::default();
    {
        let mut insert = c.prepare("INSERT INTO tile VALUES (?1,?2,?3,?4)").map_err(db)?;
        for (&(y, x), tiles) in &under {
            let near = dems.near(tile_box(z0, x, y))?;
            let near: Vec<&Dem> = near.iter().map(|d| d.as_ref()).collect();
            for (&(z, x, y), png) in tiles.iter().zip(render(&near, tiles)) {
                match png {
                    Some(png) => {
                        insert.execute(params![z, x, y, png]).map_err(db)?;
                        n.tiles += 1;
                    }
                    None => n.skipped += 1,
                }
            }
        }
    }
    c.execute("INSERT INTO meta VALUES ('source', ?1), ('licence', ?2)", params![source, licence]).map_err(db)?;
    c.execute_batch("COMMIT; VACUUM;").map_err(db)?;
    Ok(n)
}

#[cfg(test)]
pub mod write {
    use super::*;
    use tiff::encoder::{colortype, TiffEncoder};

    /// A small GeoTIFF: heights rising 1 m a pixel eastward from 400 m, in WGS 84.
    pub fn geotiff(path: &Path, west: f64, north: f64, d: f64, w: u32, h: u32, point: bool) {
        let mut f = std::fs::File::create(path).unwrap();
        let mut enc = TiffEncoder::new(&mut f).unwrap();
        let mut img = enc.new_image::<colortype::Gray32Float>(w, h).unwrap();
        img.encoder().write_tag(Tag::ModelPixelScaleTag, &[d, d, 0.0][..]).unwrap();
        img.encoder().write_tag(Tag::ModelTiepointTag, &[0.0, 0.0, 0.0, west, north, 0.0][..]).unwrap();
        img.encoder().write_tag(Tag::GeoKeyDirectoryTag, &[1u16, 1, 0, 1, 1025, 0, 1, if point { 2 } else { 1 }][..]).unwrap();
        let data: Vec<f32> = (0..w * h).map(|i| 400.0 + (i % w) as f32).collect();
        img.write_data(&data).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::write::geotiff;
    use super::*;

    #[test]
    fn tiles_where_the_dem_covers_them_and_heights_the_engine_reads() {
        let dir = std::env::temp_dir().join(format!("lb-tools-terrain-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("data/derived/geo")).unwrap();
        let tif = dir.join("dem.tif");
        geotiff(&tif, 9.40, 47.30, 0.001, 300, 300, false);
        let dem = Dem::read(&tif).unwrap();
        assert_eq!((dem.w, dem.h), (300, 300));
        assert!((dem.west - 9.4005).abs() < 1e-9 && (dem.north - 47.2995).abs() < 1e-9, "a corner tie point: pixel centres half a pixel in");
        let h = height(&[&dem], 9.4505, 47.25).unwrap();
        assert!((h - 450.0).abs() < 0.01, "{h}");
        assert_eq!(height(&[&dem], 9.0, 47.25), None);
        let path = dir.join("data/derived/geo/terrain.sqlite");
        let mut dems = Dems::open(&[tif], DEM_MEMORY).unwrap();
        let n = write(&path, &mut dems, &[[9.40, 47.0, 9.70, 47.30]], 11..=12, "Test DEM", "test").unwrap();
        assert!(n.tiles >= 2 && n.skipped >= 1, "{n:?}: whole tiles written, tiles half outside skipped");
        let map = lastbook_engine::map::Map::new(lastbook_engine::paths::Paths::new(dir.clone(), Some(dir.join("data"))));
        let m = map.elevation(47.15, 9.55).unwrap();
        assert!((m - 550.0).abs() < 2.0, "the engine reads back {m} m where the DEM says 550 m");
        assert_eq!(map.map_contents()["contours"], true);
        lastbook_engine::db::close_all();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn files_read_a_tile_at_a_time_give_what_all_of_them_at_once_give() {
        let dir = std::env::temp_dir().join(format!("lb-tools-terrain-parts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // two files side by side, meeting at 9.55
        let (west, east) = (dir.join("west.tif"), dir.join("east.tif"));
        geotiff(&west, 9.40, 47.30, 0.001, 150, 300, false);
        geotiff(&east, 9.55, 47.30, 0.001, 150, 300, false);
        let both = [Dem::read(&west).unwrap(), Dem::read(&east).unwrap()];
        let boxes = [[9.40, 47.0, 9.70, 47.30]];
        let mut written = Vec::new();
        for budget in [0, DEM_MEMORY] {
            // with no memory to spare, each file is read again for every zoom-11 tile that needs it
            let mut dems = Dems::open(&[west.clone(), east.clone()], budget).unwrap();
            let path = dir.join(format!("terrain-{budget}.sqlite"));
            let n = write(&path, &mut dems, &boxes, 11..=13, "Test DEM", "test").unwrap();
            assert!(if budget == 0 { dems.reads > 2 } else { dems.reads == 2 }, "{budget}: {} reads", dems.reads);
            let c = Connection::open(&path).unwrap();
            let rows: Vec<(u32, i64, i64, Vec<u8>)> = c.prepare("SELECT z, x, y, png FROM tile ORDER BY z, x, y").unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap().map(Result::unwrap).collect();
            assert_eq!(rows.len(), n.tiles);
            // every tile over the boxes the two files cover whole, as both files at once make it, the seam included
            let mut expected = 0;
            for z in 11..=13 {
                let (x0, x1, y0, y1) = geo::tile_range(z, &boxes[0]);
                for x in x0..=x1 {
                    for y in y0..=y1 {
                        let Some(h) = tile(&[&both[0], &both[1]], z, x, y) else { continue };
                        expected += 1;
                        let png = rows.iter().find(|r| (r.0, r.1, r.2) == (z, x, y)).map(|r| &r.3);
                        assert_eq!(png, Some(&terrain_png(&h)), "{z}/{x}/{y}");
                    }
                }
            }
            assert_eq!(n.tiles, expected);
            assert!(rows.iter().any(|r| { let b = tile_box(r.0, r.1, r.2); b[0] < 9.55 && b[2] > 9.55 }), "a tile across the seam");
            written.push(std::fs::read(&path).unwrap());
        }
        assert!(written[0] == written[1], "the same file, whatever the memory");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
