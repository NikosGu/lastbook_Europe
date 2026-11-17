//! The place finder's index, `derived/geo/places.sqlite` (docs/MAPS.md, "Search keys"): towns and villages, municipalities,
//! postcodes, mountain peaks and named help points of the region, each with the search keys the engine compares
//! (`map::text::place_key`), so every spelling of a name meets.
//!
//!   place(id, name, kind, detail, lat, lon, rank, source, country, key, area)
//!   place_fts USING fts5(key, area, content='place', content_rowid='id', tokenize='unicode61 remove_diacritics 0')
//!
//! rank: 1 a city or town, 2 a village, municipality or postcode, 3 a smaller place or a peak, 4 a named point.

use crate::map::addresses::Row as Address;
use crate::map::admin::Admin;
use crate::map::detail::POI_KEEP;
use crate::map::geo;
use crate::map::gpkg::{self, Kind as Geom};
use crate::map::osm::{Kind, Tags, Thing};
use lastbook_engine::map::address::countries;
use lastbook_engine::map::text::place_key;
use lastbook_engine::regions;
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Place {
    pub name: String,
    pub kind: String,
    pub detail: String,
    pub lat: f64,
    pub lon: f64,
    pub rank: i64,
    pub country: String,
    /// Other names it is known by (other languages, old names).
    pub other: Vec<String>,
    pub osm_id: Option<String>,
}

/// Place nodes, for their names in other languages.
pub fn wanted(kind: Kind, t: &[(&str, &str)]) -> bool {
    kind == Kind::Node && t.iter().any(|(k, _)| *k == "place") && t.iter().any(|(k, _)| *k == "name")
}

/// A place's other names: in other languages, and its alternative, old, official, short and local names.
pub fn other_names(t: &Tags) -> Vec<String> {
    let main = t.get("name").unwrap_or("");
    let mut out: Vec<String> = Vec::new();
    for (k, v) in &t.0 {
        let other = (k.starts_with("name:") && k.len() <= 8) || matches!(k.as_str(), "alt_name" | "old_name" | "official_name" | "short_name" | "loc_name" | "int_name");
        if other {
            for n in v.split(';').map(str::trim).filter(|n| !n.is_empty() && *n != main) {
                if !out.iter().any(|o| o == n) {
                    out.push(n.to_string());
                }
            }
        }
    }
    out
}

fn country_name(code: &str) -> String {
    regions::COUNTRIES.iter().find(|c| c.0 == code).map(|c| c.2.to_string()).unwrap_or_default()
}

/// The words that say where a place is: its municipality or district, and its country in its own languages and English.
fn area_words(detail: &str, country: &str) -> Vec<String> {
    let mut out = vec![detail.to_string()];
    if let Some(c) = regions::COUNTRIES.iter().find(|c| c.0 == country) {
        out.extend(c.1.split(" / ").map(str::to_string));
        out.push(c.2.to_string());
    }
    out
}

