//! A region's map set and address index, from its sources, in the layout setup installs:
//!
//!   <out>/map/map-set.json                     {name, files: {path: bytes}, bytes}
//!   <out>/map/derived/geo/national.sqlite      the base and the region's detail (detail.rs, base.rs)
//!   <out>/map/derived/geo/national-status.json {state: complete, features}
//!   <out>/map/derived/geo/places.sqlite        places.rs
//!   <out>/map/derived/geo/water.sqlite         water.rs
//!   <out>/map/derived/geo/buildings.sqlite     buildings.rs
//!   <out>/map/derived/geo/terrain.sqlite       terrain.rs, when a DEM is given
//!   <out>/addresses/<REGION>.sqlite            addresses.rs
//!
//! `map/` is what the region map zip holds (pack.rs); the address index is zipped on its own.

use crate::map::admin::Admin;
use crate::map::detail::{self, MapWriter};
use crate::map::osm::{self, Kind};
use crate::map::{addresses, base, buildings, gpkg, places, terrain, water};
use lastbook_engine::regions;
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub struct Sources {
    /// Geofabrik's `<region>-latest-free.gpkg.zip` (or the .gpkg).
    pub gpkg: PathBuf,
    /// Geofabrik's `<region>-latest.osm.pbf`.
    pub pbf: PathBuf,
    /// Copernicus DEM GLO-30 GeoTIFFs covering the region's boxes; none leaves terrain out.
    pub dem: Vec<PathBuf>,
    /// The folder of Natural Earth zips (base.rs); None leaves the base out.
    pub ne: Option<PathBuf>,
}

pub const TERRAIN_SOURCE: &str = "Copernicus DEM GLO-30";
pub const TERRAIN_LICENCE: &str = "© DLR e.V. 2010-2014 and © Airbus Defence and Space GmbH 2014-2018, provided under COPERNICUS by the European Union and ESA; all rights reserved";

fn now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (y, m, d) = lastbook_engine::setup::install::civil_from_days(secs.div_euclid(86400));
    format!("{y:04}-{m:02}-{d:02}")
}

