//! Building outlines (`derived/geo/buildings.sqlite`) from the PBF, filed by the zoom-14 tile holding their middle, as the
//! engine's tiles and the 3D view read them (map::tiles, `/api/map/bldg3d`):
//!
//!   bldg(x, y, part, n, data)     every building: zlib(int32 [npts, height_dm, X0, Y0, dX1, dY1, ...]) in zoom-30
//!                                 pixels, first point whole, the rest as steps; height 0 when the map gives none. An
//!                                 outline reaching more than ~150 m into a neighbouring bucket is filed there too.
//!   bldg_osm(x, y, n, data)       buildings and building parts with a mapped height or storey count, for 3D:
//!                                 [npts, height_dm, min_height_dm, is_part, X0, Y0, dX1, dY1, ...]
//!   meta(key, value)

use crate::map::geo;
use crate::map::osm::{Kind, Tags, Thing};
use flate2::write::ZlibEncoder;
use rusqlite::{params, Connection};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

const REACH: i64 = 1 << 12;
const STOREY: f64 = 3.2;

pub fn wanted(_: Kind, t: &[(&str, &str)]) -> bool {
    t.iter().any(|(k, v)| matches!(*k, "building" | "building:part") && !v.is_empty() && *v != "no")
}

/// "45", "45 m", "12,5", "150 ft", "150'": metres, or None.
pub fn metres(v: Option<&str>) -> Option<f64> {
    let v = v?.replace(',', ".");
    let start = v.find(|c: char| c.is_ascii_digit() || c == '-')?;
    let num: String = v[start..].chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-').collect();
    let mut x: f64 = num.parse().ok()?;
    if v.contains('\'') || v.contains("ft") || v.contains("feet") {
        x *= 0.3048;
    }
    (0.0..1000.0).contains(&x).then_some(x)
}

/// A building's height and the height its part starts at, in metres, from `height` or `building:levels`.
pub fn heights(t: &Tags) -> Option<(f64, f64)> {
    let levels = metres(t.get("building:levels")).filter(|l| *l <= 200.0);
    let h = metres(t.get("height")).or(levels.map(|l| l * STOREY + if l > 0.0 { 1.5 } else { 0.0 }))?;
    if !(2.0..=600.0).contains(&h) {
        return None;
    }
    let mut min = metres(t.get("min_height")).or(metres(t.get("building:min_level")).map(|l| l * STOREY)).unwrap_or(0.0);
    if min >= h {
        min = 0.0;
    }
    Some((h, min))
}

#[derive(Default)]
struct Buckets(BTreeMap<(i64, i64), (Vec<i32>, i64)>);

impl Buckets {
    fn add(&mut self, key: (i64, i64), rec: &[i32]) {
        let e = self.0.entry(key).or_default();
        e.0.extend_from_slice(rec);
        e.1 += 1;
    }
}

fn zlib(a: &[i32]) -> Vec<u8> {
    let mut e = ZlibEncoder::new(Vec::new(), flate2::Compression::new(6));
    let raw: Vec<u8> = a.iter().flat_map(|v| v.to_le_bytes()).collect();
    let _ = e.write_all(&raw);
    e.finish().unwrap_or_default()
}

