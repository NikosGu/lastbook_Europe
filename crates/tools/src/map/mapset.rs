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

use crate::map::admin::{self, Admin};
use crate::map::detail::{self, MapWriter};
use crate::map::osm::{self, Kind};
use crate::map::gpkg::Layers;
use crate::map::{addresses, base, buildings, coast, gpkg, places, terrain, water};
use lastbook_engine::regions;
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub struct Sources {
    /// Geofabrik's `<region>-latest-free.gpkg.zip` (or the .gpkg) of the region: one, or the old French regions that
    /// make a new one.
    pub gpkg: Vec<PathBuf>,
    /// Geofabrik's `<region>-latest.osm.pbf` of the region, likewise.
    pub pbf: Vec<PathBuf>,
    /// The neighbours' GeoPackages and PBFs, read only inside the region's boxes (Switzerland's for Liechtenstein's bank
    /// of the Rhine).
    pub neighbour_gpkg: Vec<PathBuf>,
    pub neighbour_pbf: Vec<PathBuf>,
    /// Copernicus DEM GLO-30 GeoTIFFs covering the region's boxes; none leaves terrain out.
    pub dem: Vec<PathBuf>,
    /// The most detailed elevation tiles, 9 to 12 (terrain::ZOOMS).
    pub terrain_zoom: u32,
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
    let Some(own_gpkg) = src.gpkg.first() else { return Err("a GeoPackage of the region is needed".into()) };
    if src.pbf.is_empty() {
        return Err("a PBF of the region is needed".into());
    }
    if !terrain::ZOOMS.contains(&src.terrain_zoom) {
        return Err(format!("the terrain's zoom is {} to {}, not {}", terrain::ZOOMS.start(), terrain::ZOOMS.end(), src.terrain_zoom));
    }
    let work = out.join("work");
    let unpack = |list: &[PathBuf]| list.iter().map(|p| gpkg::unpacked(p, &work)).collect::<Result<Vec<_>, _>>();
    let g = gpkg::Set::open(&unpack(&src.gpkg)?, &unpack(&src.neighbour_gpkg)?, boxes)?;
    let as_of = gpkg::data_as_of(own_gpkg).unwrap_or_default();
    let osm_source = format!("OpenStreetMap contributors (ODbL 1.0), Geofabrik extract{}", if as_of.is_empty() { String::new() } else { format!(" as of {as_of}") });
    // the region's own extracts say which country an address lies in; the neighbours' only add what is in the boxes
    let admin = Admin::from_rows(&g.own_layer("gis_osm_adminareas_a_free", admin::ADMIN_COLUMNS), country);
    say(&format!("{} municipalities", admin.towns.len()));
    let db = |e: rusqlite::Error| e.to_string();
    // the vector map: the base, then the detail, read from the GeoPackages before the PBFs, so that a large region does
    // not hold both at once
    let mut w = MapWriter::create(&geo.join("national.sqlite")).map_err(db)?;
    let base_counts = match &src.ne {
        Some(ne) => Some(base::write(&mut w, ne)?),
        None => None,
    };
    let detail = detail::detail(&mut w, &g, boxes).map_err(db)?;
    say(&format!("{} features of detail", detail.features));
    let keep = |k: Kind, t: &[(&str, &str)]| {
        addresses::wanted(k, t) || addresses::street_wanted(k, t) || buildings::wanted(k, t) || places::wanted(k, t) || water::wanted(k, t)
            || coast::wanted(k, t)
    };
    // an element two extracts hold (a building on the border) comes once, from the first that has it
    let mut things = Vec::new();
    let mut seen: HashSet<(Kind, i64)> = HashSet::new();
    for (p, near) in src.pbf.iter().map(|p| (p, false)).chain(src.neighbour_pbf.iter().map(|p| (p, true))) {
        let read = if near { osm::read_within(p, &keep, boxes)? } else { osm::read(p, &keep)? };
        let before = things.len();
        things.extend(read.into_iter().filter(|t| seen.insert((t.kind, t.id))));
        if near {
            say(&format!("{} more elements in the boxes from {}", things.len() - before, p.display()));
        }
    }
    drop(seen);
    say(&format!("{} elements from the PBF", things.len()));
    // the coast at every zoom: land the world outline lacks, sea it covers
    let shore = coast::coast(&things);
    let (islands, seas) = (coast::kept(&shore.islands, boxes), coast::kept(&shore.seas, boxes));
    for r in &islands {
        w.add("island", "island", None, 0, 30, std::slice::from_ref(r)).map_err(db)?;
    }
    for r in &seas {
        w.add("ocean", "ocean", None, 0, 30, std::slice::from_ref(r)).map_err(db)?;
    }
    say(&format!("coast: {} islands, {} seas ({} cut by the extract, closed across their ends)", islands.len(), seas.len(), shore.open));
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
    let names: Vec<String> = g.layer("gis_osm_places_free", &["name"]).iter().filter_map(|r| r.col(0).map(str::to_string)).collect();
    let rows = addresses::rows(&things, region, &admin, &names);
    let index = out.join("addresses").join(format!("{region}.sqlite"));
    let count = addresses::write(&index, region, &rows, &osm_source, &addresses::town_names(&things, country)).map_err(db)?;
    say(&format!("{count} addresses, and {} rows under a street's other names", rows.len() - count));
    let ps = places::collect(&g, &things, &admin, &rows, region);
    places::write(&geo.join("places.sqlite"), &ps, &osm_source).map_err(db)?;
    say(&format!("{} places", ps.len()));
    let recs = water::collect(&g, &things, region);
    water::write(&geo.join("water.sqlite"), &recs, &admin, region, &osm_source).map_err(db)?;
    say(&format!("{} water records", recs.len()));
    let (outlines, tall) = buildings::write(&geo.join("buildings.sqlite"), &things, boxes, "OpenStreetMap").map_err(db)?;
    say(&format!("{outlines} buildings, {tall} with heights"));
    // what was read from the PBFs and the GeoPackages is let go before the terrain takes its memory
    let (row_count, place_count, water_count) = (rows.len(), ps.len(), recs.len());
    drop((things, rows, ps, recs, admin, names));
    drop(g);
    let mut terrain_counts = None;
    if !src.dem.is_empty() {
        let mut dems = terrain::Dems::open(&src.dem, terrain::DEM_MEMORY)?;
        let n = terrain::write(&geo.join("terrain.sqlite"), &mut dems, boxes, *terrain::ZOOMS.start()..=src.terrain_zoom, TERRAIN_SOURCE, TERRAIN_LICENCE)?;
        say(&format!("{} elevation tiles to zoom {}, {} not covered whole by the DEM ({} DEM files, read {} times)", n.tiles, src.terrain_zoom,
                     n.skipped, src.dem.len(), dems.reads));
        terrain_counts = Some(n);
    }
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
        "address_index": {"path": index.to_string_lossy(), "bytes": std::fs::metadata(&index).map(|m| m.len()).unwrap_or(0), "addresses": count,
                          "rows": row_count},
        "features": features, "detail_features": detail.features, "help_points": pois, "places": place_count, "water_records": water_count,
        "buildings": outlines, "buildings_with_heights": tall,
        "coast": {"islands": islands.len(), "seas": seas.len(), "open": shore.open},
        "base": base_counts.map(|c| json!({"roads": c.roads, "places": c.places, "water": c.water})),
        "terrain": terrain_counts.map(|c| json!({"tiles": c.tiles, "not_covered": c.skipped, "zoom": src.terrain_zoom})),
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
    use rusqlite::Connection;

    /// Small sources for Liechtenstein, made here: a GeoPackage, a PBF, a GeoTIFF and a Natural Earth zip.
    fn sources(dir: &Path) -> Sources {
        std::fs::create_dir_all(dir.join("ne")).unwrap();
        let gpkg = dir.join("li.gpkg");
        let g = Connection::open(&gpkg).unwrap();
        for t in ["gis_osm_roads_free", "gis_osm_places_free", "gis_osm_adminareas_a_free", "gis_osm_pois_free", "gis_osm_water_a_free"] {
            table(&g, t, &["fclass", "name", "ref", "population", "osm_id"]);
        }
        row(&g, "gis_osm_roads_free", Geom::Line, &[vec![(9.5200, 47.1380), (9.5215, 47.1395), (9.5225, 47.1410)]], &[("fclass", "residential"), ("name", "Städtle"), ("osm_id", "500")]);
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
        Sources { gpkg: vec![gpkg], pbf: vec![pbf_path], neighbour_gpkg: Vec::new(), neighbour_pbf: Vec::new(), dem: vec![dem], terrain_zoom: 12,
                  ne: Some(dir.join("ne")) }
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
        let a = crate::map::pack::addresses(&out.join("addresses/LI.sqlite"), &packs, &base).unwrap();
        // the check the workflow runs after a build: setup's checks, then the town, the addresses and the boxes
        std::fs::write(packs.join("map-entry-LI.json"), m.to_string()).unwrap();
        std::fs::write(packs.join("addresses-entry-LI.json"), a.to_string()).unwrap();
        let report = crate::map::check::check("LI", &packs, &dir.join("check"), "Vaduz", 2).unwrap();
        assert_eq!(report["ok"], true, "{report:#}");
        assert_eq!(report["checks"].as_array().unwrap().len(), 8);
        let fewer = crate::map::check::check("LI", &packs, &dir.join("check"), "Vaduz", 3).unwrap();
        assert_eq!(fewer["ok"], false, "two addresses are not three");
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
        // a region whose zoom-12 elevation would be too large stops at a lower zoom
        let coarse = Sources { terrain_zoom: 11, ..sources(&dir.join("src-coarse")) };
        build("LI", &coarse, &dir.join("coarse")).unwrap();
        let t = Connection::open(dir.join("coarse/map/derived/geo/terrain.sqlite")).unwrap();
        assert_eq!(t.query_row("SELECT max(z) FROM tile", [], |r| r.get::<_, i64>(0)).unwrap(), 11, "the small test DEM covers no tile below 11");
        let fine = Sources { terrain_zoom: 13, gpkg: coarse.gpkg.clone(), pbf: coarse.pbf.clone(), neighbour_gpkg: Vec::new(), neighbour_pbf: Vec::new(),
                             dem: coarse.dem.clone(), ne: None };
        assert!(build("LI", &fine, &dir.join("fine")).is_err(), "zoom 12 at most");
        // the open file first: Windows removes no folder that holds one
        drop(t);
        lastbook_engine::db::close_all();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_neighbours_extract_adds_what_lies_in_the_boxes() {
        let dir = std::env::temp_dir().join(format!("lb-tools-neighbours-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut src = sources(&dir.join("src"));
        // Switzerland's extract: the bank of the Rhine in Liechtenstein's box, Zürich far outside it, Liechtenstein's
        // street and building again (an extract reaches past its border), and the whole country's outline
        let ch = dir.join("src/ch.gpkg");
        let g = Connection::open(&ch).unwrap();
        for t in ["gis_osm_roads_free", "gis_osm_adminareas_a_free"] {
            table(&g, t, &["fclass", "name", "ref", "osm_id"]);
        }
        let road = |name: &str, x: f64, y: f64, id: &str| row(&g, "gis_osm_roads_free", Geom::Line, &[vec![(x, y), (x + 0.004, y + 0.004)]], &[("fclass", "residential"), ("name", name), ("osm_id", id)]);
        road("Rheinstrasse", 9.470, 47.165, "600");
        road("Bahnhofstrasse", 8.539, 47.373, "601");
        road("Städtle", 9.520, 47.138, "500");
        row(&g, "gis_osm_adminareas_a_free", Geom::Polygon, &[vec![(5.9, 45.8), (10.5, 45.8), (10.5, 47.8), (5.9, 47.8), (5.9, 45.8)]], &[("fclass", "national"), ("name", "Schweiz")]);
        drop(g);
        let sq = |id: i64, x: f64, y: f64| vec![El::Node(id, y, x, &[]), El::Node(id + 1, y, x + 0.0002, &[]), El::Node(id + 2, y + 0.0002, x + 0.0002, &[]), El::Node(id + 3, y + 0.0002, x, &[])];
        let mut els = Vec::new();
        els.extend(sq(1000, 9.472, 47.168));
        els.extend(sq(1010, 8.540, 47.375));
        els.extend(sq(10, 9.5216, 47.1396));
        els.push(El::Way(2000, &[1000, 1001, 1002, 1003, 1000], &[("building", "yes")]));
        els.push(El::Way(2001, &[1010, 1011, 1012, 1013, 1010], &[("building", "yes")]));
        els.push(El::Way(100, &[10, 11, 12, 13, 10], &[("building", "retail"), ("building:levels", "3"), ("addr:housenumber", "13"), ("addr:street", "Städtle"), ("addr:postcode", "9490")]));
        let ch_pbf = dir.join("src/ch.osm.pbf");
        std::fs::write(&ch_pbf, pbf(&els)).unwrap();
        src.neighbour_gpkg.push(ch);
        src.neighbour_pbf.push(ch_pbf);
        let report = build("LI", &src, &dir.join("out")).unwrap();
        let c = Connection::open(dir.join("out/map/derived/geo/national.sqlite")).unwrap();
        let named = |n: &str| -> i64 { c.query_row("SELECT count(*) FROM feat WHERE layer='road' AND name=?1", [n], |r| r.get(0)).unwrap() };
        assert_eq!((named("Rheinstrasse"), named("Bahnhofstrasse"), named("Städtle")), (1, 0, 1), "the bank in the box, not Zürich, the street once");
        assert_eq!(report["buildings"], 2, "Liechtenstein's building once, and the one on the bank");
        assert_eq!(report["address_index"]["addresses"], 2, "Liechtenstein's outline still says where its addresses are, not Switzerland's");
        drop(c);
        let _ = std::fs::remove_dir_all(&dir); // a temporary folder left behind (Windows, a file still held) fails no test
    }
}