/// Builds everything, printing each step. Returns what was made, with counts and sizes.
pub fn build(region: &str, src: &Sources, out: &Path) -> Result<Value, String> {
    let r = regions::get(region).ok_or_else(|| format!("{region} is not a region (crates/engine/src/regions.rs)"))?;
    let boxes = regions::boxes(region).ok_or("the region has no boxes")?;
    let country = regions::country(region);
    let started = Instant::now();
    let say = |what: &str| eprintln!("{region}: {what} ({:.1} s)", started.elapsed().as_secs_f64());
    let geo = out.join("map/derived/geo");
    let _ = std::fs::remove_dir_all(out.join("map"));
    std::fs::create_dir_all(&geo).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(out.join("addresses")).map_err(|e| e.to_string())?;
    let work = out.join("work");
    let gpkg_file = gpkg::unpacked(&src.gpkg, &work)?;
    let g = Connection::open_with_flags(&gpkg_file, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| e.to_string())?;
    let as_of = gpkg::data_as_of(&src.gpkg).unwrap_or_default();
    let osm_source = format!("OpenStreetMap contributors (ODbL 1.0), Geofabrik extract{}", if as_of.is_empty() { String::new() } else { format!(" as of {as_of}") });
    let admin = Admin::load(&g, country);
    say(&format!("{} municipalities", admin.towns.len()));
    let keep = |k: Kind, t: &[(&str, &str)]| addresses::wanted(k, t) || buildings::wanted(k, t) || places::wanted(k, t) || water::wanted(k, t);
    let things = osm::read(&src.pbf, &keep)?;
    say(&format!("{} elements from the PBF", things.len()));
    let db = |e: rusqlite::Error| e.to_string();
    // the vector map: the base, then the detail
    let mut w = MapWriter::create(&geo.join("national.sqlite")).map_err(db)?;
    let base_counts = match &src.ne {
        Some(ne) => Some(base::write(&mut w, ne)?),
        None => None,
    };
    let detail = detail::detail(&mut w, &g, boxes).map_err(db)?;
    let b = regions::bounds(region).unwrap_or([0.0; 4]);
    for (k, v) in [("source", osm_source.clone()), ("data_as_of", as_of.clone()), ("region", region.to_string()),
                   ("bounds", format!("{:.2},{:.2},{:.2},{:.2}", b[0], b[1], b[2], b[3])), ("built", now())] {
        w.meta(k, &v).map_err(db)?;
    }
    if base_counts.is_some() {
        w.meta("base_source", "Natural Earth (public domain)").map_err(db)?;
    }
    let (features, pois) = w.finish().map_err(db)?;
    std::fs::write(geo.join("national-status.json"), json!({"state": "complete", "features": features}).to_string()).map_err(|e| e.to_string())?;
    say(&format!("national.sqlite: {features} features ({} detail), {pois} help points", detail.features));
    // addresses first: the place index takes its postcodes from them
    let names: Vec<String> = gpkg::layer(&g, "gis_osm_places_free", &["name"]).iter().filter_map(|r| r.col(0).map(str::to_string)).collect();
    let rows = addresses::rows(&things, region, &admin, &names);
    let index = out.join("addresses").join(format!("{region}.sqlite"));
    addresses::write(&index, region, &rows, &osm_source).map_err(db)?;
    say(&format!("{} addresses", rows.len()));
    let ps = places::collect(&g, &things, &admin, &rows, region);
    places::write(&geo.join("places.sqlite"), &ps, &osm_source).map_err(db)?;
    say(&format!("{} places", ps.len()));
    let recs = water::collect(&g, &things, region);
    water::write(&geo.join("water.sqlite"), &recs, &admin, region, &osm_source).map_err(db)?;
    say(&format!("{} water records", recs.len()));
    let (outlines, tall) = buildings::write(&geo.join("buildings.sqlite"), &things, boxes, "OpenStreetMap").map_err(db)?;
    say(&format!("{outlines} buildings, {tall} with heights"));
    let mut terrain_counts = None;
    if !src.dem.is_empty() {
        let dems: Vec<terrain::Dem> = src.dem.iter().map(|p| terrain::Dem::read(p)).collect::<Result<_, _>>()?;
        let n = terrain::write(&geo.join("terrain.sqlite"), &dems, boxes, terrain::ZOOMS, TERRAIN_SOURCE, TERRAIN_LICENCE).map_err(db)?;
        say(&format!("{} elevation tiles, {} not covered whole by the DEM", n.tiles, n.skipped));
        terrain_counts = Some(n);
    }
    drop(g);
    let _ = std::fs::remove_dir_all(&work);
    let map = out.join("map");
    let mut files = Map::new();
    for f in ["national.sqlite", "national-status.json", "places.sqlite", "water.sqlite", "buildings.sqlite", "terrain.sqlite"] {
        let p = geo.join(f);
        if let Ok(m) = std::fs::metadata(&p) {
            files.insert(format!("derived/geo/{f}"), json!(m.len()));
        }
    }
    let bytes: u64 = files.values().filter_map(Value::as_u64).sum();
    std::fs::write(map.join("map-set.json"), serde_json::to_string_pretty(&json!({"name": region, "files": files.clone(), "bytes": bytes})).unwrap()).map_err(|e| e.to_string())?;
    say("done");
    Ok(json!({
        "region": region, "title": r.name, "files": files, "bytes": bytes,
        "address_index": {"path": index.to_string_lossy(), "bytes": std::fs::metadata(&index).map(|m| m.len()).unwrap_or(0), "addresses": rows.len()},
        "features": features, "detail_features": detail.features, "help_points": pois, "places": ps.len(), "water_records": recs.len(),
        "buildings": outlines, "buildings_with_heights": tall,
        "base": base_counts.map(|c| json!({"roads": c.roads, "places": c.places, "water": c.water})),
        "terrain": terrain_counts.map(|c| json!({"tiles": c.tiles, "not_covered": c.skipped})),
        "seconds": started.elapsed().as_secs(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::gpkg::write::{row, table};
    use crate::map::gpkg::Kind as Geom;
    use crate::map::osm::write::{pbf, El};
    use lastbook_engine::setup::{download::Control, install};

    /// Small sources for Liechtenstein, made here: a GeoPackage, a PBF, a GeoTIFF and a Natural Earth zip.
    fn sources(dir: &Path) -> Sources {
        std::fs::create_dir_all(dir.join("ne")).unwrap();
        let gpkg = dir.join("li.gpkg");
        let g = Connection::open(&gpkg).unwrap();
        for t in ["gis_osm_roads_free", "gis_osm_places_free", "gis_osm_adminareas_a_free", "gis_osm_pois_free", "gis_osm_water_a_free"] {
            table(&g, t, &["fclass", "name", "ref", "population", "osm_id"]);
        }
        row(&g, "gis_osm_roads_free", Geom::Line, &[vec![(9.5200, 47.1380), (9.5215, 47.1395), (9.5225, 47.1410)]], &[("fclass", "residential"), ("name", "Städtle")]);
        row(&g, "gis_osm_places_free", Geom::Point, &[vec![(9.5209, 47.1410)]], &[("fclass", "town"), ("name", "Vaduz"), ("osm_id", "1")]);
        row(&g, "gis_osm_adminareas_a_free", Geom::Polygon, &[vec![(9.47, 47.04), (9.64, 47.04), (9.64, 47.28), (9.47, 47.28), (9.47, 47.04)]],
            &[("fclass", "national"), ("name", "Liechtenstein")]);
        row(&g, "gis_osm_adminareas_a_free", Geom::Polygon, &[vec![(9.50, 47.12), (9.56, 47.12), (9.56, 47.155), (9.50, 47.155), (9.50, 47.12)]],
            &[("fclass", "admin_level8"), ("name", "Vaduz")]);
        row(&g, "gis_osm_pois_free", Geom::Point, &[vec![(9.5218, 47.1398)]], &[("fclass", "pharmacy"), ("name", "Apotheke am Städtle")]);
        row(&g, "gis_osm_water_a_free", Geom::Polygon, &[vec![(9.51, 47.20), (9.513, 47.20), (9.513, 47.203), (9.51, 47.20)]], &[("fclass", "water"), ("name", "Gampriner Seele")]);
        drop(g);
        let sq = |id: i64, x: f64, y: f64| vec![El::Node(id, y, x, &[]), El::Node(id + 1, y, x + 0.0002, &[]), El::Node(id + 2, y + 0.0002, x + 0.0002, &[]), El::Node(id + 3, y + 0.0002, x, &[])];
        let mut els = vec![
            El::Node(1, 47.1410, 9.5209, &[("place", "town"), ("name", "Vaduz"), ("name:it", "Vaduzzo")]),
            El::Node(2, 47.1394, 9.5214, &[("addr:housenumber", "11"), ("addr:street", "Städtle"), ("addr:postcode", "9490"), ("addr:city", "Vaduz"), ("addr:country", "LI")]),
        ];
        els.extend(sq(10, 9.5216, 47.1396));
        els.push(El::Way(100, &[10, 11, 12, 13, 10], &[("building", "retail"), ("building:levels", "3"), ("addr:housenumber", "13"), ("addr:street", "Städtle"), ("addr:postcode", "9490")]));
        let pbf_path = dir.join("li.osm.pbf");
        std::fs::write(&pbf_path, pbf(&els)).unwrap();
        let dem = dir.join("dem.tif");
        crate::map::terrain::write::geotiff(&dem, 9.40, 47.30, 0.002, 150, 150, false);
        crate::map::shp::write::zip(&dir.join("ne/ne_10m_roads.zip"), crate::map::shp::Kind::Line, &[(vec![vec![(9.0, 47.0), (9.6, 47.2), (10.0, 47.5)]],
            vec![("type", "Major Highway"), ("expressway", "0"), ("min_zoom", "5"), ("name", "13")])]);
        Sources { gpkg, pbf: pbf_path, dem: vec![dem], ne: Some(dir.join("ne")) }
    }

    #[test]
    fn a_region_from_its_sources_to_what_setup_installs() {
        let dir = std::env::temp_dir().join(format!("lb-tools-mapset-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let src = sources(&dir.join("src"));
        let out = dir.join("out");
        let report = build("LI", &src, &out).unwrap();
        assert_eq!(report["address_index"]["addresses"], 2);
        assert_eq!(report["buildings"], 1);
        assert!(report["terrain"]["tiles"].as_u64().unwrap() > 0);
        let set: Value = serde_json::from_slice(&std::fs::read(out.join("map/map-set.json")).unwrap()).unwrap();
        assert_eq!(set["name"], "LI");
        assert_eq!(set["files"].as_object().unwrap().len(), 6);
        // packed and unpacked as setup does, with the address index beside the map
        let packs = dir.join("packs");
        let base = lastbook_engine::dist::download_base();
        let m = crate::map::pack::map(&out.join("map"), &packs, &base).unwrap();
        crate::map::pack::addresses(&out.join("addresses/LI.sqlite"), &packs, &base).unwrap();
        let lib = install::unpack(&packs.join("Lastbook-Map-LI.zip"), &dir.join("unpack"), m["installed_bytes"].as_u64(), &Control::new(), &mut |_, _| {},
                                  Some(&install::map_member), None).unwrap();
        std::fs::create_dir_all(lib.join("derived/geo/addresses")).unwrap();
        std::fs::copy(out.join("addresses/LI.sqlite"), lib.join("derived/geo/addresses/LI.sqlite")).unwrap();
        // the engine answers from it
        let map = lastbook_engine::map::Map::new(lastbook_engine::paths::Paths::new(dir.clone(), Some(lib)));
        let a = map.find_addresses("Städtle 11, Vaduz", 8, None);
        assert_eq!((a[0]["name"].as_str(), a[0]["detail"].as_str()), (Some("Städtle 11"), Some("9490 Vaduz, Liechtenstein")));
        assert_eq!(map.find_addresses("Staedtle 13", 8, None)[0]["detail"], "9490 Vaduz, Liechtenstein", "the town from its municipality");
        assert!(map.find_addresses("Städtle 12, Vaduz", 8, None).is_empty(), "never a guessed house");
        assert_eq!(map.places("Vaduzzo", 20, None)[0]["name"], "Vaduz");
        assert_eq!(map.places("9490", 20, None)[0]["kind"], "Postcode");
        assert_eq!(map.places("Gampriner Seele", 20, None)[0]["kind"], "Lake");
        assert_eq!(map.near(47.1400, 9.5209, &["pharmacy"], 1)[0]["name"], "Apotheke am Städtle");
        let tile = |z: u32| -> Value {
            let (x, y) = crate::map::geo::tile_of(z, 9.5209, 47.1400);
            serde_json::from_slice(&map.tile(z, x, y)).unwrap()
        };
        assert_eq!(tile(6)["layers"]["road"][0][0], "trunk", "the base zoomed out");
        assert!(tile(14)["layers"]["road"].as_array().unwrap().iter().any(|r| r[1] == "Städtle"));
        assert!(map.elevation(47.15, 9.55).is_some());
        let info = map.map_info();
        assert_eq!(info["detail"], true);
        assert!(info["datasets"].to_string().contains("Elevation (Copernicus DEM GLO-30)"), "{}", info["datasets"]);
        lastbook_engine::db::close_all();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
