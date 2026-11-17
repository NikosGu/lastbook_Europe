//! Where a region's sources are fetched from, for building it on GitHub's machines (.github/workflows/regions.yml,
//! packaging/regions/build-region.sh): its Geofabrik extracts (data/region-sources.json), its neighbours', the
//! Copernicus DEM tiles its terrain needs, and Natural Earth's six zips. Nothing here fetches anything: the
//! `region-sources` command prints the list and the script downloads it.

use crate::map::{base, geo};
use lastbook_engine::regions;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::PI;

const DATA: &str = include_str!("../../data/region-sources.json");
pub const DEM: &str = "https://copernicus-dem-30m.s3.amazonaws.com";
pub const NATURAL_EARTH: &str = "https://naciscdn.org/naturalearth/10m";
/// The addresses a region's index holds at least, unless the table says otherwise: an empty or broken index fails.
pub const MIN_ADDRESSES: u64 = 100;

#[derive(Debug, Clone)]
pub struct Entry {
    /// Geofabrik extracts, as paths under the site: `europe/italy/nord-ovest`.
    pub extracts: Vec<String>,
    /// The town the check looks for: the capital, or the region's main town.
    pub capital: String,
    pub min_addresses: u64,
    /// The neighbours' extracts, when the table names them (boxes alone would take Bayern for Liechtenstein).
    pub neighbours: Option<Vec<String>>,
}

fn data() -> Value {
    serde_json::from_str(DATA).expect("data/region-sources.json is JSON")
}

/// Every region's entry.
pub fn table() -> BTreeMap<String, Entry> {
    let strings = |v: &Value| -> Vec<String> { v.as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect() };
    data()["regions"].as_object().into_iter().flatten().map(|(code, e)| {
        (code.clone(), Entry { extracts: strings(&e["extracts"]), capital: e["capital"].as_str().unwrap_or("").to_string(),
                               min_addresses: e["min_addresses"].as_u64().unwrap_or(MIN_ADDRESSES), neighbours: e.get("neighbours").map(strings) })
    }).collect()
}

/// The extracts of the regions next to this one, its own left out: as the table names them, or else those of the
/// regions whose boxes, without their margin (where their land actually reaches), meet this region's boxes.
pub fn neighbours(region: &str, t: &BTreeMap<String, Entry>) -> Vec<String> {
    let Some(own) = t.get(region) else { return Vec::new() };
    if let Some(n) = &own.neighbours {
        return n.clone();
    }
    let mine = regions::boxes(region).unwrap_or(&[]);
    let m = regions::MARGIN;
    let meets = |b: &[f64; 4]| mine.iter().any(|x| b[0] + m <= x[2] && b[2] - m >= x[0] && b[1] + m <= x[3] && b[3] - m >= x[1]);
    let mut out = BTreeSet::new();
    for (code, e) in t {
        if code != region && regions::boxes(code).unwrap_or(&[]).iter().any(meets) {
            out.extend(e.extracts.iter().filter(|x| !own.extracts.contains(x)).cloned());
        }
    }
    out.into_iter().collect()
}

/// A Copernicus DEM GLO-30 tile's name, for the whole degree south-west of (lat, lon).
pub fn dem_name(lat: i32, lon: i32) -> String {
    format!("Copernicus_DSM_COG_10_{}{:02}_00_{}{:03}_00_DEM", if lat < 0 { 'S' } else { 'N' }, lat.abs(), if lon < 0 { 'W' } else { 'E' }, lon.abs())
}

/// The DEM tiles a region's terrain needs: every whole degree its boxes reach, widened to the zoom-9 terrain tiles over
/// them (a zoom-9 tile is written only where the DEM covers all of it). Tiles over open sea do not exist.
pub fn dem_tiles(boxes: &[[f64; 4]]) -> Vec<String> {
    let lat = |y: i64| (PI * (1.0 - 2.0 * y as f64 / 512.0)).sinh().atan().to_degrees();
    let mut cells = BTreeSet::new();
    for b in boxes {
        let (x0, x1, y0, y1) = geo::tile_range(9, b);
        let (w, e) = (x0 as f64 / 512.0 * 360.0 - 180.0, (x1 + 1) as f64 / 512.0 * 360.0 - 180.0);
        let (n, s) = (lat(y0), lat(y1 + 1));
        for la in s.floor() as i32..n.ceil() as i32 {
            for lo in w.floor() as i32..e.ceil() as i32 {
                cells.insert((la, lo));
            }
        }
    }
    cells.into_iter().map(|(la, lo)| dem_name(la, lo)).collect()
}

fn extract(base: &str, path: &str) -> Value {
    let last = path.rsplit('/').next().unwrap_or(path);
    json!({"path": path, "gpkg": format!("{base}/{path}-latest-free.gpkg.zip"), "pbf": format!("{base}/{path}-latest.osm.pbf"),
           "gpkg_file": format!("{last}-latest-free.gpkg.zip"), "pbf_file": format!("{last}-latest.osm.pbf")})
}

