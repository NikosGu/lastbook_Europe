//! Reads Geofabrik's free GeoPackage (OpenStreetMap, ODbL): its `gis_osm_*` layers, in WGS 84 longitude and latitude.
//! A layer's rows come with their geometry as parts (a point, a line, or every ring of a polygon, holes included) and the
//! columns asked for, as text.

use crate::map::geo::Part;
use rusqlite::types::ValueRef;
use rusqlite::Connection;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Point,
    Line,
    Polygon,
}

#[derive(Debug, Clone)]
pub struct Row {
    pub kind: Kind,
    pub parts: Vec<Part>,
    pub cols: Vec<Option<String>>,
}

impl Row {
    pub fn col(&self, i: usize) -> Option<&str> {
        self.cols.get(i).and_then(|c| c.as_deref()).filter(|s| !s.is_empty())
    }
}

/// The GeoPackage itself: a `.gpkg`, or the first `.gpkg` inside Geofabrik's `.gpkg.zip`, unpacked into `work`.
pub fn unpacked(path: &Path, work: &Path) -> Result<PathBuf, String> {
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("gpkg")) {
        return Ok(path.to_path_buf());
    }
    let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| format!("{}: {e}", path.display()))?;
    let name = z.file_names_raw().map(|n| String::from_utf8_lossy(n).into_owned()).find(|n| n.ends_with(".gpkg")).ok_or("no .gpkg in the zip")?;
    std::fs::create_dir_all(work).map_err(|e| e.to_string())?;
    let out = work.join(Path::new(&name).file_name().ok_or("bad name")?);
    let mut src = z.by_name(&name).map_err(|e| e.to_string())?;
    let mut f = std::fs::File::create(&out).map_err(|e| e.to_string())?;
    std::io::copy(&mut src, &mut f).map_err(|e| e.to_string())?;
    Ok(out)
}

/// The date of the OpenStreetMap data, from the README in Geofabrik's zip ("as of 2026-11-16T21:22:36Z").
pub fn data_as_of(zip_path: &Path) -> Option<String> {
    let mut z = zip::ZipArchive::new(std::fs::File::open(zip_path).ok()?).ok()?;
    let mut text = String::new();
    z.by_name("README").ok()?.read_to_string(&mut text).ok()?;
    let i = text.find("as of ")? + 6;
    Some(text[i..].split_whitespace().next()?.to_string())
}

/// Every row of a layer that has a readable geometry, with the columns asked for. A layer that is not there has no rows.
/// A layer's rows, with the columns asked for (a column the layer lacks reads as none).
pub fn layer(c: &Connection, table: &str, cols: &[&str]) -> Vec<Row> {
    read(c, table, cols, None).into_iter().map(|(_, r)| r).collect()
}

/// A geometry's box from its header's envelope, when it has one: [west, south, east, north].
fn envelope(b: &[u8]) -> Option<[f64; 4]> {
    if b.len() < 40 || &b[..2] != b"GP" || (b[3] >> 1) & 7 == 0 {
        return None;
    }
    let le = b[3] & 1 == 1;
    let f = |i: usize| -> Option<f64> {
        let a: [u8; 8] = b[8 + 8 * i..16 + 8 * i].try_into().ok()?;
        Some(if le { f64::from_le_bytes(a) } else { f64::from_be_bytes(a) })
    };
    let (minx, maxx, miny, maxy) = (f(0)?, f(1)?, f(2)?, f(3)?);
    Some([minx, miny, maxx, maxy])
}

fn touches(b: &[f64; 4], boxes: &[[f64; 4]]) -> bool {
    boxes.iter().any(|x| b[0] <= x[2] && b[2] >= x[0] && b[1] <= x[3] && b[3] >= x[1])
}

