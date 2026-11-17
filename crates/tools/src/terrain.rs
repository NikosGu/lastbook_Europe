//! Elevation tiles, `derived/geo/terrain.sqlite`, from Copernicus DEM GLO-30 GeoTIFFs (one per degree, 1 arc-second): the
//! 256-pixel web-mercator tiles the map's relief, contours and 3D view read, in terrain-RGB PNG
//! (metres = -10000 + (R * 65536 + G * 256 + B) * 0.1), written with the engine's own `terrain::terrain_png`.
//!
//!   tile(z, x, y, png)    zoom 9 to 12 over the region's boxes
//!   meta(key, value)
//!
//! A tile is written only where every one of its pixels has data: half a tile would show a cliff at the edge of the DEM.
//! A pixel is the mean of the DEM over its footprint (one sample at zoom 12, up to 8 by 8 at zoom 9).

use crate::geo;
use lastbook_engine::map::terrain::terrain_png;
use rusqlite::{params, Connection};
use std::f64::consts::PI;
use std::path::Path;
use tiff::decoder::{Decoder, DecodingResult, Limits};
use tiff::tags::Tag;

/// Zooms written: 12 is about 25 m a pixel in central Europe, close to the DEM's 30 m.
pub const ZOOMS: std::ops::RangeInclusive<u32> = 9..=12;

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

impl Dem {
    pub fn read(path: &Path) -> Result<Dem, String> {
        let e = |x: tiff::TiffError| format!("{}: {x}", path.display());
        let f = std::io::BufReader::new(std::fs::File::open(path).map_err(|x| format!("{}: {x}", path.display()))?);
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
        let data = match d.read_image().map_err(e)? {
            DecodingResult::F32(v) => v,
            DecodingResult::I16(v) => v.into_iter().map(f32::from).collect(),
            DecodingResult::U16(v) => v.into_iter().map(f32::from).collect(),
            DecodingResult::F64(v) => v.into_iter().map(|x| x as f32).collect(),
            _ => return Err(format!("{}: heights must be numbers", path.display())),
        };
        if data.len() != w as usize * h as usize {
            return Err(format!("{}: one band of heights expected", path.display()));
        }
        Ok(Dem { west, north, dx, dy, w: w as usize, h: h as usize, data })
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
pub fn height(dems: &[Dem], lon: f64, lat: f64) -> Option<f32> {
    dems.iter().find_map(|d| d.at(lon, lat)).or_else(|| dems.iter().find_map(|d| d.nearest(lon, lat)))
}

fn lat_of(y: f64, n: f64) -> f64 {
    (PI * (1.0 - 2.0 * y / n)).sinh().atan().to_degrees()
}

/// A tile's 256 by 256 heights, or None if any pixel has no data.
pub fn tile(dems: &[Dem], z: u32, x: i64, y: i64) -> Option<Vec<f32>> {
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

/// Writes terrain.sqlite: every tile of `zooms` that touches the boxes and that the DEM covers whole.
pub fn write(path: &Path, dems: &[Dem], boxes: &[[f64; 4]], zooms: std::ops::RangeInclusive<u32>, source: &str, licence: &str) -> rusqlite::Result<Counts> {
    let _ = std::fs::remove_file(path);
    let c = Connection::open(path)?;
    c.execute_batch("PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF;
        CREATE TABLE tile(z INT, x INT, y INT, png BLOB, PRIMARY KEY(z,x,y)) WITHOUT ROWID;
        CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT);")?;
    let mut n = Counts::default();
    for z in zooms {
        let mut done = std::collections::HashSet::new();
        for b in boxes {
            let (x0, x1, y0, y1) = geo::tile_range(z, b);
            for x in x0..=x1 {
                for y in y0..=y1 {
                    if !done.insert((x, y)) {
                        continue;
                    }
                    match tile(dems, z, x, y) {
                        Some(h) => {
                            c.execute("INSERT INTO tile VALUES (?1,?2,?3,?4)", params![z, x, y, terrain_png(&h)])?;
                            n.tiles += 1;
                        }
                        None => n.skipped += 1,
                    }
                }
            }
        }
    }
    c.execute("INSERT INTO meta VALUES ('source', ?1), ('licence', ?2)", params![source, licence])?;
    c.execute_batch("VACUUM;")?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiff::encoder::{colortype, TiffEncoder};

    /// A small GeoTIFF: heights rising 1 m a pixel eastward from 400 m, in WGS 84.
    fn geotiff(path: &Path, west: f64, north: f64, d: f64, w: u32, h: u32, point: bool) {
        let mut f = std::fs::File::create(path).unwrap();
        let mut enc = TiffEncoder::new(&mut f).unwrap();
        let mut img = enc.new_image::<colortype::Gray32Float>(w, h).unwrap();
        img.encoder().write_tag(Tag::ModelPixelScaleTag, &[d, d, 0.0][..]).unwrap();
        img.encoder().write_tag(Tag::ModelTiepointTag, &[0.0, 0.0, 0.0, west, north, 0.0][..]).unwrap();
        img.encoder().write_tag(Tag::GeoKeyDirectoryTag, &[1u16, 1, 0, 1, 1025, 0, 1, if point { 2 } else { 1 }][..]).unwrap();
        let data: Vec<f32> = (0..w * h).map(|i| 400.0 + (i % w) as f32).collect();
        img.write_data(&data).unwrap();
    }

    #[test]
    fn tiles_where_the_dem_covers_them_and_heights_the_engine_reads() {
        let dir = std::env::temp_dir().join(format!("lb-tools-terrain-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("data/derived/geo")).unwrap();
        let tif = dir.join("dem.tif");
        geotiff(&tif, 9.40, 47.30, 0.001, 300, 300, false);
        let dem = Dem::read(&tif).unwrap();
        assert_eq!((dem.w, dem.h), (300, 300));
        assert!((dem.west - 9.4005).abs() < 1e-9 && (dem.north - 47.2995).abs() < 1e-9, "a corner tie point: pixel centres half a pixel in");
        let h = height(std::slice::from_ref(&dem), 9.4505, 47.25).unwrap();
        assert!((h - 450.0).abs() < 0.01, "{h}");
        assert_eq!(height(std::slice::from_ref(&dem), 9.0, 47.25), None);
        let path = dir.join("data/derived/geo/terrain.sqlite");
        let n = write(&path, std::slice::from_ref(&dem), &[[9.40, 47.0, 9.70, 47.30]], 11..=12, "Test DEM", "test").unwrap();
        assert!(n.tiles >= 2 && n.skipped >= 1, "{n:?}: whole tiles written, tiles half outside skipped");
        let map = lastbook_engine::map::Map::new(lastbook_engine::paths::Paths::new(dir.clone(), Some(dir.join("data"))));
        let m = map.elevation(47.15, 9.55).unwrap();
        assert!((m - 550.0).abs() < 2.0, "the engine reads back {m} m where the DEM says 550 m");
        assert_eq!(map.map_contents()["contours"], true);
        lastbook_engine::db::close_all();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
