//! Image tiles: elevation (terrain-RGB PNG, for relief, contours and the 3D view) and the optional aerial photos (JPEG or
//! PNG), each read from its map file as built. A missing file or tile is None, and the route answers 204. Elevation is
//! also read here, in metres, from the same tiles (the `png` crate).

use super::{Map, AERIAL, AERIAL_COAST, AERIAL_DETAIL, TERRAIN, TERRAIN_DETAIL};
use std::f64::consts::PI;

/// Zooms of the elevation tiles: 4 to 12 from terrain.sqlite, 13 to 17 from terrain-detail.sqlite where it has them.
pub const TERRAIN_ZOOMS: (u32, u32, u32) = (4, 12, 17);
/// Zooms a tile of terrain.sqlite is enlarged across, where it has none more detailed (zoom 10 serves 11 and 12).
const ENLARGED: i64 = 3;

/// The part of a tile's heights under one of its tiles `up` zooms below (at `dx`, `dy` among them), enlarged to 256
/// by 256: bilinear between the pixels' centres, as the page reads heights.
fn enlarged(h: &[f32], up: u32, dx: i64, dy: i64) -> Vec<f32> {
    let k = (1u32 << up) as f64;
    let mut out = Vec::with_capacity(65536);
    for py in 0..256 {
        for px in 0..256 {
            let fx = ((dx as f64 * 256.0 + px as f64 + 0.5) / k - 0.5).clamp(0.0, 254.999);
            let fy = ((dy as f64 * 256.0 + py as f64 + 0.5) / k - 0.5).clamp(0.0, 254.999);
            let (i, j) = (fx.floor() as usize, fy.floor() as usize);
            let (a, b) = ((fx - i as f64) as f32, (fy - j as f64) as f32);
            let at = |i: usize, j: usize| h[j * 256 + i];
            out.push(at(i, j) * (1.0 - a) * (1.0 - b) + at(i + 1, j) * a * (1.0 - b) + at(i, j + 1) * (1.0 - a) * b + at(i + 1, j + 1) * a * b);
        }
    }
    out
}

impl Map {
    fn tile_blob(&self, file: &str, column: &str, z: i64, x: i64, y: i64) -> Option<Vec<u8>> {
        let c = self.db(file)?;
        c.query_row(&format!("SELECT {column} FROM tile WHERE z=?1 AND x=?2 AND y=?3"), [z, x, y], |r| r.get(0)).ok()
    }

    /// An elevation tile (terrain-RGB PNG), or None for open sea or outside the data. Where terrain.sqlite stops below
    /// zoom 12 (Norway's, Sweden's and Finland's stop at 10, to keep their maps small enough to publish), the tile is
    /// the part of the most detailed tile above it, enlarged: the page draws no relief or contours for a missing tile.
    pub fn terrain_tile(&self, z: i64, x: i64, y: i64) -> Option<Vec<u8>> {
        if z >= 13 {
            return self.tile_blob(TERRAIN_DETAIL, "png", z, x, y);
        }
        if let Some(png) = self.tile_blob(TERRAIN, "png", z, x, y) {
            return Some(png);
        }
        (1..=ENLARGED).filter(|up| z - up >= TERRAIN_ZOOMS.0 as i64).find_map(|up| {
            let above = heights(&self.tile_blob(TERRAIN, "png", z - up, x >> up, y >> up)?)?;
            Some(terrain_png(&enlarged(&above, up as u32, x & ((1 << up) - 1), y & ((1 << up) - 1))))
        })
    }

    /// An aerial photo tile, or None: an optional layer, from aerial.sqlite (zoom 0-16) and, from zoom 17, the sharper
    /// files where they exist. No European map has these files yet.
    pub fn aerial_tile(&self, z: i64, x: i64, y: i64) -> Option<Vec<u8>> {
        let files: &[&str] = if z >= 17 { &[AERIAL_COAST, AERIAL_DETAIL] } else { &[AERIAL] };
        files.iter().find_map(|f| self.tile_blob(f, "jpg", z, x, y))
    }