/// A layer's rows with their `osm_id`, only those that touch `within` when given: a row is read through, and its
/// geometry decoded, only if its header's envelope (or, without one, the geometry) reaches into a box.
fn read(c: &Connection, table: &str, cols: &[&str], within: Option<&[[f64; 4]]>) -> Vec<(Option<String>, Row)> {
    let have: Vec<String> = c.prepare(&format!("SELECT * FROM \"{table}\" LIMIT 0")).map(|st| st.column_names().map(|s| s.to_string()).collect()).unwrap_or_default();
    if have.is_empty() {
        return Vec::new();
    }
    let col = |c: &str| if have.iter().any(|h| h == c) { format!("\"{c}\"") } else { "NULL".into() };
    let select: Vec<String> = std::iter::once(col("osm_id")).chain(cols.iter().map(|c| col(c))).collect();
    let Ok(mut st) = c.prepare(&format!("SELECT geom,{} FROM \"{table}\"", select.join(","))) else { return Vec::new() };
    let text = |v: ValueRef| match v {
        ValueRef::Text(t) => Some(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Integer(n) => Some(n.to_string()),
        ValueRef::Real(f) => Some(f.to_string()),
        _ => None,
    };
    let Ok(mut rows) = st.query([]) else { return Vec::new() };
    let mut out = Vec::new();
    while let Ok(Some(r)) = rows.next() {
        let Ok(ValueRef::Blob(g)) = r.get_ref(0) else { continue };
        if let Some(boxes) = within {
            if envelope(g).is_some_and(|e| !touches(&e, boxes)) {
                continue;
            }
        }
        let Some((kind, parts)) = geometry(g) else { continue };
        if let Some(boxes) = within {
            if !crate::map::geo::bbox(&parts).is_some_and(|b| touches(&[b.minx, b.miny, b.maxx, b.maxy], boxes)) {
                continue;
            }
        }
        let id = r.get_ref(1).ok().and_then(text);
        let cols = (0..cols.len()).map(|i| r.get_ref(i + 2).ok().and_then(text)).collect();
        out.push((id, Row { kind, parts, cols }));
    }
    out
}

/// Where the builders read layers from: one GeoPackage, or a region's [`Set`].
pub trait Layers {
    fn layer(&self, table: &str, cols: &[&str]) -> Vec<Row>;
}

impl Layers for Connection {
    fn layer(&self, table: &str, cols: &[&str]) -> Vec<Row> {
        layer(self, table, cols)
    }
}

/// A region's GeoPackages: its own extract first, read whole, then its neighbours', read only inside the region's boxes
/// (Switzerland's for Liechtenstein's bank of the Rhine). A feature two extracts hold, a road across the border, comes
/// once, from the first.
pub struct Set {
    conns: Vec<Connection>,
    boxes: Vec<[f64; 4]>,
}

impl Set {
    pub fn open(paths: &[PathBuf], boxes: &[[f64; 4]]) -> Result<Set, String> {
        let conns = paths.iter().map(|p| Connection::open_with_flags(p, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| format!("{}: {e}", p.display())))
            .collect::<Result<Vec<_>, _>>()?;
        if conns.is_empty() {
            return Err("no GeoPackage".into());
        }
        Ok(Set { conns, boxes: boxes.to_vec() })
    }

    /// The region's own extract: its administrative areas say which country an address is in.
    pub fn own(&self) -> &Connection {
        &self.conns[0]
    }
}

impl Layers for Set {
    fn layer(&self, table: &str, cols: &[&str]) -> Vec<Row> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for (i, c) in self.conns.iter().enumerate() {
            for (id, row) in read(c, table, cols, (i > 0).then_some(&self.boxes[..])) {
                if id.is_none_or(|id| seen.insert(id)) {
                    out.push(row);
                }
            }
        }
        out
    }
}

