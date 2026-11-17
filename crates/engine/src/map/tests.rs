//! The map's answers, against tiny map files made here: a few roads, places, help points, a lake, an elevation tile
//! and a building, around Vaduz. Ported from app/tests/test_bridge.py and test_what_ships.py (the map parts), with the
//! shape of the test library of app/tests/fixture.py.

use super::tiles::{pack, tile_bounds, EXT};
use super::*;
use rusqlite::params;
use serde_json::Value;

pub(super) struct Fx {
    pub dir: PathBuf,
    pub map: Map,
}

impl Drop for Fx {
    fn drop(&mut self) {
        db::close_all();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A data folder (`data/`) and a program folder (`app/web/`) of their own, with the made-up library's national status.
pub(super) fn fx(name: &str) -> Fx {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("lb-map-{name}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for d in ["data/derived/geo/addresses", "data/library-data", "app/web"] {
        std::fs::create_dir_all(dir.join(d)).unwrap();
    }
    std::fs::write(
        dir.join("data/derived/geo/national-status.json"),
        r#"{"state": "complete", "features": 7, "source": "/Users/someone/library-data/x.pbf", "database": "/Users/someone/derived/geo/national.sqlite"}"#,
    )
    .unwrap();
    let map = Map::new(Paths::new(dir.join("app"), Some(dir.join("data"))));
    Fx { dir, map }
}

impl Fx {
    pub fn file(&self, rel: &str) -> PathBuf {
        self.dir.join("data").join(rel)
    }

    /// A SQLite file in the data folder, made by `sql`.
    pub fn sqlite(&self, rel: &str, sql: &str) -> rusqlite::Connection {
        let p = self.file(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let c = rusqlite::Connection::open(&p).unwrap();
        c.execute_batch(sql).unwrap();
        c
    }

    pub fn get(&self, url: &str) -> Response {
        self.map.route(&Request::get(url)).expect("a map route")
    }

    pub fn json(&self, url: &str) -> Value {
        let r = self.get(url);
        assert_eq!(r.status, 200, "{url}: {}", String::from_utf8_lossy(&r.body));
        serde_json::from_slice(&r.body).unwrap()
    }
}

const MAP_TABLES: &str = "CREATE TABLE feat(id INTEGER PRIMARY KEY, layer TEXT, cls TEXT, name TEXT, minz INTEGER, geom BLOB);
    CREATE VIRTUAL TABLE feat_idx USING rtree(id, minx, maxx, miny, maxy, minz, maxz);
    CREATE TABLE poi(id INTEGER PRIMARY KEY, cls TEXT, name TEXT, lon REAL, lat REAL);
    CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT);";

const RESOURCE_TABLES: &str = "CREATE TABLE resource(id INTEGER PRIMARY KEY, source_id TEXT, kind TEXT, name TEXT, lat REAL, lon REAL, detail TEXT, source TEXT, origin TEXT);
    CREATE VIRTUAL TABLE spatial USING rtree(id, minx, maxx, miny, maxy);";

fn feature(c: &rusqlite::Connection, id: i64, layer: &str, cls: &str, name: Option<&str>, minz: i64, parts: &[Vec<(f64, f64)>]) {
    c.execute("INSERT INTO feat VALUES (?1,?2,?3,?4,?5,?6)", params![id, layer, cls, name, minz, pack(parts)]).unwrap();
    let xs = parts.iter().flatten().map(|p| p.0);
    let ys = parts.iter().flatten().map(|p| p.1);
    let (x0, x1) = (xs.clone().fold(f64::MAX, f64::min), xs.fold(f64::MIN, f64::max));
    let (y0, y1) = (ys.clone().fold(f64::MAX, f64::min), ys.fold(f64::MIN, f64::max));
    c.execute("INSERT INTO feat_idx VALUES (?1,?2,?3,?4,?5,?6,30)", params![id, x0, x1, y0, y1, minz]).unwrap();
}

#[allow(clippy::too_many_arguments)]
fn resource(c: &rusqlite::Connection, id: i64, kind: &str, name: &str, lat: f64, lon: f64, detail: &str, source: &str) {
    c.execute("INSERT INTO resource VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'')", params![id, format!("s{id}"), kind, name, lat, lon, detail, source]).unwrap();
    c.execute("INSERT INTO spatial VALUES (?1,?2,?2,?3,?3)", params![id, lon, lat]).unwrap();
}

/// The tile holding a point.
pub(super) fn tile_of(z: u32, lat: f64, lon: f64) -> (i64, i64) {
    let n = 2f64.powi(z as i32);
    let s = lat.to_radians().sin();
    (((lon + 180.0) / 360.0 * n).floor() as i64, ((0.5 - ((1.0 + s) / (1.0 - s)).ln() / (4.0 * std::f64::consts::PI)) * n).floor() as i64)
}

const VADUZ: (f64, f64) = (47.1410, 9.5209);

/// National map with Vaduz: a road, a town, a lake area, and help points (two pharmacies mapped twice).
fn vaduz(f: &Fx) {
    let c = f.sqlite("derived/geo/national.sqlite", MAP_TABLES);
    let (la, lo) = VADUZ;
    feature(&c, 1, "road", "primary", Some("Städtle"), 10, &[vec![(lo - 0.01, la), (lo, la + 0.001), (lo + 0.01, la)]]);
    feature(&c, 2, "place", "town", Some("Vaduz"), 8, &[vec![(lo, la)]]);
    feature(&c, 3, "water", "lake", None, 10, &[vec![(lo - 0.002, la - 0.002), (lo + 0.002, la - 0.002), (lo + 0.002, la + 0.002), (lo - 0.002, la + 0.002)]]);
    feature(&c, 4, "bldg", "yes", None, 15, &[vec![(lo, la), (lo + 0.0002, la), (lo + 0.0002, la + 0.0002), (lo, la + 0.0002)]]);
    for (id, cls, name, dla, dlo) in [(1, "hospital", "Landesspital", 0.002, 0.001), (2, "pharmacy", "Apotheke am Städtle", 0.0005, 0.0005),
                                     (3, "pharmacy", "Apotheke am Städtle", 0.0006, 0.0007), (4, "chemist", "Drogerie", -0.01, 0.02), (5, "fire_station", "Feuerwehr", 0.02, -0.02),
                                     (6, "school", "Primarschule", 0.001, 0.0)] {
        c.execute("INSERT INTO poi VALUES (?1,?2,?3,?4,?5)", params![id, cls, name, lo + dlo, la + dla]).unwrap();
    }
}

#[test]
fn map_info_has_what_the_ui_reads_and_no_personal_paths() {
    let f = fx("info");
    let r = f.get("/api/map/info");
    assert_eq!(r.status, 200);
    assert!(!String::from_utf8_lossy(&r.body).contains("/Users/"), "no paths from the machine that built it");
    let info: Value = serde_json::from_slice(&r.body).unwrap();
    assert_eq!(info["national"], json!({"state": "complete", "features": 7}));
    for k in ["national", "version", "aerial_detail", "aerial_rev", "terrain_circle", "datasets", "layers", "bounds", "detail", "contours", "aerial", "meta", "credits"] {
        assert!(info.get(k).is_some(), "{k}");
    }
    assert_eq!((&info["meta"], &info["credits"]), (&json!({}), &json!([])), "no map files: nothing to describe, no one to credit");
    assert!(info["layers"].is_array());
    assert_eq!(info["aerial"], false);
    assert_eq!(f.map.national_status()["source"], "/Users/someone/library-data/x.pbf", "the engine itself still sees it");
    // not a pack: described from its own files, as a pack is
    assert!(f.map.pack_info().is_none());
    let datasets = info["datasets"].to_string();
    for us in ["United States", "USGS", "ZIP", "mi\"", "acre"] {
        assert!(!datasets.contains(us), "{us}");
    }
    assert!(datasets.contains("This download has only the base map"));
    assert_eq!(info["datasets"], json!([
        {"code": "base", "params": {"source": ""}, "label": "Main roads, towns and water", "status": "Shown at every zoom level"},
        {"code": "no_detail", "params": {}, "label": "No street detail or help points",
         "status": "This download has only the base map. Hospitals, water taps and side streets are not in it."},
    ]), "each row a code with its params, the English beside them; no map file, so the base map names no source");
    assert_eq!(info["version"], "n7");
    // the assistant's map sources: no map description, so OpenStreetMap's credit and no date
    assert_eq!(f.map.data_credit(), json!({"text": "© OpenStreetMap contributors", "code": "osm", "credit": "© OpenStreetMap contributors", "data_as_of": ""}));
}

#[test]
fn map_info_names_what_the_files_hold() {
    let f = fx("info2");
    vaduz(&f);
    f.sqlite("derived/geo/terrain.sqlite", "CREATE TABLE tile(z INT, x INT, y INT, png BLOB, PRIMARY KEY(z,x,y)) WITHOUT ROWID;
        CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT); INSERT INTO meta VALUES ('source', 'Copernicus DEM GLO-30'); INSERT INTO tile VALUES (11, 1, 1, x'00');");
    let w = f.sqlite("derived/geo/water.sqlite", RESOURCE_TABLES);
    resource(&w, 1, "lake", "Gänglesee", 47.12, 9.6, r#"{"area_ha": 2.5}"#, "test");
    let a = f.sqlite("derived/geo/addresses/LI.sqlite", "CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT); INSERT INTO meta VALUES ('publisher', 'Landesverwaltung Liechtenstein');");
    drop(a);
    std::fs::write(f.file("pack.json"), r#"{"name": "LI", "map": "LI"}"#).unwrap();
    let info = f.json("/api/map/info");
    assert_eq!(info["layers"], json!(["chemist", "fire_station", "hospital", "lake", "pharmacy", "school"]));
    assert_eq!(info["detail"], true);
    assert_eq!(info["contours"], true);
    let b = info["bounds"].as_array().unwrap();
    assert!((b[0].as_f64().unwrap() - (VADUZ.1 - 0.02)).abs() < 1e-9 && (b[3].as_f64().unwrap() - (VADUZ.0 + 0.02)).abs() < 1e-9);
    let d = info["datasets"].to_string();
    assert!(d.contains("Elevation (Copernicus DEM GLO-30)"), "{d}");
    assert!(d.contains("Street addresses (Landesverwaltung Liechtenstein)") && d.contains("house numbers in Liechtenstein"), "{d}");
    assert!(d.contains("Street detail stops at the edge of this area."));
    assert_eq!(info["datasets"], json!([
        {"code": "base", "params": {"source": ""}, "label": "Main roads, towns and water", "status": "Shown at every zoom level"},
        {"code": "streets_detail", "params": {}, "label": "Streets, places and help points (OpenStreetMap) for the area this map details",
         "status": "Street detail stops at the edge of this area."},
        {"code": "addresses", "params": {"source": "Landesverwaltung Liechtenstein", "regions": ["LI"], "region_names": ["Liechtenstein"]},
         "label": "Street addresses (Landesverwaltung Liechtenstein)", "status": "Used by \"Find a place\": house numbers in Liechtenstein"},
        {"code": "elevation_contours", "params": {"source": "Copernicus DEM GLO-30"}, "label": "Elevation (Copernicus DEM GLO-30)",
         "status": "Shaded relief. Contour lines only where this map has fine detail."},
    ]));
    assert!(info["version"].as_str().unwrap().starts_with("n7."), "the water and elevation files are part of the version");
    assert_eq!(f.map.pack_info().unwrap()["map"], "LI");
    assert_eq!(info["credits"], json!([{"text": "Copernicus DEM GLO-30"}, {"text": "Landesverwaltung Liechtenstein"}]),
               "only the sources the files name: this national map and its lake name none");
}

/// A region's map as `lastbook-tools build-map` writes it (crates/tools/src/map/mapset.rs): national.sqlite's meta says
/// when its data was taken and names OpenStreetMap and Natural Earth; the other files name their own sources.
fn liechtenstein(f: &Fx) {
    let meta = "CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT);";
    let osm = "OpenStreetMap contributors (ODbL 1.0), Geofabrik extract as of 2026-11-16T21:22:36Z";
    vaduz(f);
    let c = f.sqlite("derived/geo/national.sqlite", "");
    for (k, v) in [("source", osm), ("data_as_of", "2026-11-16T21:22:36Z"), ("region", "LI"), ("bounds", "9.32,46.90,9.79,47.43"), ("built", "2026-11-17"),
                   ("base_source", "Natural Earth (public domain)")] {
        c.execute("INSERT INTO meta VALUES (?1,?2)", params![k, v]).unwrap();
    }
    drop(c);
    places_db(f, &[("Vaduz", "town", "Liechtenstein", 47.14, 9.52, 2, &[])]);
    f.sqlite("derived/geo/places.sqlite", meta).execute("INSERT INTO meta VALUES ('source', ?1)", [osm]).unwrap();
    f.sqlite("derived/geo/water.sqlite", &format!("{RESOURCE_TABLES} {meta}")).execute("INSERT INTO meta VALUES ('source', ?1)", [osm]).unwrap();
    f.sqlite("derived/geo/buildings.sqlite", &format!("CREATE TABLE bldg(x INT, y INT, part INT, n INT, data BLOB, PRIMARY KEY(x,y,part)) WITHOUT ROWID; {meta}
        INSERT INTO meta VALUES ('source', 'OpenStreetMap');"));
    f.sqlite("derived/geo/terrain.sqlite", &format!("CREATE TABLE tile(z INT, x INT, y INT, png BLOB, PRIMARY KEY(z,x,y)) WITHOUT ROWID; {meta}
        INSERT INTO meta VALUES ('source', 'Copernicus DEM GLO-30'), ('licence', '© DLR e.V. 2010-2014 and © Airbus Defence and Space GmbH 2014-2018');"));
    let a = f.sqlite("derived/geo/addresses/LI.sqlite", meta);
    a.execute_batch("INSERT INTO meta VALUES ('publisher', 'OpenStreetMap'), ('region', 'LI');").unwrap();
    a.execute("INSERT INTO meta VALUES ('source', ?1)", [osm]).unwrap();
}

#[test]
fn map_info_says_when_its_data_was_taken_and_who_made_it() {
    let f = fx("info-li");
    liechtenstein(&f);
    let info = f.json("/api/map/info");
    let meta = &info["meta"];
    assert_eq!((meta["data_as_of"].as_str(), meta["region"].as_str(), meta["base_source"].as_str()), (Some("2026-11-16T21:22:36Z"), Some("LI"), Some("Natural Earth (public domain)")),
               "a region's map: national.sqlite's meta");
    assert_eq!(info["credits"], json!([
        {"code": "osm", "params": {}, "text": "© OpenStreetMap contributors"},
        {"text": "Natural Earth"},
        {"text": "Copernicus DEM GLO-30", "licence": "© DLR e.V. 2010-2014 and © Airbus Defence and Space GmbH 2014-2018"},
    ]), "each source once, OpenStreetMap as a code, a licence in brackets left out of a name");
    let said: Vec<&str> = info["credits"].as_array().unwrap().iter().filter_map(|c| c["code"].as_str()).collect();
    assert_eq!(said, info::CREDIT_CODES, "every credit code is said");
    assert!(info["datasets"].as_array().unwrap().iter().any(|d| d["code"] == "addresses" && d["params"]["regions"] == json!(["LI"])),
            "the panel names the region whose addresses are there");
    assert_eq!(info["datasets"][0], json!({"code": "base", "params": {"source": "OpenStreetMap, Natural Earth"},
                                           "label": "Main roads, towns and water (OpenStreetMap, Natural Earth)", "status": "Shown at every zoom level"}),
               "the base map names the sources national.sqlite names");
    assert_eq!(f.map.data_credit(), json!({"text": "© OpenStreetMap contributors (data as of 2026-11-16)", "code": "osm", "credit": "© OpenStreetMap contributors",
                                           "data_as_of": "2026-11-16"}), "the assistant's map sources: whose data, and of when");
    // map.sqlite's own meta comes first; national.sqlite adds what it lacks
    let c = f.sqlite("derived/geo/map.sqlite", MAP_TABLES);
    c.execute_batch("INSERT INTO meta VALUES ('source', 'Overture Maps Foundation'), ('data_as_of', '2026-10-25');").unwrap();
    drop(c);
    let info = f.json("/api/map/info");
    assert_eq!((info["meta"]["data_as_of"].as_str(), info["meta"]["region"].as_str()), (Some("2026-10-25"), Some("LI")));
    assert_eq!(info["credits"][0], json!({"text": "Overture Maps Foundation"}));
    assert_eq!(info["credits"][1]["code"], "osm");
    assert_eq!((info["datasets"][0]["params"]["source"].as_str(), info["datasets"][0]["label"].as_str()),
               (Some("Overture Maps Foundation, OpenStreetMap, Natural Earth"), Some("Main roads, towns and water (Overture Maps Foundation, OpenStreetMap, Natural Earth)")));
    assert_eq!(f.map.data_credit(), json!({"text": "Overture Maps Foundation (data as of 2026-10-25)", "code": "", "credit": "Overture Maps Foundation",
                                           "data_as_of": "2026-10-25"}));
}

/// The panel's rows for three more map sets: a region with its places, buildings and crops; a map of everything whose
/// files name no source, with the address indexes of a country and of a region; and the base map with elevation only.
/// Between them, every code is said.
#[test]
fn map_info_says_each_row_as_a_code() {
    let base = json!({"code": "base", "params": {"source": ""}, "label": "Main roads, towns and water", "status": "Shown at every zoom level"});
    let meta = "CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT);";
    let bldg = "CREATE TABLE bldg(x INT, y INT, part INT, n INT, data BLOB, PRIMARY KEY(x,y,part)) WITHOUT ROWID;";
    let terrain = "CREATE TABLE tile(z INT, x INT, y INT, png BLOB, PRIMARY KEY(z,x,y)) WITHOUT ROWID; INSERT INTO tile VALUES (11, 1, 1, x'00');";
    let mut said: Vec<String> = Vec::new();
    let mut seen = |info: &Value| said.extend(info["datasets"].as_array().unwrap().iter().map(|d| d["code"].as_str().unwrap().to_string()));

    // a region: its own box; buildings that name no source are OpenStreetMap's
    let f = fx("info-region");
    vaduz(&f);
    places_db(&f, &[("Vaduz", "town", "Liechtenstein", 47.14, 9.52, 2, &[])]);
    f.sqlite("derived/geo/buildings.sqlite", bldg);
    f.sqlite("derived/geo/crops.sqlite", &format!("{RESOURCE_TABLES} {meta} INSERT INTO meta VALUES ('source', 'EUCROPMAP 2022');"));
    let info = f.json("/api/map/info");
    assert_eq!(info["datasets"], json!([base,
        {"code": "streets_detail", "params": {}, "label": "Streets, places and help points (OpenStreetMap) for the area this map details",
         "status": "Street detail stops at the edge of this area."},
        {"code": "places", "params": {}, "label": "Place names and postcodes", "status": "Used by \"Find a place\""},
        {"code": "buildings_detail", "params": {"source": "OpenStreetMap"}, "label": "Building outlines (OpenStreetMap)",
         "status": "For the area this map details, shown from street level zoom"},
        {"code": "crops_detail", "params": {"source": "EUCROPMAP 2022"}, "label": "Orchards, vineyards and olive groves (EUCROPMAP 2022)",
         "status": "For the area this map details"},
    ]));
    seen(&info);

    // everything: help points from one end of the map to the other; a source left unnamed is '' and the English says none
    let f = fx("info-all");
    let c = f.sqlite("derived/geo/map.sqlite", MAP_TABLES);
    c.execute_batch("INSERT INTO poi VALUES (1, 'hospital', 'West', -20.0, 40.0), (2, 'hospital', 'East', 80.0, 60.0);").unwrap();
    drop(c);
    f.sqlite("derived/geo/terrain.sqlite", terrain);
    f.sqlite("derived/geo/buildings.sqlite", &format!("{bldg} {meta} INSERT INTO meta VALUES ('source', 'Overture Maps Foundation');"));
    f.sqlite("derived/geo/crops.sqlite", RESOURCE_TABLES);
    f.sqlite("derived/geo/addresses/AT.sqlite", meta);
    f.sqlite("derived/geo/addresses/DE-BY.sqlite", &format!("{meta} INSERT INTO meta VALUES ('publisher', 'Bayerische Vermessungsverwaltung');"));
    let info = f.json("/api/map/info");
    assert!(info["bounds"].is_null(), "a map of everything");
    assert_eq!(info["datasets"], json!([base,
        {"code": "streets_whole", "params": {}, "label": "Streets, places and help points (OpenStreetMap) for the whole map", "status": "Shown when you zoom in"},
        {"code": "addresses", "params": {"source": "", "regions": ["AT", "DE-BY"], "region_names": ["Austria", "Bayern"]},
         "label": "Street addresses", "status": "Used by \"Find a place\": house numbers in Austria, Bayern"},
        {"code": "elevation_contours", "params": {"source": ""}, "label": "Elevation", "status": "Shaded relief. Contour lines only where this map has fine detail."},
        {"code": "buildings_whole", "params": {"source": "Overture Maps Foundation"}, "label": "Building outlines (Overture Maps Foundation)",
         "status": "For the whole map, shown from street level zoom"},
        {"code": "crops_whole", "params": {"source": ""}, "label": "Orchards, vineyards and olive groves", "status": "For the whole map"},
    ]), "the first index's publisher names the source");
    seen(&info);

    // the base map only: elevation at broad zoom levels
    let f = fx("info-base");
    f.sqlite("derived/geo/terrain.sqlite", &format!("{terrain} {meta} INSERT INTO meta VALUES ('source', 'Copernicus DEM GLO-90');"));
    let info = f.json("/api/map/info");
    assert_eq!(info["datasets"], json!([base,
        {"code": "no_detail", "params": {}, "label": "No street detail or help points",
         "status": "This download has only the base map. Hospitals, water taps and side streets are not in it."},
        {"code": "elevation_broad", "params": {"source": "Copernicus DEM GLO-90"}, "label": "Elevation (Copernicus DEM GLO-90)", "status": "Shaded relief at broad zoom levels"},
    ]));
    seen(&info);
    said.sort();
    said.dedup();
    let mut all: Vec<String> = info::CODES.iter().map(|c| c.to_string()).collect();
    all.sort();
    assert_eq!(said, all, "every code is said by some map set");
}

#[test]
fn every_code_has_words_in_the_interface() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/locales/en.json");
    let words: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for code in info::CODES {
        for key in [format!("map.info.{code}"), format!("map.info.{code}_status")] {
            assert!(words[&key].is_string(), "web/locales/en.json has no {key}");
        }
    }
    let keys = info::CREDIT_CODES.iter().map(|c| format!("map.credit.{c}"))
        .chain(places::PLACE_CODES.iter().map(|c| format!("map.place.{c}")))
        .chain(pins::SUMMARY_CODES.iter().map(|c| format!("map.pin.{c}")));
    for key in keys {
        assert!(words[&key].is_string(), "web/locales/en.json has no {key}");
    }
}

#[test]
fn vector_tiles() {
    let f = fx("tiles");
    assert_eq!(f.json("/api/map/tile/14/1/1"), json!({"error": "map not built"}));
    vaduz(&f);
    for z in [2, 19, 25] {
        assert_eq!(f.json(&format!("/api/map/tile/{z}/0/0")), json!({"z": z, "layers": {}}));
    }
    let (x, y) = tile_of(14, VADUZ.0, VADUZ.1);
    let r = f.get(&format!("/api/map/tile/14/{x}/{y}?v=n7"));
    assert_eq!(r.header("Cache-Control"), Some("max-age=86400"));
    assert_eq!(f.get(&format!("/api/map/tile/14/{x}/{y}")).header("Cache-Control"), Some("max-age=60"));
    let t: Value = serde_json::from_slice(&r.body).unwrap();
    assert_eq!(t["z"], 14);
    let road = &t["layers"]["road"][0];
    assert_eq!(road[0], "primary");
    assert_eq!(road[1], "Städtle");
    let pts = road[2][0].as_array().unwrap();
    assert!(pts.len() >= 4 && pts.len() % 2 == 0);
    let place = &t["layers"]["place"][0];
    assert_eq!(place[1], "Vaduz");
    let (lon0, _, lon1, _) = tile_bounds(14, x, y);
    let px = place[2][0][0].as_f64().unwrap();
    assert!((px - (VADUZ.1 - lon0) / (lon1 - lon0) * EXT).abs() < 0.06, "projected to the tile's 512 units");
    assert_eq!(px, (px * 10.0).round() / 10.0, "one decimal, as the original Lastbook wrote them");
    assert!(t["layers"]["water"][0][1].is_null());
    assert!(t["layers"].get("bldg").is_none(), "no buildings below zoom 15");
    // zoomed out, once the national map is complete, a tile is kept on disk under the data version
    let (x8, y8) = tile_of(8, VADUZ.0, VADUZ.1);
    let first = f.get(&format!("/api/map/tile/8/{x8}/{y8}")).body;
    assert!(f.file(&format!("derived/geo/tilecache/n7/8/{x8}/{y8}.json")).is_file());
    assert_eq!(f.get(&format!("/api/map/tile/8/{x8}/{y8}")).body, first);
}

#[test]
fn clipping_and_unpacking() {
    let ring = vec![(-100.0, -100.0), (600.0, -100.0), (600.0, 600.0), (-100.0, 600.0)];
    let c = tiles::clip_ring(ring, -16.0, -16.0, 528.0, 528.0);
    assert_eq!(c.len(), 4);
    assert!(c.iter().all(|p| (-16.0..=528.0).contains(&p.0) && (-16.0..=528.0).contains(&p.1)));
    assert!(tiles::clip_ring(vec![(1000.0, 1000.0), (1100.0, 1000.0), (1100.0, 1100.0)], 0.0, 0.0, 10.0, 10.0).is_empty());
    let parts = vec![vec![(9.5, 47.1), (9.6, 47.2)], vec![(-3.7, 40.4)]];
    assert_eq!(tiles::unpack(&pack(&parts)), parts);
    assert!(tiles::unpack(&[1, 2, 3]).is_empty());
    assert_eq!(tiles::unpack(&pack(&parts)[..20]).len(), 2, "a cut blob gives what it holds, never a panic");
}

fn bucket(outlines: &[(i32, Vec<(i32, i32)>)]) -> Vec<u8> {
    let mut a: Vec<i32> = Vec::new();
    for (h, pts) in outlines {
        a.extend([pts.len() as i32, *h, pts[0].0, pts[0].1]);
        for w in pts.windows(2) {
            a.extend([w[1].0 - w[0].0, w[1].1 - w[0].1]);
        }
    }
    let raw: Vec<u8> = a.iter().flat_map(|v| v.to_le_bytes()).collect();
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut e, &raw).unwrap();
    e.finish().unwrap()
}

#[test]
fn buildings_at_street_zoom_and_in_3d() {
    let f = fx("bldg");
    vaduz(&f);
    let (x16, y16) = tile_of(16, VADUZ.0, VADUZ.1);
    let (x14, y14) = (x16 >> 2, y16 >> 2);
    // an outline in the middle of the zoom-16 tile, in zoom-30 pixels
    let (cx, cy) = ((x16 << 14) + 8000, (y16 << 14) + 8000);
    let square = vec![(cx as i32, cy as i32), (cx as i32 + 600, cy as i32), (cx as i32 + 600, cy as i32 + 600), (cx as i32, cy as i32 + 600)];
    let blob = bucket(&[(95, square)]);
    let c = f.sqlite("derived/geo/buildings.sqlite", "CREATE TABLE bldg(x INT, y INT, part INT, n INT, data BLOB, PRIMARY KEY(x,y,part)) WITHOUT ROWID;
        CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT);");
    c.execute("INSERT INTO bldg VALUES (?1,?2,0,1,?3)", params![x14, y14, blob]).unwrap();
    drop(c);
    let t = f.json(&format!("/api/map/tile/16/{x16}/{y16}"));
    let b = t["layers"]["bldg"].as_array().unwrap();
    assert!(b.iter().any(|f| f[0] == "building" && f[1].is_null() && f[2][0].as_array().unwrap().len() == 8), "{b:?}");
    let r = f.get(&format!("/api/map/bldg3d/{x14}/{y14}"));
    assert_eq!(r.header("Content-Type"), Some("application/octet-stream"));
    let ints: Vec<i32> = r.body.chunks(4).map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    assert_eq!(ints[0], 10, "4 + 3 point deltas");
    assert_eq!(&ints[1..3], &[4, 95]);
    assert_eq!(ints.len(), 11, "no bldg_osm table: nothing after the footprints");
    assert_eq!(f.get("/api/map/bldg3d/1/1").body, vec![0, 0, 0, 0]);
    assert!(fx("nobldg").get("/api/map/bldg3d/1/1").body.is_empty());
}

#[test]
fn image_tiles_and_their_gaps() {
    let f = fx("img");
    let mut h = vec![0f32; 65536];
    for (i, v) in h.iter_mut().enumerate() {
        *v = 400.0 + (i % 256) as f32; // rises to the east
    }
    let png = terrain::terrain_png(&h);
    assert_eq!(&png[1..4], b"PNG");
    let back = terrain::heights(&png).unwrap();
    assert!((back[300] - h[300]).abs() < 0.06);
    let (x, y) = tile_of(12, VADUZ.0, VADUZ.1);
    let c = f.sqlite("derived/geo/terrain.sqlite", "CREATE TABLE tile(z INT, x INT, y INT, png BLOB, PRIMARY KEY(z,x,y)) WITHOUT ROWID;");
    c.execute("INSERT INTO tile VALUES (12,?1,?2,?3)", params![x, y, png]).unwrap();
    drop(c);
    let r = f.get(&format!("/api/map/terrain/12/{x}/{y}.png?v=n7"));
    assert_eq!((r.status, r.header("Content-Type"), r.header("Cache-Control")), (200, Some("image/png"), Some("max-age=86400")));
    assert_eq!(r.body, png);
    let gap = f.get("/api/map/terrain/12/1/1.png");
    assert_eq!((gap.status, gap.header("Cache-Control")), (204, Some("max-age=3600")));
    assert!(gap.body.is_empty());
    assert_eq!(f.get("/api/map/terrain/14/1/1.png").header("Cache-Control"), Some("no-store"), "detail may still be building");
    let crop = f.get("/api/map/crops/12/1/1.png");
    assert_eq!((crop.status, crop.header("Cache-Control")), (204, Some("max-age=3600")));
    let aerial = f.get("/api/map/aerial/10/176/409");
    assert_eq!((aerial.status, aerial.header("Cache-Control")), (204, Some("no-store")), "an optional layer: no file, no photo");
    assert_eq!(f.get("/api/map/terrain/12/1/x.png").status, 404);
    let m = f.map.elevation(VADUZ.0, VADUZ.1).unwrap();
    assert!((400.0..656.0).contains(&m), "{m}");
    assert!(f.map.elevation(10.0, 10.0).is_none());
}

#[test]
fn help_pins_members_and_nearest() {
    let f = fx("pins");
    vaduz(&f);
    let (la, lo) = VADUZ;
    let q = format!("west={}&south={}&east={}&north={}", lo - 0.05, la - 0.05, lo + 0.05, la + 0.05);
    let p = f.json(&format!("/api/map/pins?{q}&zoom=17&kinds=hospital,pharmacy,nonsense"));
    assert_eq!(p["sites"], 3, "two pharmacies and a hospital; unknown kinds are ignored");
    assert_eq!(p["cell"], json!([1e-6, 1e-6]));
    let one = p["points"].as_array().unwrap().iter().find(|c| c["cls"] == "hospital").unwrap();
    assert_eq!((one["count"].as_i64(), one["label"].as_str(), one["name"].as_str()), (Some(1), Some("Hospital"), Some("Landesspital")));
    let z10 = f.json(&format!("/api/map/pins?{q}&zoom=10&kinds=hospital,pharmacy"));
    let top = &z10["points"][0];
    assert_eq!(top["count"], 3, "one cluster at zoom 10: {z10}");
    assert_eq!(top["cls"], "pharmacy", "the kind with most sites");
    assert!(top["name"].is_null() && top.get("label").is_none());
    assert_eq!(top["kinds"], json!({"hospital": 1, "pharmacy": 2}));
    let m = f.json(&format!("/api/map/members?{q}&lat={la}&lon={lo}&kinds=pharmacy,hospital"));
    assert_eq!(m["total"], 2, "the pharmacy mapped twice counts once: {m}");
    assert_eq!(m["items"][0]["name"], "Apotheke am Städtle");
    let near = f.json(&format!("/api/map/near?lat={la}&lon={lo}&cls=pharmacy,chemist,hospital&limit=5"));
    let near = near.as_array().unwrap();
    assert_eq!(near.len(), 3, "{near:?}");
    assert_eq!(near[0]["label"], "Pharmacy");
    assert_eq!(near[0]["dir"], "north-east");
    let chemist = near.iter().find(|p| p["cls"] == "chemist").unwrap();
    assert_eq!((chemist["name"].as_str(), chemist["label"].as_str()), (Some("Drogerie"), Some("Drugstore")), "OpenStreetMap's shop=chemist is a drugstore, not a pharmacy");
    assert_eq!(help("chemist"), Some("Drugstore"));
    // pharmacies alone: amenity=pharmacy, no drugstore among them
    let pharmacies = f.json(&format!("/api/map/near?lat={la}&lon={lo}&cls=pharmacy&limit=5"));
    assert_eq!(pharmacies.as_array().unwrap().iter().map(|p| (p["cls"].as_str().unwrap(), p["name"].as_str().unwrap())).collect::<Vec<_>>(),
               [("pharmacy", "Apotheke am Städtle")], "the pharmacy mapped twice counts once, and the drugstore is not one");
    assert!(near.windows(2).all(|w| w[0]["km"].as_f64() <= w[1]["km"].as_f64()));
    assert_eq!(near[0]["km"].as_f64().unwrap(), pins::round_to(near[0]["km"].as_f64().unwrap(), 2));
    let default = f.json(&format!("/api/map/near?lat={la}&lon={lo}"));
    assert!(default.as_array().unwrap().iter().all(|p| ["hospital", "clinic", "pharmacy", "fire_station", "drinking_water"].contains(&p["cls"].as_str().unwrap())));
    assert_eq!(f.get("/api/map/near?lat=x&lon=1").status, 400);
    assert_eq!(f.get("/api/map/pins?west=1").status, 400);
    assert_eq!(f.get("/api/map/nothing").status, 404);
    // the assistant's own calls
    assert_eq!(f.map.near(la, lo, &["fire_station"], 3)[0]["name"], "Feuerwehr");
    assert_eq!(help("dam"), Some("Dam"));
}

/// The assistant looks up the pharmacies near a place (lookup_place, nearby "pharmacy": the kind the model is told to ask
/// for, whatever the person's language calls it, farmacia, Apotheke, pharmacie, apteka) on the open library's own map:
/// it gets the pharmacy, never the drugstore beside it; a drugstore is found only when asked for by its own kind.
#[test]
fn the_assistant_asking_for_pharmacies_gets_no_drugstores() {
    use crate::ai::assistant::{run_tool, Knowledge, Session};
    let f = fx("assistant-pharmacy");
    vaduz(&f);
    places_db(&f, &[("Vaduz", "town", "Liechtenstein", VADUZ.0, VADUZ.1, 2, &[])]);
    std::fs::write(f.file("pack.json"), r#"{"name": "LI", "map": "LI"}"#).unwrap(); // a pack's map: the places are on it
    let paths = Paths::new(f.dir.join("app"), Some(f.dir.join("data")));
    let k = crate::knowledge::EngineKnowledge { library: Arc::new(crate::library::Library::new(paths.clone())), map: Arc::new(Map::new(paths)) };
    assert!(k.help_kinds().contains(&"pharmacy".to_string()) && k.help_kinds().contains(&"chemist".to_string()));
    let ask = |nearby: &str| {
        let mut s = Session::default();
        let args = json!({"query": "Vaduz", "nearby": nearby}).as_object().unwrap().clone();
        let (text, _) = run_tool("lookup_place", &mut args.clone(), &mut s, &k, "it");
        let found: Vec<(String, String)> = s.all().iter().filter(|x| x["code"] == "near")
            .map(|x| (x["params"]["cls"].as_str().unwrap_or("").to_string(), x["title"].as_str().unwrap_or("").to_string())).collect();
        (text, found)
    };
    let (text, found) = ask("pharmacy");
    assert_eq!(found, [("pharmacy".to_string(), "Apotheke am Städtle".to_string())], "{text}");
    assert!(!text.contains("Drogerie") && text.contains("Pharmacy: Apotheke am Städtle"), "{text}");
    let (text, found) = ask("chemist");
    assert_eq!(found, [("chemist".to_string(), "Drogerie".to_string())]);
    assert!(text.contains("Drugstore: Drogerie"), "named a drugstore to the model too: {text}");
}

#[test]
fn water_and_crop_resources_in_metric() {
    let f = fx("res");
    let w = f.sqlite("derived/geo/water.sqlite", RESOURCE_TABLES);
    resource(&w, 1, "lake", "Steger See", 47.1, 9.6, r#"{"area_ha": 3.2, "water_type": "fresh"}"#, "Amt für Umwelt");
    resource(&w, 2, "dam", "Saminatal", 47.12, 9.62, r#"{"storage_m3": 1500000, "purpose": "hydropower"}"#, "Amt für Umwelt");
    drop(w);
    let c = f.sqlite("derived/geo/crops.sqlite", RESOURCE_TABLES);
    resource(&c, 1, "vineyard", "", 47.13, 9.52, r#"{"crop": "Grapes", "crop_acres": 10}"#, "crop map");
    drop(c);
    let near = f.map.near(47.1, 9.6, &["lake", "dam", "vineyard"], 3);
    assert_eq!(near[0]["detail"], "3.2 ha · fresh");
    assert_eq!(near[0]["detail_parts"], json!([{"code": "area_ha", "params": {"ha": 3.2}, "text": "3.2 ha"}, {"text": "fresh"}]),
               "the area a code with its number, for the interface to write; the record's own word as it is");
    let dam = near.iter().find(|p| p["cls"] == "dam").unwrap();
    assert_eq!(dam["detail"], "hydropower · 1,500,000 m³ stored");
    assert_eq!(dam["detail_parts"], json!([{"text": "hydropower"}, {"code": "stored_m3", "params": {"m3": 1500000}, "text": "1,500,000 m³ stored"}]));
    let vine = near.iter().find(|p| p["cls"] == "vineyard").unwrap();
    assert_eq!(vine["detail"], "Grapes · about 4 ha in this block", "acres become hectares");
    assert_eq!(vine["detail_parts"], json!([{"text": "Grapes"}, {"code": "crop_block_ha", "params": {"ha": 4.0}, "text": "about 4 ha in this block"}]));
    let all = near.iter().map(|p| p.to_string()).collect::<String>();
    assert!(!all.contains("acre") && !all.contains(" mi"));
    let pins = f.json("/api/map/pins?west=9.4&south=47&east=9.7&north=47.3&zoom=17&kinds=lake,dam");
    assert_eq!(pins["sites"], 2);
    let lake = pins["points"].as_array().unwrap().iter().find(|p| p["cls"] == "lake").unwrap();
    assert_eq!((lake["summary"].as_str(), &lake["summary_parts"]), (Some("3.2 ha · fresh"), &near[0]["detail_parts"]), "a pin of one record: the same parts");
    let m = f.json("/api/map/members?west=9.59&south=47.09&east=9.61&north=47.11&lat=47.1&lon=9.6&kinds=lake");
    assert_eq!((m["items"][0]["summary"].as_str(), &m["items"][0]["summary_parts"]), (Some("3.2 ha · fresh"), &near[0]["detail_parts"]), "and a record of a cluster");
    assert_eq!(resource_summary("reservoir", &json!({"area_acres": 100})), "40 ha");
    // the library's atlas holds none here
    assert_eq!(f.json("/api/map/resources?west=9&south=47&east=10&north=48"), json!({"points": [], "records": 0}));
}

/// Every part of a resource's line, as codes with their params beside the English: a pond of 0.2 ha (the number the
/// interface writes "0,2" in Italian), a lake of whole hectares, a pond that is NOT drinking water, a dam's storage, a
/// crop block, a fish stocking. Between them, every code is said.
#[test]
fn resource_lines_as_codes() {
    let f = fx("res-codes");
    let w = f.sqlite("derived/geo/water.sqlite", RESOURCE_TABLES);
    resource(&w, 1, "pond", "", 47.153, 9.515, r#"{"area_ha": 0.2, "region": "Vaduz", "country": "Liechtenstein"}"#, "OpenStreetMap");
    resource(&w, 2, "lake", "Bodensee", 47.6, 9.4, r#"{"area_ha": 53600}"#, "OpenStreetMap");
    resource(&w, 3, "pond", "Klärteich", 47.16, 9.52, r#"{"area_ha": 1.26, "not_drinking_water": true}"#, "OpenStreetMap");
    resource(&w, 4, "dam", "Steg", 47.11, 9.57, r#"{"normal_storage_af": 1000}"#, "OpenStreetMap");
    drop(w);
    let c = f.sqlite("derived/geo/crops.sqlite", RESOURCE_TABLES);
    resource(&c, 1, "vineyard", "", 47.13, 9.52, r#"{"crop_ha": 0.6}"#, "crop map");
    drop(c);
    let a = f.sqlite("library-data/geography/resources/atlas.sqlite", RESOURCE_TABLES);
    resource(&a, 1, "fish_stocking", "Gampriner Seele", 47.22, 9.51, r#"{"FishType": "Trout"}"#, "stocking list");
    drop(a);
    let near = f.map.near(47.153, 9.515, &["pond", "lake", "dam", "vineyard", "fish_stocking"], 3);
    let of = |name: &str, cls: &str| near.iter().find(|p| p["cls"] == cls && p["name"].as_str().unwrap_or("") == name).unwrap_or_else(|| panic!("{name}: {near:?}"));
    let pond = of("", "pond");
    assert_eq!(pond["detail"], "0.2 ha");
    assert_eq!(pond["detail_parts"], json!([{"code": "area_ha", "params": {"ha": 0.2}, "text": "0.2 ha"}]));
    let lake = of("Bodensee", "lake");
    assert_eq!(lake["detail"], "53,600 ha");
    assert_eq!(lake["detail_parts"], json!([{"code": "area_ha", "params": {"ha": 53600}, "text": "53,600 ha"}]), "whole hectares from 10 up");
    let waste = of("Klärteich", "pond");
    assert_eq!(waste["detail"], "1.3 ha · NOT drinking water");
    assert_eq!(waste["detail_parts"], json!([{"code": "area_ha", "params": {"ha": 1.3}, "text": "1.3 ha"}, {"code": "not_drinking_water", "params": {}, "text": "NOT drinking water"}]));
    let dam = of("Steg", "dam");
    assert_eq!(dam["detail"], "1,233,480 m³ stored", "acre-feet become cubic metres");
    assert_eq!(dam["detail_parts"], json!([{"code": "stored_m3", "params": {"m3": 1233480}, "text": "1,233,480 m³ stored"}]));
    let vine = of("", "vineyard");
    assert_eq!(vine["detail"], "about 0.6 ha in this block");
    assert_eq!(vine["detail_parts"], json!([{"code": "crop_block_ha", "params": {"ha": 0.6}, "text": "about 0.6 ha in this block"}]));
    let fish = of("Gampriner Seele", "fish_stocking");
    assert_eq!(fish["detail"], "Trout · historical stocking record");
    assert_eq!(fish["detail_parts"], json!([{"text": "Trout"}, {"code": "stocking_record", "params": {}, "text": "historical stocking record"}]));
    let mut said: Vec<String> = near.iter().flat_map(|p| p["detail_parts"].as_array().unwrap().iter().filter_map(|d| d["code"].as_str().map(str::to_string))).collect();
    said.sort();
    said.dedup();
    let mut all: Vec<String> = pins::SUMMARY_CODES.iter().map(|c| c.to_string()).collect();
    all.sort();
    assert_eq!(said, all, "every code is said");
}

#[test]
fn compass_and_distance() {
    let (d, dir) = haversine(47.1410, 9.5209, 46.9480, 7.4474);
    assert!((d - 159.0).abs() < 3.0, "Vaduz to Bern: {d}");
    assert_eq!(dir, "west");
    assert_eq!(haversine(0.0, 0.0, 1.0, 0.0).1, "north");
    assert_eq!(haversine(0.0, 0.0, -1.0, -1.0).1, "south-west");
}

#[test]
fn coverage_home_area_and_pack() {
    let f = fx("cov");
    std::fs::write(f.file("library-data/catalog.json"), serde_json::to_vec(&json!([
        {"id": "video-a", "kind": "video", "path": "videos/a.mp4", "captions": ["videos/a.vtt"]},
        {"id": "video-b", "kind": "video", "path": "videos/b.mp4"},
        {"id": "tx-b", "kind": "transcript", "path": "transcripts/b.txt", "video_path": "videos/b.mp4"},
        {"id": "video-c", "kind": "video", "path": "videos/c.mp4"},
        {"id": "video-x", "kind": "video", "path": "videos/x.mp4"},
        {"id": "pdf1", "kind": "pdf", "path": "documents/1.pdf", "searchability": "image-only scan"},
        {"id": "pdf2", "kind": "pdf", "path": "documents/2.pdf"}
    ])).unwrap()).unwrap();
    std::fs::write(f.file("library-data/video-exclusions.json"), r#"{"videos": {"video-x": "off topic"}}"#).unwrap();
    f.sqlite("derived/index/semantic.sqlite", "CREATE TABLE gaps(doc_id TEXT PRIMARY KEY, reason TEXT); INSERT INTO gaps VALUES ('pdf1', 'no text');");
    assert_eq!(f.map.coverage(), json!({"videos": 3, "videos_with_text": 2, "pdfs": 2, "pdfs_image_only": 1, "docs_without_text": 1, "remote_videos": 0, "videos_excluded": 1}));
    assert!(f.map.home_area().is_none());
    std::fs::write(f.dir.join("app/web/home-area.json"), r#"{"lat": 47.14, "lon": "9.52", "z": 13}"#).unwrap();
    assert_eq!(f.map.home_area(), Some((47.14, 9.52)));
}

/// A European places.sqlite: the search key of each place written with the engine's own text::place_key.
type Place<'a> = (&'a str, &'a str, &'a str, f64, f64, i64, &'a [&'a str]); // name, kind, detail, lat, lon, rank, other names

fn places_db(f: &Fx, rows: &[Place]) {
    let c = f.sqlite("derived/geo/places.sqlite", "CREATE TABLE place(id INTEGER PRIMARY KEY, name TEXT, kind TEXT, detail TEXT, lat REAL, lon REAL, rank INTEGER,
            source TEXT, country TEXT, key TEXT, area TEXT);
        CREATE VIRTUAL TABLE place_fts USING fts5(key, area, content='place', content_rowid='id', tokenize='unicode61 remove_diacritics 0');");
    for (name, kind, detail, lat, lon, rank, other) in rows {
        let names: Vec<&str> = std::iter::once(*name).chain(other.iter().copied()).collect();
        c.execute("INSERT INTO place(name,kind,detail,lat,lon,rank,source,country,key,area) VALUES (?1,?2,?3,?4,?5,?6,'OpenStreetMap','',?7,?8)",
                  params![name, kind, detail, lat, lon, rank, text::place_key(&names), text::place_key(&[detail])]).unwrap();
    }
    c.execute_batch("INSERT INTO place_fts(place_fts) VALUES ('rebuild')").unwrap();
}

#[test]
fn place_names_with_accents_and_both_spellings() {
    let f = fx("places");
    places_db(&f, &[("München", "city", "Bayern, Deutschland", 48.137, 11.575, 1, &[]), ("Zürich", "city", "Zürich, Schweiz", 47.377, 8.540, 1, &[]),
                    ("Łódź", "city", "Łódzkie, Polska", 51.759, 19.457, 1, &[]), ("Straßwalchen", "town", "Salzburg, Österreich", 47.98, 13.25, 2, &[]),
                    ("Sankt Gallen", "city", "St. Gallen, Schweiz", 47.424, 9.377, 1, &["St. Gallen"]), ("Tromsø", "city", "Troms, Norge", 69.649, 18.955, 1, &[]),
                    ("Bruxelles", "city", "Bruxelles, Belgique", 50.846, 4.352, 1, &["Brussel", "Brüssel"]), ("1012 AB", "Postcode", "Amsterdam, Nederland", 52.37, 4.89, 2, &["1012AB"]),
                    ("Müllheim", "town", "Baden-Württemberg", 47.81, 7.63, 2, &[]), ("Muenchenbuchsee", "village", "Bern, Schweiz", 47.02, 7.45, 3, &[])]);
    for (q, want) in [("München", "München"), ("Munchen", "München"), ("Muenchen", "München"), ("MÜNCHEN", "München"), ("Zuerich", "Zürich"), ("Zurich", "Zürich"),
                      ("Lodz", "Łódź"), ("Łódź", "Łódź"), ("Strasswalchen", "Straßwalchen"), ("Straßwalchen", "Straßwalchen"), ("St. Gallen", "Sankt Gallen"),
                      ("Sankt Gallen", "Sankt Gallen"), ("Tromso", "Tromsø"), ("Tromsoe", "Tromsø"), ("Brussel", "Bruxelles"), ("Bruessel", "Bruxelles"),
                      ("1012 AB", "1012 AB"), ("1012ab", "1012 AB"), ("Muellheim", "Müllheim"), ("München Bayern", "München")] {
        let r = f.map.places(q, 20, None);
        assert_eq!(r.first().and_then(|p| p["name"].as_str()), Some(want), "{q}: {r:?}");
    }
    assert!(f.map.places("Münchenbuchsee", 20, None).iter().any(|p| p["name"] == "Muenchenbuchsee"), "and the other way round");
    let r = f.map.places("Munchen", 20, None);
    assert_eq!((r[0]["kind"].as_str(), r[0]["detail"].as_str(), r[0]["onMap"].as_bool()), (Some("city"), Some("Bayern, Deutschland"), Some(false)));
    assert_eq!(f.get("/api/places?q=Munchen&near=48.1,11.5").header("X-Query-Kind"), Some("place"));
}

#[test]
fn an_older_place_index_still_answers() {
    // places.sqlite as the original Lastbook's builders wrote it: unicode61 alone, no key column
    let f = fx("places-old");
    let c = f.sqlite("derived/geo/places.sqlite", "CREATE TABLE place(id INTEGER PRIMARY KEY, name TEXT, kind TEXT, detail TEXT, lat REAL, lon REAL, rank INTEGER, source TEXT);
        CREATE VIRTUAL TABLE place_fts USING fts5(name, detail, content='place', content_rowid='id', tokenize='unicode61');
        INSERT INTO place(name,kind,detail,lat,lon,rank,source) VALUES ('München','city','Bayern',48.137,11.575,1,'x'), ('Straßwalchen','town','Salzburg',47.98,13.25,2,'x');
        INSERT INTO place_fts(place_fts) VALUES ('rebuild');");
    drop(c);
    for (q, want) in [("München", "München"), ("Munchen", "München"), ("Muenchen", "München"), ("Strasswalchen", "Straßwalchen")] {
        assert_eq!(f.map.places(q, 20, None).first().and_then(|p| p["name"].as_str()), Some(want), "{q}");
    }
}

#[test]
fn lakes_and_dams_by_name() {
    let f = fx("water");
    let c = f.sqlite("derived/geo/water.sqlite", &format!("{RESOURCE_TABLES} CREATE VIRTUAL TABLE water_fts USING fts5(key, tokenize='unicode61 remove_diacritics 0');"));
    for (id, kind, name, lat, lon, detail) in [(1, "lake", "Lago di Garda", 45.6, 10.6, r#"{"area_ha": 36800, "region": "Lombardia", "country": "Italia"}"#),
                                               (2, "lake", "Wörthersee", 46.62, 14.15, r#"{"area_ha": 1939, "country": "Österreich"}"#),
                                               (3, "lake", "Lac Léman", 46.45, 6.55, r#"{"area_ha": 58000, "other_names": "Genfersee"}"#),
                                               (4, "dam", "Diga del Vajont", 46.27, 12.33, r#"{"country": "Italia"}"#)] {
        resource(&c, id, kind, name, lat, lon, detail, "test");
        let other = serde_json::from_str::<Value>(detail).unwrap()["other_names"].as_str().unwrap_or("").to_string();
        c.execute("INSERT INTO water_fts(rowid, key) VALUES (?1, ?2)", params![id, text::place_key(&[name, &other])]).unwrap();
    }
    drop(c);
    for (q, want) in [("Garda", "Lago di Garda"), ("Lago di Garda", "Lago di Garda"), ("Woerthersee", "Wörthersee"), ("Worthersee", "Wörthersee"), ("lac leman", "Lac Léman"),
                      ("Genfersee", "Lac Léman"), ("Vajont", "Diga del Vajont")] {
        assert_eq!(f.map.places(q, 20, None).first().and_then(|p| p["name"].as_str()), Some(want), "{q}");
    }
    let r = f.map.water_places("Garda", 10, Some((45.4, 10.9)));
    assert_eq!((r[0]["kind"].as_str(), r[0]["source"].as_str()), (Some("Lake"), Some("water")));
    let detail = r[0]["detail"].as_str().unwrap();
    assert!(detail.starts_with("Lombardia, Italia · ") && detail.ends_with(" km away"), "{detail}");
    assert_eq!(f.map.places("Vajont", 20, None)[0]["kind"], "Dam");
    assert!(f.map.water_places("lago", 10, None).is_empty(), "a word for water alone names no lake");
}

/// The line under a lake's or dam's name: where it is apart (its country by its code), and what more it says as codes
/// with their params, the English detail beside them. Between them, every code is said.
#[test]
fn a_lakes_line_as_codes() {
    let f = fx("water-codes");
    let c = f.sqlite("derived/geo/water.sqlite", &format!("{RESOURCE_TABLES} CREATE VIRTUAL TABLE water_fts USING fts5(key, tokenize='unicode61 remove_diacritics 0');"));
    for (id, kind, name, lat, lon, detail) in [(1, "lake", "Lago di Garda", 45.6, 10.6, r#"{"area_ha": 36800, "region": "Lombardia", "country": "Italia"}"#),
                                               (2, "lake", "Lac Léman", 46.45, 6.55, r#"{"area_ha": 58000, "other_names": "Genfersee", "country": "Switzerland"}"#),
                                               (3, "reservoir", "Stausee Steg", 47.1098, 9.5757, r#"{"region": "Triesenberg", "country": "Liechtenstein"}"#),
                                               (4, "dam", "Kölnbreinsperre", 47.08, 13.34, r#"{"region": "Malta", "country": "Republik Österreich"}"#)] {
        resource(&c, id, kind, name, lat, lon, detail, "test");
        let other = serde_json::from_str::<Value>(detail).unwrap()["other_names"].as_str().unwrap_or("").to_string();
        c.execute("INSERT INTO water_fts(rowid, key) VALUES (?1, ?2)", params![id, text::place_key(&[name, &other])]).unwrap();
    }
    drop(c);
    let one = |q: &str, near: Option<(f64, f64)>| f.map.water_places(q, 10, near).into_iter().next().unwrap_or_else(|| panic!("{q}"));
    let garda = one("Garda", Some((45.4, 10.9)));
    let km = garda["detail_parts"][0]["params"]["km"].as_i64().unwrap();
    assert!((25..40).contains(&km), "{garda}");
    assert_eq!(garda["detail"], format!("Lombardia, Italia · {km} km away"), "the English as before");
    assert_eq!((garda["locality"].as_str(), garda["country"].as_str()), (Some("Lombardia"), Some("IT")), "a country in its own language, by its code");
    assert_eq!(garda["detail_parts"], json!([{"code": "km_away", "params": {"km": km}, "text": format!("{km} km away")}]));
    let leman = one("Genfersee", None);
    assert_eq!(leman["detail"], "Switzerland · also called Genfersee");
    assert_eq!((leman["locality"].as_str(), leman["country"].as_str()), (Some(""), Some("CH")), "and in English");
    assert_eq!(leman["detail_parts"], json!([{"code": "also_called", "params": {"names": "Genfersee"}, "text": "also called Genfersee"}]));
    let steg = one("Stausee Steg", Some((47.11, 9.575)));
    assert_eq!(steg["detail"], "Triesenberg, Liechtenstein · nearby", "under a kilometre away");
    assert_eq!((steg["locality"].as_str(), steg["country"].as_str()), (Some("Triesenberg"), Some("LI")));
    assert_eq!(steg["detail_parts"], json!([{"code": "nearby", "params": {}, "text": "nearby"}]));
    let far = one("Kölnbreinsperre", Some((47.14, 9.52)));
    let km = haversine(47.14, 9.52, 47.08, 13.34).0.round_ties_even() as i64;
    assert!((280..300).contains(&km), "{km}");
    assert_eq!(far["detail"], format!("Malta, Republik Österreich · {km} km away"));
    assert_eq!((far["locality"].as_str(), far["country"].as_str()), (Some("Malta, Republik Österreich"), Some("")), "a country the engine does not know stays as its name");
    assert_eq!(far["detail_parts"], json!([{"code": "km_away", "params": {"km": km}, "text": format!("{km} km away")}]));
    let mut said: Vec<&str> = [&garda, &leman, &steg, &far].iter().flat_map(|p| p["detail_parts"].as_array().unwrap().iter().filter_map(|d| d["code"].as_str())).collect();
    said.sort_unstable();
    said.dedup();
    let mut all = places::PLACE_CODES.to_vec();
    all.sort_unstable();
    assert_eq!(said, all, "every code is said");
    assert_eq!(f.get("/api/places?q=Genfersee").header("X-Query-Kind"), Some("place"));
}

#[test]
fn no_southern_california_and_no_us_units_in_the_map_code() {
    let code = [include_str!("mod.rs"), include_str!("tiles.rs"), include_str!("pins.rs"), include_str!("info.rs"), include_str!("terrain.rs"), include_str!("places.rs"),
                 include_str!("address/mod.rs"), include_str!("address/countries.rs"), include_str!("address/words.rs")].concat();
    for gone in [concat!("118", ".4"), concat!("So", "Cal"), concat!("Southern ", "California"), concat!("socal", "_"), concat!(" mi", "les")] {
        assert!(!code.contains(gone), "{gone}");
    }
}
