//! The map's vector file, `derived/geo/national.sqlite`: the zoomed-out base around the region (`base.rs`) and the
//! region's own detail from Geofabrik's GeoPackage, as the engine's tiles read them (crates/engine/src/map/tiles.rs).
//!
//!   feat(id, layer, cls, name, minz, geom)       geom packed as the engine packs it (map::tiles::pack)
//!   feat_idx USING rtree(id, minx, maxx, miny, maxy, minz, maxz)
//!   poi(id, cls, name, lon, lat)                  the help points: hospitals, pharmacies, water, shelters...
//!   meta(key, value)
//!
//! A feature shows from zoom `minz` to zoom `maxz`. The layers, classes and zooms are the original Lastbook's.

use crate::map::geo::{self, area_z, bbox, simplify, Part};
use crate::map::gpkg::{self, Kind};
use lastbook_engine::map::tiles::pack;
use rusqlite::{params, Connection};
use std::path::Path;

/// The zoom a road appears from, by class.
pub fn road_z(cls: &str) -> i64 {
    match cls {
        "motorway" => 5,
        "trunk" => 6,
        "primary" => 8,
        "secondary" | "motorway_link" | "trunk_link" => 10,
        "tertiary" => 11,
        "primary_link" => 12,
        "secondary_link" | "tertiary_link" | "residential" | "unclassified" | "track" | "track_grade1" | "track_grade2" | "track_grade3"
        | "track_grade4" | "track_grade5" => 13,
        "living_street" | "pedestrian" | "busway" | "path" | "cycleway" | "bridleway" => 14,
        "steps" => 16,
        _ => 15,
    }
}

/// Land use kept, and the colour class the page draws it with.
fn land(cls: &str) -> Option<&'static str> {
    Some(match cls {
        "park" | "recreation_ground" | "cemetery" => "park",
        "forest" | "grass" | "meadow" | "scrub" | "heath" => "green",
        "allotments" | "farmland" | "orchard" | "vineyard" | "farmyard" => "farm",
        "residential" => "res",
        "commercial" | "retail" => "com",
        "industrial" | "military" | "quarry" | "landfill" => "ind",
        _ => return None,
    })
}

/// The zoom a place's name appears from.
fn place_z(cls: &str, population: i64) -> i64 {
    if matches!(cls, "city" | "town") && population > 300_000 {
        return 5;
    }
    match cls {
        "region" => 5,
        "city" => 6,
        "county" => 7,
        "town" => 9,
        "village" | "island" => 11,
        "suburb" => 12,
        "farm" => 15,
        _ => 14,
    }
}

/// Points people look for when services fail (the names shown come from the app, not from here).
pub const POI_KEEP: &[&str] = &["hospital", "clinic", "doctors", "pharmacy", "chemist", "dentist", "veterinary", "fire_station", "police", "drinking_water",
    "water_tower", "water_works", "fountain", "toilet", "shelter", "community_centre", "town_hall", "school", "library", "supermarket", "convenience",
    "doityourself", "hardware", "garden_centre", "post_office", "camp_site", "wastewater_plant", "comms_tower", "spring", "park", "courthouse", "embassy",
    "nursing_home", "kindergarten", "college", "university", "bicycle_shop", "car_repair", "fuel", "beach"];

pub struct MapWriter {
    conn: Connection,
    next: i64,
    pois: i64,
}