/// Every place of the region the place finder offers.
pub fn collect(g: &Connection, things: &[Thing], admin: &Admin, addresses: &[Address], region: &str) -> Vec<Place> {
    let own = regions::country(region);
    let inside = |lon: f64, lat: f64| regions::contains(region, lat, lon);
    let country_at = |lon: f64, lat: f64| if admin.in_country(lon, lat) { own.to_string() } else { String::new() };
    let detail = |name: &str, lon: f64, lat: f64| -> String {
        let country = country_at(lon, lat);
        let mut parts: Vec<String> = Vec::new();
        if let Some(t) = admin.town_at(lon, lat).filter(|t| *t != name) {
            parts.push(t.to_string());
        } else if let Some(d) = admin.district_at(lon, lat).filter(|d| *d != name) {
            parts.push(d.to_string());
        }
        if !country.is_empty() {
            parts.push(country_name(&country));
        }
        parts.join(", ")
    };
    let others: HashMap<String, Vec<String>> = things.iter().filter(|t| t.kind == Kind::Node && t.tags.has("place")).map(|t| (t.id.to_string(), other_names(&t.tags))).collect();
    let mut out: Vec<Place> = Vec::new();
    for r in gpkg::layer(g, "gis_osm_places_free", &["fclass", "name", "osm_id"]) {
        let (Some(name), Some(&(lon, lat))) = (r.col(1), r.parts.first().and_then(|p| p.first())) else { continue };
        if !inside(lon, lat) {
            continue;
        }
        let cls = r.col(0).unwrap_or("locality");
        let rank = match cls {
            "city" | "town" | "national_capital" => 1,
            "village" | "suburb" => 2,
            _ => 3,
        };
        let id = r.col(2).map(str::to_string);
        out.push(Place { name: name.into(), kind: cls.replace('_', " "), detail: detail(name, lon, lat), lat, lon, rank, country: country_at(lon, lat),
                         other: id.as_ref().and_then(|i| others.get(i)).cloned().unwrap_or_default(), osm_id: id });
    }
    for (name, rings) in &admin.towns {
        // a municipality is shown where its town of the same name is, else at its middle
        let at = out.iter().find(|p| &p.name == name && geo::contains(rings, p.lon, p.lat)).map(|p| (p.lon, p.lat)).or_else(|| geo::centroid(rings));
        let Some((lon, lat)) = at.filter(|&(x, y)| inside(x, y)) else { continue };
        let d = admin.district_at(lon, lat).map(|d| format!("{d}, {}", country_name(own))).unwrap_or_else(|| country_name(own));
        out.push(Place { name: name.clone(), kind: "municipality".into(), detail: d, lat, lon, rank: 2, country: own.into(), other: Vec::new(), osm_id: None });
    }
    // postcodes: the middle of their addresses, named after their most common town
    let mut codes: HashMap<(String, String), (f64, f64, usize, HashMap<String, usize>)> = HashMap::new();
    for a in addresses.iter().filter(|a| !a.postcode.is_empty()) {
        let e = codes.entry((a.country.clone(), a.postcode.clone())).or_default();
        e.0 += a.lat;
        e.1 += a.lon;
        e.2 += 1;
        *e.3.entry(a.city.clone()).or_default() += 1;
    }
    let mut codes: Vec<_> = codes.into_iter().collect();
    codes.sort_by(|a, b| a.0.cmp(&b.0));
    for ((country, pc), (lat, lon, n, towns)) in codes {
        let town = towns.into_iter().filter(|(t, _)| !t.is_empty()).max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0))).map(|t| t.0).unwrap_or_default();
        let shown = countries::postcode_shown(&pc, &country);
        let key = countries::postcode_key(&pc, &country);
        let detail = [town, country_name(&country)].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ");
        out.push(Place { name: shown.clone(), kind: "Postcode".into(), detail, lat: lat / n as f64, lon: lon / n as f64, rank: 2, country,
                         other: [pc, key].into_iter().filter(|x| *x != shown).collect(), osm_id: None });
    }
    for r in gpkg::layer(g, "gis_osm_natural_free", &["fclass", "name"]) {
        let (Some("peak"), Some(name), Some(&(lon, lat))) = (r.col(0), r.col(1), r.parts.first().and_then(|p| p.first())) else { continue };
        if inside(lon, lat) {
            out.push(Place { name: name.into(), kind: "peak".into(), detail: detail(name, lon, lat), lat, lon, rank: 3, country: country_at(lon, lat), other: Vec::new(), osm_id: None });
        }
    }
    for table in ["gis_osm_pois_free", "gis_osm_pois_a_free", "gis_osm_traffic_free"] {
        for r in gpkg::layer(g, table, &["fclass", "name"]) {
            let (Some(cls), Some(name)) = (r.col(0).filter(|c| POI_KEEP.contains(c)), r.col(1)) else { continue };
            let Some((lon, lat)) = (if r.kind == Geom::Polygon { geo::centroid(&r.parts) } else { r.parts.first().and_then(|p| p.first()).copied() }) else { continue };
            if inside(lon, lat) {
                out.push(Place { name: name.into(), kind: cls.replace('_', " "), detail: detail(name, lon, lat), lat, lon, rank: 4, country: country_at(lon, lat), other: Vec::new(), osm_id: None });
            }
        }
    }
    out
}