/// A GeoPackage geometry blob: the "GP" header, its envelope, then well-known binary.
pub fn geometry(b: &[u8]) -> Option<(Kind, Vec<Part>)> {
    if b.len() < 8 || &b[..2] != b"GP" {
        return None;
    }
    let flags = b[3];
    if flags & 0x10 != 0 {
        return None; // empty
    }
    let envelope = match (flags >> 1) & 7 {
        0 => 0,
        1 => 32,
        2 | 3 => 48,
        4 => 64,
        _ => return None,
    };
    let mut kind = None;
    let mut parts = Vec::new();
    wkb(b, 8 + envelope, &mut parts, &mut kind)?;
    Some((kind?, parts))
}

fn wkb(b: &[u8], o: usize, parts: &mut Vec<Part>, kind: &mut Option<Kind>) -> Option<usize> {
    let le = *b.get(o)? == 1;
    let u32_at = |o: usize| -> Option<u32> {
        let s: [u8; 4] = b.get(o..o + 4)?.try_into().ok()?;
        Some(if le { u32::from_le_bytes(s) } else { u32::from_be_bytes(s) })
    };
    let f64_at = |o: usize| -> Option<f64> {
        let s: [u8; 8] = b.get(o..o + 8)?.try_into().ok()?;
        Some(if le { f64::from_le_bytes(s) } else { f64::from_be_bytes(s) })
    };
    let raw = u32_at(o + 1)?;
    let mut o = o + 5;
    // ISO codes add 1000 for Z, 2000 for M, 3000 for both; EWKB sets high bits instead (and may carry an SRID)
    let (mut dims, mut t) = (2usize, raw & 0x0fff_ffff);
    if raw & 0x8000_0000 != 0 {
        dims += 1;
    }
    if raw & 0x4000_0000 != 0 {
        dims += 1;
    }
    if raw & 0x2000_0000 != 0 {
        o += 4;
    }
    dims += match t / 1000 {
        1 | 2 => 1,
        3 => 2,
        _ => 0,
    };
    t %= 1000;
    let points = |o: usize, n: usize| -> Option<Part> {
        let mut p = Vec::with_capacity(n.min(1 << 20));
        for i in 0..n {
            p.push((f64_at(o + i * dims * 8)?, f64_at(o + i * dims * 8 + 8)?));
        }
        Some(p)
    };
    match t {
        1 => {
            parts.push(points(o, 1)?);
            kind.get_or_insert(Kind::Point);
            Some(o + dims * 8)
        }
        2 => {
            let n = u32_at(o)? as usize;
            parts.push(points(o + 4, n)?);
            kind.get_or_insert(Kind::Line);
            Some(o + 4 + n * dims * 8)
        }
        3 => {
            let rings = u32_at(o)? as usize;
            let mut o = o + 4;
            for _ in 0..rings {
                let n = u32_at(o)? as usize;
                parts.push(points(o + 4, n)?);
                o += 4 + n * dims * 8;
            }
            kind.get_or_insert(Kind::Polygon);
            Some(o)
        }
        4..=7 => {
            let n = u32_at(o)? as usize;
            let mut o = o + 4;
            for _ in 0..n {
                o = wkb(b, o, parts, kind)?;
            }
            Some(o)
        }
        _ => None,
    }
}

#[cfg(test)]
pub mod write {
    //! A small GeoPackage for tests: the layers' tables with a geometry column, rows given as WKB through `gp()`.
    use super::*;

    /// A geometry blob: the GeoPackage header without an envelope, then little-endian WKB.
    pub fn gp(kind: Kind, parts: &[Part]) -> Vec<u8> {
        let mut b = vec![b'G', b'P', 0, 1, 0xe6, 0x10, 0, 0]; // srs 4326
        let pt = |b: &mut Vec<u8>, p: &(f64, f64)| {
            b.extend(p.0.to_le_bytes());
            b.extend(p.1.to_le_bytes());
        };
        match kind {
            Kind::Point => {
                b.push(1);
                b.extend(1u32.to_le_bytes());
                pt(&mut b, &parts[0][0]);
            }
            Kind::Line => {
                b.push(1);
                b.extend(2u32.to_le_bytes());
                b.extend((parts[0].len() as u32).to_le_bytes());
                parts[0].iter().for_each(|p| pt(&mut b, p));
            }
            Kind::Polygon => {
                b.push(1);
                b.extend(6u32.to_le_bytes()); // a multipolygon of one polygon, as Geofabrik writes them
                b.extend(1u32.to_le_bytes());
                b.push(1);
                b.extend(3u32.to_le_bytes());
                b.extend((parts.len() as u32).to_le_bytes());
                for r in parts {
                    b.extend((r.len() as u32).to_le_bytes());
                    r.iter().for_each(|p| pt(&mut b, p));
                }
            }
        }
        b
    }

