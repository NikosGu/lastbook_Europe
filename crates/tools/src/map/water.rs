//! The water inventory, `derived/geo/water.sqlite`: lakes, reservoirs, ponds and dams as records the map pins, lists
//! and finds by name (map::pins, map::places). Their outlines are drawn from national.sqlite; this file holds the records.
//!
//!   resource(id, source_id, kind, name, lat, lon, detail, source, origin)   detail: JSON, metric (area_ha, region, country)
//!   spatial USING rtree(id, minx, maxx, miny, maxy)
//!   water_fts USING fts5(key)                                              rowid = resource id, key = place_key of its names
//!   feat, feat_idx (empty here), meta

use crate::map::admin::Admin;
use crate::map::geo;
use crate::map::gpkg::Layers;
use crate::map::osm::{Kind, Thing};
use lastbook_engine::map::text::place_key;
use lastbook_engine::regions;
use rusqlite::{params, Connection};
use serde_json::json;
use std::path::Path;

/// Below this a pond is too small to count on (0.4 ha, about an acre).
const POND_HA: f64 = 0.4;

/// Dams, from the PBF.
pub fn wanted(_: Kind, t: &[(&str, &str)]) -> bool {
    t.iter().any(|(k, v)| *k == "waterway" && *v == "dam")
}

pub struct Record {
    pub source_id: String,
    pub kind: &'static str,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub area_ha: Option<f64>,
}

/// The region's water records: reservoirs, named lakes and ponds of at least 0.4 ha from the GeoPackage, and dams.
pub fn collect(g: &impl Layers, dams: &[Thing], region: &str) -> Vec<Record> {
    let mut out = Vec::new();
    for r in g.layer("gis_osm_water_a_free", &["fclass", "name", "osm_id"]) {
        let ha = r.parts.first().map(|p| geo::ring_area_m2(p) / 10_000.0).unwrap_or(0.0);
        let name = r.col(1).unwrap_or("").to_string();
        let kind = match r.col(0) {
            Some("reservoir") if !name.is_empty() || ha >= POND_HA => "reservoir",
            Some("water") if !name.is_empty() => "lake",
            Some("water") if ha >= POND_HA => "pond",
            _ => continue,
        };
        let Some((lon, lat)) = geo::centroid(&r.parts) else { continue };
        if regions::contains(region, lat, lon) {
            out.push(Record { source_id: format!("osm-{}", r.col(2).unwrap_or("")), kind, name, lat, lon, area_ha: Some((ha * 100.0).round() / 100.0) });
        }
    }
    for d in dams.iter().filter(|d| d.tags.get("waterway") == Some("dam")) {
        let (lon, lat) = d.point;
        if regions::contains(region, lat, lon) {
            let id = format!("osm-{}{}", match d.kind { Kind::Node => "n", Kind::Way => "w", Kind::Relation => "r" }, d.id);
            out.push(Record { source_id: id, kind: "dam", name: d.tags.get("name").unwrap_or("").to_string(), lat, lon, area_ha: None });
        }
    }
    out
}