/// Writes buildings.sqlite from the buildings kept by `wanted`, those whose middle lies in the boxes. Returns the
/// number of outlines, and of those with a height.
pub fn write(path: &Path, things: &[Thing], boxes: &[[f64; 4]], source: &str) -> rusqlite::Result<(usize, usize)> {
    let mut all = Buckets::default();
    let mut tall = Buckets::default();
    let (mut n, mut measured) = (0, 0);
    for t in things {
        let (lon, lat) = t.point;
        let tags: Vec<(&str, &str)> = t.tags.0.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        if !wanted(t.kind, &tags) {
            continue; // read for another builder: a town's boundary, a school's grounds with an address, are no buildings
        }
        if !boxes.iter().any(|b| b[0] <= lon && lon <= b[2] && b[1] <= lat && lat <= b[3]) {
            continue;
        }
        let part = t.tags.get("building:part").is_some_and(|v| v != "no") && !matches!(t.tags.get("building"), Some(v) if v != "no");
        let h = heights(&t.tags);
        for ring in &t.rings {
            if ring.len() < 4 || ring[0] != ring[ring.len() - 1] {
                continue; // not closed: not an outline
            }
            let pts: Vec<(i64, i64)> = geo::open(ring).iter().map(|&(x, y)| geo::world30(x, y)).collect();
            let steps = |head: &[i32]| -> Vec<i32> {
                let mut rec = head.to_vec();
                rec.extend([pts[0].0 as i32, pts[0].1 as i32]);
                for w in pts.windows(2) {
                    rec.extend([(w[1].0 - w[0].0) as i32, (w[1].1 - w[0].1) as i32]);
                }
                rec
            };
            let (cx, cy) = (pts.iter().map(|p| p.0).sum::<i64>() / pts.len() as i64, pts.iter().map(|p| p.1).sum::<i64>() / pts.len() as i64);
            if !part {
                let hd = h.map(|h| (h.0 * 10.0).round() as i32).unwrap_or(0);
                let rec = steps(&[pts.len() as i32, hd]);
                let (x0, x1) = (pts.iter().map(|p| p.0).min().unwrap(), pts.iter().map(|p| p.0).max().unwrap());
                let (y0, y1) = (pts.iter().map(|p| p.1).min().unwrap(), pts.iter().map(|p| p.1).max().unwrap());
                let mut keys = vec![(cx >> 16, cy >> 16)];
                for i in (x0 + REACH) >> 16..=(x1 - REACH) >> 16 {
                    for j in (y0 + REACH) >> 16..=(y1 - REACH) >> 16 {
                        if !keys.contains(&(i, j)) {
                            keys.push((i, j)); // a big outline is filed in each bucket it reaches well into; readers keep one copy
                        }
                    }
                }
                for k in keys {
                    all.add(k, &rec);
                }
                n += 1;
            }
            if let Some((h, min)) = h {
                tall.add((cx >> 16, cy >> 16), &steps(&[pts.len() as i32, (h * 10.0).round() as i32, (min * 10.0).round() as i32, part as i32]));
                measured += 1;
            }
        }
    }
    let _ = std::fs::remove_file(path);
    let c = Connection::open(path)?;
    c.execute_batch("PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF;
        CREATE TABLE bldg(x INT, y INT, part INT, n INT, data BLOB, PRIMARY KEY(x,y,part)) WITHOUT ROWID;
        CREATE TABLE bldg_osm(x INT, y INT, n INT, data BLOB, PRIMARY KEY(x,y)) WITHOUT ROWID;
        CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT);
        BEGIN;")?;
    for ((x, y), (a, count)) in &all.0 {
        c.execute("INSERT INTO bldg VALUES (?1,?2,0,?3,?4)", params![x, y, count, zlib(a)])?;
    }
    for ((x, y), (a, count)) in &tall.0 {
        c.execute("INSERT INTO bldg_osm VALUES (?1,?2,?3,?4)", params![x, y, count, zlib(a)])?;
    }
    c.execute("INSERT INTO meta VALUES ('source', ?1), ('osm_heights', ?2)", params![source, measured.to_string()])?;
    c.execute_batch("COMMIT; VACUUM;")?;
    Ok((n, measured))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::osm::write::{pbf, El};

    #[test]
    fn heights_from_tags() {
        let t = |v: &[(&str, &str)]| Tags(v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect());
        assert_eq!(heights(&t(&[("height", "12 m")])), Some((12.0, 0.0)));
        assert_eq!(heights(&t(&[("building:levels", "3")])).map(|h| ((h.0 * 10.0).round(), h.1)), Some((111.0, 0.0)));
        assert_eq!(heights(&t(&[("height", "10,5"), ("min_height", "4")])), Some((10.5, 4.0)));
        assert_eq!(heights(&t(&[("height", "100 ft")])).map(|h| (h.0 * 10.0).round()), Some(305.0));
        assert_eq!(heights(&t(&[("building", "yes")])), None);
        assert_eq!(heights(&t(&[("height", "1")])), None, "a fence, not a building");
        assert_eq!(metres(Some("x")), None);
    }

    #[test]
    fn outlines_for_the_map_and_heights_for_3d() {
        let dir = std::env::temp_dir().join(format!("lb-tools-bldg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("b.osm.pbf");
        let sq = |id: i64, x: f64, y: f64| vec![El::Node(id, y, x, &[]), El::Node(id + 1, y, x + 0.0003, &[]), El::Node(id + 2, y + 0.0002, x + 0.0003, &[]), El::Node(id + 3, y + 0.0002, x, &[])];
        let mut els = Vec::new();
        els.extend(sq(10, 9.5215, 47.1395));
        els.extend(sq(20, 9.5225, 47.1405));
        els.extend(sq(30, 9.5216, 47.1396));
        els.push(El::Way(100, &[10, 11, 12, 13, 10], &[("building", "yes")]));
        els.push(El::Way(101, &[20, 21, 22, 23, 20], &[("building", "church"), ("building:levels", "4")]));
        els.push(El::Way(102, &[30, 31, 32, 33, 30], &[("building:part", "yes"), ("height", "30"), ("min_height", "10")]));
        // read, as a region is, for the other builders too: grounds with an address and a suburb's outline are no buildings
        els.extend(sq(40, 9.5230, 47.1410));
        els.push(El::Way(103, &[40, 41, 42, 43, 40], &[("amenity", "school"), ("addr:housenumber", "3"), ("addr:street", "Schulweg")]));
        els.push(El::Way(104, &[10, 21, 32, 43, 10], &[("place", "suburb"), ("name", "Mitte")]));
        std::fs::write(&src, pbf(&els)).unwrap();
        let things = crate::map::osm::read(&src, &|k, t| wanted(k, t) || crate::map::addresses::wanted(k, t) || crate::map::places::wanted(k, t)).unwrap();
        assert_eq!(things.len(), 5);
        let path = dir.join("buildings.sqlite");
        let li = lastbook_engine::regions::boxes("LI").unwrap();
        assert_eq!(write(&path, &things, li, "OpenStreetMap").unwrap(), (2, 2), "two outlines; the church and the part have heights");
        // the engine reads them, in 2D and in 3D
        let data = dir.join("data");
        std::fs::create_dir_all(data.join("derived/geo")).unwrap();
        std::fs::copy(&path, data.join("derived/geo/buildings.sqlite")).unwrap();
        std::fs::write(data.join("derived/geo/national-status.json"), r#"{"state":"complete","features":0}"#).unwrap();
        crate::map::detail::MapWriter::create(&data.join("derived/geo/national.sqlite")).unwrap().finish().unwrap();
        let map = lastbook_engine::map::Map::new(lastbook_engine::paths::Paths::new(dir.clone(), Some(data)));
        let (x, y) = geo::tile_of(16, 9.5218, 47.1398);
        let t: serde_json::Value = serde_json::from_slice(&map.tile(16, x, y)).unwrap();
        assert!(!t["layers"]["bldg"].as_array().unwrap().is_empty(), "{t}");
        let (x14, y14) = geo::tile_of(14, 9.5216, 47.1396);
        let b = map.bldg3d(x14, y14);
        let ints: Vec<i32> = b.chunks(4).map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        assert_eq!(ints[0], 2 * (4 + 6), "two footprints of four points");
        let osm = &ints[1 + ints[0] as usize..];
        assert_eq!(osm.len(), 2 * (6 + 6));
        let part = osm.chunks(12).find(|r| r[3] == 1).unwrap();
        assert_eq!((part[0], part[1], part[2]), (4, 300, 100), "30 m from 10 m, a part");
        lastbook_engine::db::close_all();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