    pub fn table(c: &Connection, name: &str, cols: &[&str]) {
        let extra: String = cols.iter().map(|c| format!(", \"{c}\" TEXT")).collect();
        c.execute_batch(&format!("CREATE TABLE \"{name}\"(fid INTEGER PRIMARY KEY, geom BLOB{extra});")).unwrap();
    }

    pub fn row(c: &Connection, name: &str, kind: Kind, parts: &[Part], vals: &[(&str, &str)]) {
        let cols: String = vals.iter().map(|(k, _)| format!(",\"{k}\"")).collect();
        let qs: String = vals.iter().map(|_| ",?").collect();
        let mut params: Vec<rusqlite::types::Value> = vec![rusqlite::types::Value::Blob(gp(kind, parts))];
        params.extend(vals.iter().map(|(_, v)| rusqlite::types::Value::Text(v.to_string())));
        c.execute(&format!("INSERT INTO \"{name}\"(geom{cols}) VALUES (?{qs})"), rusqlite::params_from_iter(params)).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::write::*;
    use super::*;

    #[test]
    fn reads_points_lines_and_polygons_with_holes() {
        let c = Connection::open_in_memory().unwrap();
        table(&c, "gis_osm_roads_free", &["fclass", "name"]);
        row(&c, "gis_osm_roads_free", Kind::Line, &[vec![(9.5, 47.1), (9.6, 47.2)]], &[("fclass", "primary"), ("name", "Landstrasse")]);
        table(&c, "gis_osm_water_a_free", &["fclass"]);
        let outer = vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 0.0)];
        let hole = vec![(0.2, 0.1), (0.8, 0.1), (0.8, 0.7), (0.2, 0.1)];
        row(&c, "gis_osm_water_a_free", Kind::Polygon, &[outer.clone(), hole.clone()], &[("fclass", "water")]);
        let roads = layer(&c, "gis_osm_roads_free", &["fclass", "name", "ref"]);
        assert_eq!(roads.len(), 1);
        assert_eq!((roads[0].kind, roads[0].col(0), roads[0].col(1), roads[0].col(2)), (Kind::Line, Some("primary"), Some("Landstrasse"), None));
        assert_eq!(roads[0].parts, vec![vec![(9.5, 47.1), (9.6, 47.2)]]);
        let water = layer(&c, "gis_osm_water_a_free", &["fclass", "name"]);
        assert_eq!((water[0].kind, water[0].parts.len()), (Kind::Polygon, 2));
        assert_eq!(water[0].parts[1], hole);
        assert!(layer(&c, "gis_osm_nothing", &["fclass"]).is_empty());
        // a header with an envelope, and a point with Z
        let mut b = vec![b'G', b'P', 0, 0b0000_0010];
        b.extend([0u8; 4]);
        b.extend([0u8; 32]);
        b.push(1);
        b.extend(1001u32.to_le_bytes());
        for v in [9.5f64, 47.1, 450.0] {
            b.extend(v.to_le_bytes());
        }
        assert_eq!(geometry(&b), Some((Kind::Point, vec![vec![(9.5, 47.1)]])));
        assert_eq!(geometry(b"GP\0\x11\0\0\0\0"), None, "an empty geometry");
    }
}