pub fn write(path: &Path, records: &[Record], admin: &Admin, region: &str, source: &str) -> rusqlite::Result<usize> {
    let _ = std::fs::remove_file(path);
    let c = Connection::open(path)?;
    c.execute_batch("PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF;
        CREATE TABLE resource(id INTEGER PRIMARY KEY, source_id TEXT, kind TEXT, name TEXT, lat REAL, lon REAL, detail TEXT, source TEXT, origin TEXT);
        CREATE INDEX resource_kind ON resource(kind);
        CREATE VIRTUAL TABLE spatial USING rtree(id, minx, maxx, miny, maxy);
        CREATE TABLE feat(id INTEGER PRIMARY KEY, layer TEXT, cls TEXT, name TEXT, minz INTEGER, geom BLOB, origin TEXT);
        CREATE VIRTUAL TABLE feat_idx USING rtree(id, minx, maxx, miny, maxy, minz, maxz);
        CREATE VIRTUAL TABLE water_fts USING fts5(key, tokenize='unicode61 remove_diacritics 0');
        CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT);
        BEGIN;")?;
    let country = regions::COUNTRIES.iter().find(|c| c.0 == regions::country(region)).map(|c| c.2).unwrap_or("");
    for (i, r) in records.iter().enumerate() {
        let id = i as i64 + 1;
        let mut d = json!({});
        if let Some(ha) = r.area_ha {
            d["area_ha"] = json!(ha);
        }
        if let Some(t) = admin.town_at(r.lon, r.lat) {
            d["region"] = json!(t);
        }
        if admin.in_country(r.lon, r.lat) && !country.is_empty() {
            d["country"] = json!(country);
        }
        c.execute("INSERT INTO resource VALUES (?1,?2,?3,?4,?5,?6,?7,'OpenStreetMap','osm')", params![id, r.source_id, r.kind, r.name, r.lat, r.lon, d.to_string()])?;
        c.execute("INSERT INTO spatial VALUES (?1,?2,?2,?3,?3)", params![id, r.lon, r.lat])?;
        if !r.name.is_empty() {
            c.execute("INSERT INTO water_fts(rowid, key) VALUES (?1,?2)", params![id, place_key(&[&r.name])])?;
        }
    }
    c.execute("INSERT INTO meta VALUES ('source', ?1)", [source])?;
    c.execute_batch("COMMIT; VACUUM;")?;
    Ok(records.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::gpkg::write::{row, table};
    use crate::map::gpkg::Kind as Geom;
    use crate::map::osm::Tags;

    #[test]
    fn lakes_reservoirs_ponds_and_dams() {
        let g = Connection::open_in_memory().unwrap();
        table(&g, "gis_osm_water_a_free", &["fclass", "name", "osm_id"]);
        let sq = |x: f64, y: f64, d: f64| vec![vec![(x, y), (x + d, y), (x + d, y + d), (x, y + d), (x, y)]];
        row(&g, "gis_osm_water_a_free", Geom::Polygon, &sq(9.51, 47.20, 0.002), &[("fclass", "water"), ("name", "Gampriner Seele"), ("osm_id", "1")]);
        row(&g, "gis_osm_water_a_free", Geom::Polygon, &sq(9.52, 47.12, 0.002), &[("fclass", "reservoir"), ("name", "Sägaweier"), ("osm_id", "2")]);
        row(&g, "gis_osm_water_a_free", Geom::Polygon, &sq(9.53, 47.12, 0.001), &[("fclass", "water"), ("osm_id", "3")]);
        row(&g, "gis_osm_water_a_free", Geom::Polygon, &sq(9.54, 47.12, 0.0002), &[("fclass", "water"), ("osm_id", "4")]);
        row(&g, "gis_osm_water_a_free", Geom::Polygon, &sq(9.55, 47.12, 0.002), &[("fclass", "riverbank"), ("osm_id", "5")]);
        let dam = Thing { kind: Kind::Way, id: 9, tags: Tags(vec![("waterway".into(), "dam".into()), ("name".into(), "Staumauer Steg".into())]), rings: Vec::new(), point: (9.56, 47.12) };
        let recs = collect(&g, &[dam], "LI");
        let kinds: Vec<(&str, &str)> = recs.iter().map(|r| (r.kind, r.name.as_str())).collect();
        assert_eq!(kinds, vec![("lake", "Gampriner Seele"), ("reservoir", "Sägaweier"), ("pond", ""), ("dam", "Staumauer Steg")], "a puddle and a riverbank are not records");
        assert!((recs[0].area_ha.unwrap() - 3.4).abs() < 0.2, "{:?}", recs[0].area_ha);
        let dir = std::env::temp_dir().join(format!("lb-tools-water-{}", std::process::id()));
        let data = dir.join("data");
        std::fs::create_dir_all(data.join("derived/geo")).unwrap();
        write(&data.join("derived/geo/water.sqlite"), &recs, &Admin::default(), "LI", "OpenStreetMap").unwrap();
        let map = lastbook_engine::map::Map::new(lastbook_engine::paths::Paths::new(dir.clone(), Some(data)));
        let w = map.water_places("Gampriner Seele", 10, None);
        assert_eq!((w[0]["name"].as_str(), w[0]["kind"].as_str()), (Some("Gampriner Seele"), Some("Lake")));
        assert_eq!(map.water_places("Saegaweier", 10, None)[0]["kind"], "Reservoir");
        let near = map.near(47.12, 9.52, &["reservoir", "pond", "dam"], 3);
        assert_eq!(near.len(), 3);
        assert!(near.iter().any(|p| p["cls"] == "reservoir" && p["detail"].as_str().unwrap().contains("ha")), "{near:?}");
        lastbook_engine::db::close_all();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