pub fn write(path: &Path, places: &[Place], source: &str) -> rusqlite::Result<usize> {
    let _ = std::fs::remove_file(path);
    let c = Connection::open(path)?;
    c.execute_batch("PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF;
        CREATE TABLE place(id INTEGER PRIMARY KEY, name TEXT, kind TEXT, detail TEXT, lat REAL, lon REAL, rank INTEGER, source TEXT, country TEXT, key TEXT, area TEXT);
        CREATE VIRTUAL TABLE place_fts USING fts5(key, area, content='place', content_rowid='id', tokenize='unicode61 remove_diacritics 0');
        CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT);
        BEGIN;")?;
    for p in places {
        let names: Vec<&str> = std::iter::once(p.name.as_str()).chain(p.other.iter().map(String::as_str)).collect();
        let area: Vec<String> = area_words(&p.detail, &p.country);
        let area: Vec<&str> = area.iter().map(String::as_str).collect();
        c.execute("INSERT INTO place(name,kind,detail,lat,lon,rank,source,country,key,area) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                  params![p.name, p.kind, p.detail, p.lat, p.lon, p.rank, "OpenStreetMap", p.country, place_key(&names), place_key(&area)])?;
    }
    c.execute("INSERT INTO meta VALUES ('source', ?1)", [source])?;
    c.execute_batch("INSERT INTO place_fts(place_fts) VALUES ('rebuild'); COMMIT; VACUUM;")?;
    Ok(places.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::gpkg::write::{row, table};

    #[test]
    fn towns_municipalities_postcodes_and_points_found_by_the_engine() {
        let g = Connection::open_in_memory().unwrap();
        for t in ["gis_osm_places_free", "gis_osm_adminareas_a_free", "gis_osm_natural_free", "gis_osm_pois_a_free"] {
            table(&g, t, &["fclass", "name", "osm_id"]);
        }
        row(&g, "gis_osm_places_free", Geom::Point, &[vec![(9.5209, 47.1410)]], &[("fclass", "town"), ("name", "Vaduz"), ("osm_id", "100")]);
        row(&g, "gis_osm_places_free", Geom::Point, &[vec![(9.5100, 47.1650)]], &[("fclass", "town"), ("name", "Schaan"), ("osm_id", "101")]);
        row(&g, "gis_osm_places_free", Geom::Point, &[vec![(9.5600, 47.1400)]], &[("fclass", "hamlet"), ("name", "Mühleholz"), ("osm_id", "102")]);
        let li = vec![(9.47, 47.04), (9.64, 47.04), (9.64, 47.28), (9.47, 47.28), (9.47, 47.04)];
        let vaduz = vec![(9.50, 47.12), (9.58, 47.12), (9.58, 47.155), (9.50, 47.155), (9.50, 47.12)];
        row(&g, "gis_osm_adminareas_a_free", Geom::Polygon, &[li], &[("fclass", "national"), ("name", "Liechtenstein")]);
        row(&g, "gis_osm_adminareas_a_free", Geom::Polygon, &[vaduz], &[("fclass", "admin_level8"), ("name", "Vaduz")]);
        row(&g, "gis_osm_natural_free", Geom::Point, &[vec![(9.575, 47.13)]], &[("fclass", "peak"), ("name", "Kuegrat")]);
        row(&g, "gis_osm_pois_a_free", Geom::Polygon, &[vec![(9.517, 47.142), (9.519, 47.142), (9.519, 47.144), (9.517, 47.142)]], &[("fclass", "hospital"), ("name", "Landesspital")]);
        let admin = Admin::load(&g, "LI");
        let place_node = Thing { kind: Kind::Node, id: 100, tags: Tags(vec![("place".into(), "town".into()), ("name".into(), "Vaduz".into()), ("name:it".into(), "Vaduzzo".into())]),
                                 rings: Vec::new(), point: (9.5209, 47.1410) };
        let addr = |pc: &str, city: &str, lat: f64| Address { num: "1".into(), street: "S".into(), city: city.into(), postcode: pc.into(), country: "LI".into(), lat, lon: 9.52 };
        let places = collect(&g, &[place_node], &admin, &[addr("9490", "Vaduz", 47.14), addr("9490", "Vaduz", 47.15), addr("9494", "Schaan", 47.16)], "LI");
        let kinds: Vec<(String, String, String)> = places.iter().map(|p| (p.name.clone(), p.kind.clone(), p.detail.clone())).collect();
        for want in [("Vaduz", "town", "Liechtenstein"), ("Mühleholz", "hamlet", "Vaduz, Liechtenstein"), ("Vaduz", "municipality", "Liechtenstein"),
                     ("9490", "Postcode", "Vaduz, Liechtenstein"), ("Kuegrat", "peak", "Vaduz, Liechtenstein"), ("Landesspital", "hospital", "Vaduz, Liechtenstein")] {
            assert!(kinds.iter().any(|k| (k.0.as_str(), k.1.as_str(), k.2.as_str()) == want), "{want:?} in {kinds:?}");
        }
        let pc = places.iter().find(|p| p.name == "9490").unwrap();
        assert!((pc.lat - 47.145).abs() < 1e-9, "the middle of its addresses");
        let dir = std::env::temp_dir().join(format!("lb-tools-places-{}", std::process::id()));
        let data = dir.join("data");
        std::fs::create_dir_all(data.join("derived/geo")).unwrap();
        write(&data.join("derived/geo/places.sqlite"), &places, "OpenStreetMap").unwrap();
        let map = lastbook_engine::map::Map::new(lastbook_engine::paths::Paths::new(dir.clone(), Some(data)));
        let first = |q: &str| map.places(q, 20, None).first().map(|p| (p["name"].as_str().unwrap_or("").to_string(), p["kind"].as_str().unwrap_or("").to_string()));
        assert_eq!(first("Vaduz"), Some(("Vaduz".into(), "town".into())));
        assert_eq!(first("Vaduzzo"), Some(("Vaduz".into(), "town".into())), "a name in another language");
        assert_eq!(first("Muehleholz"), Some(("Mühleholz".into(), "hamlet".into())));
        assert_eq!(first("9494"), Some(("9494".into(), "Postcode".into())));
        assert_eq!(first("Landesspital"), Some(("Landesspital".into(), "hospital".into())));
        assert_eq!(first("Schaan Liechtenstein"), Some(("Schaan".into(), "town".into())), "the country's name finds its places");
        lastbook_engine::db::close_all();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