impl MapWriter {
    pub fn create(path: &Path) -> rusqlite::Result<MapWriter> {
        let _ = std::fs::remove_file(path);
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF;
             CREATE TABLE feat(id INTEGER PRIMARY KEY, layer TEXT, cls TEXT, name TEXT, minz INTEGER, geom BLOB);
             CREATE VIRTUAL TABLE feat_idx USING rtree(id, minx, maxx, miny, maxy, minz, maxz);
             CREATE TABLE poi(id INTEGER PRIMARY KEY, cls TEXT, name TEXT, lon REAL, lat REAL);
             CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT);
             BEGIN;",
        )?;
        Ok(MapWriter { conn, next: 0, pois: 0 })
    }

    /// A feature shown from zoom `minz` to `maxz`.
    pub fn add(&mut self, layer: &str, cls: &str, name: Option<&str>, minz: i64, maxz: i64, parts: &[Part]) -> rusqlite::Result<()> {
        let parts: Vec<Part> = parts.iter().filter(|p| !p.is_empty()).cloned().collect();
        let Some(b) = bbox(&parts) else { return Ok(()) };
        self.next += 1;
        self.conn.prepare_cached("INSERT INTO feat VALUES (?1,?2,?3,?4,?5,?6)")?.execute(params![self.next, layer, cls, name, minz, pack(&parts)])?;
        self.conn.prepare_cached("INSERT INTO feat_idx VALUES (?1,?2,?3,?4,?5,?6,?7)")?.execute(params![self.next, b.minx, b.maxx, b.miny, b.maxy, minz, maxz])?;
        Ok(())
    }

    pub fn poi(&mut self, cls: &str, name: Option<&str>, lon: f64, lat: f64) -> rusqlite::Result<()> {
        self.pois += 1;
        self.conn.prepare_cached("INSERT INTO poi VALUES (?1,?2,?3,?4,?5)")?.execute(params![self.pois, cls, name, lon, lat])?;
        Ok(())
    }

    pub fn meta(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        self.conn.execute("INSERT OR REPLACE INTO meta VALUES (?1,?2)", params![key, value])?;
        Ok(())
    }

    pub fn features(&self) -> i64 {
        self.next
    }

    /// Indexes the help points and closes the file, compacted.
    pub fn finish(self) -> rusqlite::Result<(i64, i64)> {
        self.conn.execute_batch("CREATE INDEX poi_cls ON poi(cls, lon, lat); COMMIT; VACUUM;")?;
        Ok((self.next, self.pois))
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Counts {
    pub features: i64,
    pub pois: i64,
}

/// The region's detail from the GeoPackage: water, land, waterways, rail, roads, place names and help points, each kept
/// when it overlaps one of the region's boxes.
pub fn detail(w: &mut MapWriter, g: &Connection, boxes: &[[f64; 4]]) -> rusqlite::Result<Counts> {
    let (f0, p0) = (w.next, w.pois);
    let inside = |parts: &[Part]| bbox(parts).is_some_and(|b| boxes.iter().any(|x| b.overlaps(x)));
    let thin = |parts: &[Part], tol: f64| -> Vec<Part> { parts.iter().map(|p| simplify(p, tol)).collect() };
    for r in gpkg::layer(g, "gis_osm_water_a_free", &["fclass", "name"]) {
        if !inside(&r.parts) {
            continue;
        }
        let cls = r.col(0).unwrap_or("water");
        let layer = if cls.starts_with("wetland") { "wet" } else { "water" };
        w.add(layer, cls, r.col(1), area_z(&bbox(&r.parts).unwrap(), 7), 30, &thin(&r.parts, 0.00001))?;
    }
    for r in gpkg::layer(g, "gis_osm_natural_a_free", &["fclass", "name"]) {
        if r.col(0) == Some("beach") && inside(&r.parts) {
            w.add("beach", "beach", r.col(1), area_z(&bbox(&r.parts).unwrap(), 10), 30, &r.parts)?;
        }
    }
    for r in gpkg::layer(g, "gis_osm_landuse_a_free", &["fclass", "name"]) {
        if let Some(cls) = r.col(0).and_then(land) {
            if inside(&r.parts) {
                w.add("land", cls, r.col(1), area_z(&bbox(&r.parts).unwrap(), 9), 30, &thin(&r.parts, 0.00001))?;
            }
        }
    }
    for r in gpkg::layer(g, "gis_osm_protected_areas_a_free", &["fclass"]) {
        if inside(&r.parts) {
            w.add("land", "park", None, area_z(&bbox(&r.parts).unwrap(), 8), 30, &thin(&r.parts, 0.00002))?;
        }
    }
    for r in gpkg::layer(g, "gis_osm_waterways_free", &["fclass", "name"]) {
        if inside(&r.parts) {
            let cls = r.col(0).unwrap_or("stream");
            w.add("waterway", cls, r.col(1), if cls == "river" { 10 } else { 13 }, 30, &r.parts)?;
        }
    }
    for r in gpkg::layer(g, "gis_osm_railways_free", &["fclass", "name"]) {
        if inside(&r.parts) {
            let cls = r.col(0).unwrap_or("rail");
            w.add("rail", cls, r.col(1), if cls == "rail" { 10 } else { 13 }, 30, &r.parts)?;
        }
    }
    for r in gpkg::layer(g, "gis_osm_roads_free", &["fclass", "name", "ref"]) {
        if inside(&r.parts) {
            let cls = r.col(0).unwrap_or("unknown");
            w.add("road", cls, r.col(1).or(r.col(2)), road_z(cls), 30, &r.parts)?;
        }
    }
    for r in gpkg::layer(g, "gis_osm_places_free", &["fclass", "population", "name"]) {
        let (Some(name), true) = (r.col(2), inside(&r.parts)) else { continue };
        let cls = r.col(0).unwrap_or("locality");
        let pop = r.col(1).and_then(|p| p.parse().ok()).unwrap_or(0);
        w.add("place", cls, Some(name), place_z(cls, pop), 30, &r.parts)?;
    }
    for table in ["gis_osm_pois_free", "gis_osm_pois_a_free", "gis_osm_traffic_free", "gis_osm_traffic_a_free", "gis_osm_natural_free", "gis_osm_natural_a_free"] {
        for r in gpkg::layer(g, table, &["fclass", "name"]) {
            let Some(cls) = r.col(0).filter(|c| POI_KEEP.contains(c)) else { continue };
            if table == "gis_osm_natural_free" && cls != "spring" {
                continue;
            }
            let Some(b) = bbox(&r.parts).filter(|_| inside(&r.parts)) else { continue };
            let (lon, lat) = if r.kind == Kind::Polygon { geo::centroid(&r.parts).unwrap_or(b.center()) } else { b.center() };
            w.poi(cls, r.col(1), lon, lat)?;
        }
    }
    Ok(Counts { features: w.next - f0, pois: w.pois - p0 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::gpkg::write::{row, table};

    /// A tiny GeoPackage around Vaduz, with the layers the detail reads.
    pub fn vaduz_gpkg() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        for t in ["gis_osm_roads_free", "gis_osm_places_free", "gis_osm_water_a_free", "gis_osm_landuse_a_free", "gis_osm_pois_free", "gis_osm_pois_a_free",
                  "gis_osm_traffic_free", "gis_osm_waterways_free", "gis_osm_natural_free"] {
            table(&c, t, &["fclass", "name", "ref", "population"]);
        }
        row(&c, "gis_osm_roads_free", Kind::Line, &[vec![(9.515, 47.13), (9.521, 47.1395), (9.525, 47.16)]], &[("fclass", "primary"), ("name", "Landstrasse")]);
        row(&c, "gis_osm_roads_free", Kind::Line, &[vec![(9.5200, 47.1380), (9.5225, 47.1410)]], &[("fclass", "residential"), ("name", "Städtle")]);
        row(&c, "gis_osm_roads_free", Kind::Line, &[vec![(9.50, 47.10), (9.51, 47.11)]], &[("fclass", "secondary"), ("ref", "28")]);
        row(&c, "gis_osm_roads_free", Kind::Line, &[vec![(12.0, 45.0), (12.1, 45.1)]], &[("fclass", "primary"), ("name", "Far away")]);
        row(&c, "gis_osm_places_free", Kind::Point, &[vec![(9.5209, 47.1410)]], &[("fclass", "town"), ("name", "Vaduz"), ("population", "5229")]);
        row(&c, "gis_osm_places_free", Kind::Point, &[vec![(9.53, 47.15)]], &[("fclass", "locality")]);
        row(&c, "gis_osm_water_a_free", Kind::Polygon, &[vec![(9.50, 47.12), (9.505, 47.12), (9.505, 47.125), (9.50, 47.12)]], &[("fclass", "reservoir"), ("name", "Sägaweier")]);
        row(&c, "gis_osm_landuse_a_free", Kind::Polygon, &[vec![(9.51, 47.14), (9.52, 47.14), (9.52, 47.15), (9.51, 47.14)]], &[("fclass", "forest")]);
        row(&c, "gis_osm_landuse_a_free", Kind::Polygon, &[vec![(9.51, 47.14), (9.52, 47.14), (9.52, 47.15), (9.51, 47.14)]], &[("fclass", "construction")]);
        row(&c, "gis_osm_pois_free", Kind::Point, &[vec![(9.5218, 47.1398)]], &[("fclass", "pharmacy"), ("name", "Apotheke am Städtle")]);
        row(&c, "gis_osm_pois_free", Kind::Point, &[vec![(9.5218, 47.1398)]], &[("fclass", "bench")]);
        row(&c, "gis_osm_pois_a_free", Kind::Polygon, &[vec![(9.517, 47.142), (9.519, 47.142), (9.519, 47.144), (9.517, 47.144), (9.517, 47.142)]],
            &[("fclass", "hospital"), ("name", "Landesspital")]);
        row(&c, "gis_osm_traffic_free", Kind::Point, &[vec![(9.51, 47.13)]], &[("fclass", "fuel"), ("name", "Tankstelle")]);
        row(&c, "gis_osm_natural_free", Kind::Point, &[vec![(9.55, 47.15)]], &[("fclass", "tree")]);
        row(&c, "gis_osm_natural_free", Kind::Point, &[vec![(9.56, 47.16)]], &[("fclass", "spring"), ("name", "Quelle")]);
        c
    }

    #[test]
    fn the_detail_as_the_engine_reads_it() {
        let dir = std::env::temp_dir().join(format!("lb-tools-detail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("national.sqlite");
        let mut w = MapWriter::create(&path).unwrap();
        let li = lastbook_engine::regions::boxes("LI").unwrap();
        let n = detail(&mut w, &vaduz_gpkg(), li).unwrap();
        assert_eq!((n.features, n.pois), (6, 4), "the far road, the nameless place and the unknown land use stay out; benches and trees are no help points");
        w.finish().unwrap();
        let c = Connection::open(&path).unwrap();
        let road: (String, String, String, i64) = c.query_row("SELECT layer, cls, name, minz FROM feat WHERE name='Landstrasse'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap();
        assert_eq!(road, ("road".into(), "primary".into(), "Landstrasse".into(), 8));
        assert_eq!(c.query_row("SELECT name FROM feat WHERE cls='secondary'", [], |r| r.get::<_, String>(0)).unwrap(), "28", "a road without a name shows its number");
        let hospital: (f64, f64) = c.query_row("SELECT lon, lat FROM poi WHERE cls='hospital'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert!((hospital.0 - 9.518).abs() < 1e-9 && (hospital.1 - 47.143).abs() < 1e-9, "an area's middle: {hospital:?}");
        assert_eq!(c.query_row("SELECT count(*) FROM feat_idx WHERE maxz=30", [], |r| r.get::<_, i64>(0)).unwrap(), 6);
        drop(c);
        // the engine draws it
        let data = dir.join("data");
        std::fs::create_dir_all(data.join("derived/geo")).unwrap();
        std::fs::rename(&path, data.join("derived/geo/national.sqlite")).unwrap();
        std::fs::write(data.join("derived/geo/national-status.json"), r#"{"state":"complete","features":6}"#).unwrap();
        let map = lastbook_engine::map::Map::new(lastbook_engine::paths::Paths::new(dir.clone(), Some(data)));
        let (x, y) = geo::tile_of(14, 9.5209, 47.1400);
        let t: serde_json::Value = serde_json::from_slice(&map.tile(14, x, y)).unwrap();
        assert_eq!(t["layers"]["place"][0][1], "Vaduz");
        assert!(t["layers"]["road"].as_array().unwrap().iter().any(|r| r[1] == "Städtle"));
        let near = map.near(47.1400, 9.5209, &["pharmacy", "hospital", "fuel"], 3);
        assert_eq!(near.len(), 3);
        assert_eq!(near[0]["name"], "Apotheke am Städtle");
        lastbook_engine::db::close_all();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
