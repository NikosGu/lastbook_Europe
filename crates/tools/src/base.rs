//! The zoomed-out base map around a region: Europe's main roads, towns, lakes and rivers from Natural Earth (public
//! domain), drawn from zoom 3 to zoom 9 with each feature's own `min_zoom`. From zoom 10 the region's own detail takes
//! over; outside the region the map then shows nothing but the country outlines the page draws itself.
//!
//! Natural Earth files read (1:10 million, about 15 MB of zips, from https://www.naturalearthdata.com/):
//! ne_10m_roads, ne_10m_populated_places_simple, ne_10m_lakes, ne_10m_lakes_europe, ne_10m_rivers_lake_centerlines,
//! ne_10m_rivers_europe.

use crate::detail::MapWriter;
use crate::geo::{bbox, simplify};
use crate::shp::{self, Shape};
use lastbook_engine::regions;
use std::path::Path;

/// The last zoom the base shows.
pub const MAX_Z: i64 = 9;

pub const FILES: &[&str] = &["ne_10m_roads", "ne_10m_populated_places_simple", "ne_10m_lakes", "ne_10m_lakes_europe", "ne_10m_rivers_lake_centerlines", "ne_10m_rivers_europe"];

/// Every region's boxes together, two degrees wider: the part of the world the base covers.
pub fn europe() -> [f64; 4] {
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for (_, boxes) in regions::BOXES {
        for x in boxes.iter() {
            b = [b[0].min(x[0]), b[1].min(x[1]), b[2].max(x[2]), b[3].max(x[3])];
        }
    }
    [b[0] - 2.0, b[1] - 2.0, b[2] + 2.0, b[3] + 2.0]
}

fn min_zoom(s: &Shape, floor: i64) -> i64 {
    (s.num("min_zoom").unwrap_or(7.0).round() as i64).clamp(floor, MAX_Z)
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Counts {
    pub roads: usize,
    pub places: usize,
    pub water: usize,
}

/// Adds the base to a map file being written. A file missing from `ne` is a layer left out.
pub fn write(w: &mut MapWriter, ne: &Path) -> Result<Counts, String> {
    let area = europe();
    let near = |s: &Shape| bbox(&s.parts).is_some_and(|b| b.overlaps(&area));
    let read = |name: &str| -> Result<Vec<Shape>, String> {
        let p = ne.join(format!("{name}.zip"));
        if p.is_file() {
            shp::read_zip(&p)
        } else {
            Ok(Vec::new())
        }
    };
    let db = |e: rusqlite::Error| e.to_string();
    let mut n = Counts::default();
    for s in read("ne_10m_roads")?.iter().filter(|s| near(s)) {
        let cls = match (s.get("expressway"), s.get("type")) {
            (Some("1"), _) => "motorway",
            (_, Some("Major Highway")) => "trunk",
            (_, Some("Secondary Highway")) => "primary",
            (_, Some("Ferry Route")) => continue,
            _ => "secondary",
        };
        let parts: Vec<_> = s.parts.iter().map(|p| simplify(p, 0.002)).collect();
        w.add("road", cls, s.get("name"), min_zoom(s, 4), MAX_Z, &parts).map_err(db)?;
        n.roads += 1;
    }
    for s in read("ne_10m_populated_places_simple")?.iter().filter(|s| near(s)) {
        let Some(name) = s.get("name") else { continue };
        let big = s.num("pop_max").unwrap_or(0.0) >= 500_000.0 || s.get("adm0cap") == Some("1");
        w.add("place", if big { "city" } else { "town" }, Some(name), min_zoom(s, 3), MAX_Z, &s.parts).map_err(db)?;
        n.places += 1;
    }
    for name in ["ne_10m_lakes", "ne_10m_lakes_europe"] {
        for s in read(name)?.iter().filter(|s| near(s)) {
            let parts: Vec<_> = s.parts.iter().map(|p| simplify(p, 0.001)).collect();
            w.add("water", "lake", s.get("name"), min_zoom(s, 4), MAX_Z, &parts).map_err(db)?;
            n.water += 1;
        }
    }
    for name in ["ne_10m_rivers_lake_centerlines", "ne_10m_rivers_europe"] {
        for s in read(name)?.iter().filter(|s| near(s)) {
            let parts: Vec<_> = s.parts.iter().map(|p| simplify(p, 0.001)).collect();
            w.add("waterway", "river", s.get("name"), min_zoom(s, 4), MAX_Z, &parts).map_err(db)?;
            n.water += 1;
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geo;
    use crate::shp::{write::zip, Kind};

    #[test]
    fn the_base_shows_zoomed_out_and_gives_way_to_the_detail() {
        let dir = std::env::temp_dir().join(format!("lb-tools-base-{}", std::process::id()));
        let ne = dir.join("ne");
        std::fs::create_dir_all(&ne).unwrap();
        zip(&ne.join("ne_10m_roads.zip"), Kind::Line, &[
            (vec![vec![(9.0, 47.0), (9.6, 47.2), (10.0, 47.5)]], vec![("type", "Major Highway"), ("expressway", "0"), ("min_zoom", "5"), ("name", "13")]),
            (vec![vec![(-75.0, 40.0), (-74.0, 41.0)]], vec![("type", "Major Highway"), ("expressway", "1"), ("min_zoom", "3"), ("name", "I95")]),
            (vec![vec![(9.5, 47.0), (9.6, 47.1)]], vec![("type", "Ferry Route"), ("expressway", "0"), ("min_zoom", "6"), ("name", "")]),
        ]);
        zip(&ne.join("ne_10m_populated_places_simple.zip"), Kind::Point, &[
            (vec![vec![(9.5209, 47.1410)]], vec![("name", "Vaduz"), ("pop_max", "5342"), ("adm0cap", "1"), ("min_zoom", "6.1")]),
        ]);
        let data = dir.join("data");
        std::fs::create_dir_all(data.join("derived/geo")).unwrap();
        let mut w = MapWriter::create(&data.join("derived/geo/national.sqlite")).unwrap();
        let n = write(&mut w, &ne).unwrap();
        assert_eq!((n.roads, n.places, n.water), (1, 1, 0), "America and ferries are not in it; missing files are layers left out");
        w.finish().unwrap();
        std::fs::write(data.join("derived/geo/national-status.json"), r#"{"state":"complete","features":2}"#).unwrap();
        let map = lastbook_engine::map::Map::new(lastbook_engine::paths::Paths::new(dir.clone(), Some(data)));
        let at = |z: u32| -> serde_json::Value {
            let (x, y) = geo::tile_of(z, 9.5209, 47.1410);
            serde_json::from_slice(&map.tile(z, x, y)).unwrap()
        };
        assert_eq!(at(6)["layers"]["road"][0][0], "trunk");
        assert_eq!(at(6)["layers"]["place"][0][0], "city");
        assert!(at(5)["layers"].get("place").is_none(), "a town from its own min_zoom");
        assert_eq!(at(9)["layers"]["place"][0][1], "Vaduz");
        assert!(at(10)["layers"].get("road").is_none() && at(10)["layers"].get("place").is_none(), "from zoom 10 the region's own detail draws it");
        lastbook_engine::db::close_all();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