    /// Is aerial-coast.sqlite still being written (no `built` stamp yet)? Its tiles may change, so they are kept briefly.
    pub fn aerial_coast_building(&self) -> bool {
        self.db(AERIAL_COAST).is_some_and(|c| c.query_row("SELECT 1 FROM meta WHERE key='built'", [], |_| Ok(())).is_err_and(|e| matches!(e, rusqlite::Error::QueryReturnedNoRows)))
    }

    pub fn has_aerial(&self) -> bool {
        [AERIAL, AERIAL_DETAIL, AERIAL_COAST].iter().any(|f| self.exists(f))
    }

    /// Height above sea level in metres at a point, from the most detailed elevation tile there, or None where there is
    /// none. Read as the map page reads it: bilinear between the four nearest pixels.
    pub fn elevation(&self, lat: f64, lon: f64) -> Option<f64> {
        if !(lat.is_finite() && lon.is_finite()) || lat.abs() > 85.0 {
            return None;
        }
        let (low, mid, high) = TERRAIN_ZOOMS;
        let top = if self.exists(TERRAIN_DETAIL) { high } else { mid };
        for z in (low..=top).rev() {
            let n = 2f64.powi(z as i32);
            let fx = (lon + 180.0) / 360.0 * n;
            let s = lat.to_radians().sin();
            let fy = (0.5 - ((1.0 + s) / (1.0 - s)).ln() / (4.0 * PI)) * n;
            let (x, y) = (fx.floor(), fy.floor());
            let Some(h) = self.terrain_tile(z as i64, x as i64, y as i64).and_then(|p| heights(&p)) else { continue };
            let px = ((fx - x) * 256.0 - 0.5).clamp(0.0, 254.999);
            let py = ((fy - y) * 256.0 - 0.5).clamp(0.0, 254.999);
            let (i, j) = (px.floor() as usize, py.floor() as usize);
            let (a, b) = (px - i as f64, py - j as f64);
            let at = |i: usize, j: usize| h[j * 256 + i] as f64;
            return Some(at(i, j) * (1.0 - a) * (1.0 - b) + at(i + 1, j) * a * (1.0 - b) + at(i, j + 1) * (1.0 - a) * b + at(i + 1, j + 1) * a * b);
        }
        None
    }
}

/// The heights in a terrain-RGB tile, row by row: metres = -10000 + (R * 65536 + G * 256 + B) * 0.1. None unless it is a
/// 256 by 256 PNG.
pub fn heights(png_bytes: &[u8]) -> Option<Vec<f32>> {
    let mut d = png::Decoder::new(std::io::Cursor::new(png_bytes));
    d.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = d.read_info().ok()?;
    let mut buf = vec![0; r.output_buffer_size()?];
    let info = r.next_frame(&mut buf).ok()?;
    if info.width != 256 || info.height != 256 {
        return None;
    }
    let step = info.color_type.samples();
    if step < 3 {
        return None;
    }
    let data = &buf[..info.line_size * 256];
    let mut out = Vec::with_capacity(65536);
    for row in data.chunks(info.line_size) {
        for p in row.chunks(step).take(256) {
            out.push((-10000.0 + (p[0] as f64 * 65536.0 + p[1] as f64 * 256.0 + p[2] as f64) * 0.1) as f32);
        }
    }
    Some(out)
}

/// A terrain-RGB tile from 256 by 256 heights in metres (NaN as sea level), as tools/build_terrain.py writes them: RGB,
/// filter Up. For builders and tests.
pub fn terrain_png(h: &[f32]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(256 * 256 * 3);
    for &m in h.iter().take(65536) {
        let m = if m.is_nan() { 0.0 } else { m as f64 };
        let v = ((m + 10000.0) * 10.0).round().clamp(0.0, 16_777_215.0) as u32;
        rgb.extend_from_slice(&[(v >> 16) as u8, (v >> 8) as u8, v as u8]);
    }
    rgb.resize(256 * 256 * 3, 0);
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, 256, 256);
        e.set_color(png::ColorType::Rgb);
        e.set_depth(png::BitDepth::Eight);
        e.set_filter(png::Filter::Up);
        let Ok(mut w) = e.write_header() else { return Vec::new() };
        if w.write_image_data(&rgb).is_err() || w.finish().is_err() {
            return Vec::new();
        }
    }
    out
}