/// What to fetch for a region, and what its check expects.
pub fn sources(region: &str, with_neighbours: bool) -> Result<Value, String> {
    let t = table();
    let e = t.get(region).ok_or_else(|| format!("{region} is not in data/region-sources.json"))?;
    let boxes = regions::boxes(region).ok_or_else(|| format!("{region} is not a region"))?;
    let base = data()["geofabrik"].as_str().unwrap_or("https://download.geofabrik.de").to_string();
    let near = if with_neighbours { neighbours(region, &t) } else { Vec::new() };
    let ne: Vec<Value> = base::FILES.iter().map(|f| {
        let kind = if matches!(*f, "ne_10m_roads" | "ne_10m_populated_places_simple") { "cultural" } else { "physical" };
        json!({"name": f, "url": format!("{NATURAL_EARTH}/{kind}/{f}.zip")})
    }).collect();
    Ok(json!({
        "region": region, "name": regions::name(region), "capital": e.capital, "min_addresses": e.min_addresses,
        "extracts": e.extracts.iter().map(|p| extract(&base, p)).collect::<Vec<_>>(),
        "neighbours": near.iter().map(|p| extract(&base, p)).collect::<Vec<_>>(),
        "dem": dem_tiles(boxes).iter().map(|n| json!({"name": n, "url": format!("{DEM}/{n}/{n}.tif")})).collect::<Vec<_>>(),
        "natural_earth": ne,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_region_once_with_its_extracts_and_town() {
        let t = table();
        let codes: BTreeSet<&str> = regions::BOXES.iter().map(|(c, _)| *c).collect();
        assert_eq!(t.keys().map(String::as_str).collect::<BTreeSet<_>>(), codes, "the regions of regions.rs, no more, no fewer");
        let mut users: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for (code, e) in &t {
            assert!(!e.extracts.is_empty() && !e.capital.is_empty(), "{code}");
            for x in e.extracts.iter().chain(e.neighbours.iter().flatten()) {
                assert!(x.split('/').count() >= 2 && x.chars().all(|c| c.is_ascii_lowercase() || c == '-' || c == '/'), "{code}: {x}");
                assert!(x.starts_with("europe/") || x == "africa/canary-islands", "{code}: {x}");
            }
            for x in &e.extracts {
                users.entry(x).or_default().insert(regions::country(code));
            }
        }
        for (x, countries) in &users {
            assert_eq!(countries.len(), 1, "{x} is shared, and only within one country: {countries:?}");
        }
        assert_eq!(t["IT-25"].extracts, ["europe/italy/nord-ovest"], "Italy's parts are macro-regions, which the boxes clip");
        assert_eq!(t["FR-GES"].extracts.len(), 3, "France's are the old regions");
    }

    #[test]
    fn neighbours_dem_tiles_and_downloads() {
        let t = table();
        assert_eq!(neighbours("LI", &t), ["europe/switzerland", "europe/austria"], "as the table says");
        assert!(neighbours("MT", &t).is_empty(), "Sicily's box reaches Lampedusa, not Malta");
        // from the boxes without their margin: Switzerland and Italy's nord-est for Lombardy, its own nord-ovest not
        let lombardy = neighbours("IT-25", &t);
        assert!(lombardy.contains(&"europe/switzerland".to_string()) && lombardy.contains(&"europe/italy/nord-est".to_string()), "{lombardy:?}");
        assert!(!lombardy.contains(&"europe/italy/nord-ovest".to_string()) && !lombardy.contains(&"europe/italy/sud".to_string()), "{lombardy:?}");
        assert_eq!(dem_tiles(regions::boxes("LI").unwrap()), [dem_name(46, 9), dem_name(47, 9)]);
        assert_eq!(dem_name(47, 9), "Copernicus_DSM_COG_10_N47_00_E009_00_DEM");
        assert_eq!(dem_name(-1, -31), "Copernicus_DSM_COG_10_S01_00_W031_00_DEM");
        assert!(dem_tiles(regions::boxes("MT").unwrap()).contains(&dem_name(35, 14)));
        let s = sources("LI", true).unwrap();
        assert_eq!(s["extracts"][0]["pbf"], "https://download.geofabrik.de/europe/liechtenstein-latest.osm.pbf");
        assert_eq!(s["extracts"][0]["gpkg_file"], "liechtenstein-latest-free.gpkg.zip");
        assert_eq!(s["neighbours"].as_array().unwrap().len(), 2);
        assert_eq!(sources("LI", false).unwrap()["neighbours"], json!([]));
        assert_eq!(s["natural_earth"][0]["url"], "https://naciscdn.org/naturalearth/10m/cultural/ne_10m_roads.zip");
        assert_eq!((s["capital"].as_str(), s["min_addresses"].as_u64()), (Some("Vaduz"), Some(MIN_ADDRESSES)));
        assert!(sources("XX", false).is_err());
    }
}
