//! The address index (`derived/geo/addresses/<REGION>.sqlite`, docs/ADDRESSES.md) from the PBF's `addr:*` tags, written
//! through the engine's own schema and keys (`map::address::SCHEMA`, `Address::insert`, `FINISH`), so the builder and the
//! search can never disagree on a key.
//!
//! An address is a node, or a building (the middle of its outline). Its country is `addr:country`, else the region's
//! country when the point lies inside it; its town is `addr:city`, else the municipality it lies in.

use crate::map::admin::Admin;
use crate::map::geo;
use crate::map::osm::{Kind, Thing};
use lastbook_engine::map::address::{self, countries, Address};
use lastbook_engine::map::text::name_key;
use lastbook_engine::regions;
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::Path;

/// The elements an address comes from.
pub fn wanted(_: Kind, t: &[(&str, &str)]) -> bool {
    t.iter().any(|(k, v)| !v.is_empty() && matches!(*k, "addr:housenumber" | "addr:conscriptionnumber" | "addr:streetnumber"))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub num: String,
    pub street: String,
    pub city: String,
    pub postcode: String,
    pub country: String,
    pub lat: f64,
    pub lon: f64,
}

/// The numbers an address is filed under: each of "12;14", and a Czech or Slovak "846/1" also as 846 and 1.
fn numbers(t: &crate::map::osm::Tags, country: &str) -> Vec<String> {
    fn add(out: &mut Vec<String>, n: &str) {
        let n = n.trim();
        if !n.is_empty() && !out.iter().any(|o| o == n) {
            out.push(n.to_string());
        }
    }
    let mut out: Vec<String> = Vec::new();
    if let Some(h) = t.get("addr:housenumber") {
        for n in h.split(';') {
            add(&mut out, n);
        }
    }
    if let (Some(c), Some(s)) = (t.get("addr:conscriptionnumber"), t.get("addr:streetnumber")) {
        add(&mut out, &format!("{c}/{s}"));
    }
    if matches!(country, "CZ" | "SK") {
        for n in out.clone() {
            if let Some((c, s)) = n.split_once('/') {
                add(&mut out, c);
                add(&mut out, s);
            }
        }
        for k in ["addr:conscriptionnumber", "addr:streetnumber"] {
            if let Some(n) = t.get(k) {
                add(&mut out, n);
            }
        }
    }
    out
}

/// The town an address is in: `addr:city` written as the region's own name for it ("MAuren" is Mauren), unless it is a
/// rare name the region does not know ("Balzers (FL)", a street typed in the wrong field), which gives way to the
/// municipality the address lies in.
fn town(tagged: Option<&str>, uses: &HashMap<String, usize>, known: &HashMap<String, String>, areas: &Admin, lon: f64, lat: f64) -> String {
    let municipality = || areas.town_at(lon, lat).map(str::to_string);
    let Some(city) = tagged else { return municipality().unwrap_or_default() };
    if let Some(name) = known.get(&name_key(city)) {
        return name.clone();
    }
    if uses.get(city).copied().unwrap_or(0) < 3 {
        if let Some(m) = municipality() {
            return m;
        }
    }
    city.to_string()
}

/// The addresses of the region, one row per number, the same address mapped twice (on a node and on its building) once.
/// `places` are the names the region knows for its towns and villages.
pub fn rows(things: &[Thing], region: &str, areas: &Admin, places: &[String]) -> Vec<Row> {
    let mut known: HashMap<String, String> = areas.towns.iter().chain(&areas.cities).map(|(n, _)| (name_key(n), n.clone())).collect();
    for p in places {
        known.entry(name_key(p)).or_insert_with(|| p.clone());
    }
    let mut uses: HashMap<String, usize> = HashMap::new();
    for t in things {
        if let Some(c) = t.tags.get("addr:city") {
            *uses.entry(c.to_string()).or_default() += 1;
        }
    }
    let mut out: Vec<Row> = Vec::new();
    let mut seen: HashMap<(String, String, String, String), Vec<(f64, f64)>> = HashMap::new();
    let mut sorted: Vec<&Thing> = things.iter().collect();
    sorted.sort_by_key(|t| match t.kind {
        Kind::Way => 0, // a building's middle first, then entrances
        Kind::Relation => 1,
        Kind::Node => 2,
    });
    for t in sorted {
        let (lon, lat) = t.point;
        if !regions::contains(region, lat, lon) {
            continue;
        }
        let tagged = t.tags.get("addr:country").map(str::to_uppercase).filter(|c| c.len() == 2 && countries::get(c).is_some());
        let country = match (tagged, &areas.country) {
            (Some(c), _) => c,
            (None, Some((c, rings))) if geo::contains(rings, lon, lat) => c.clone(),
            (None, Some(_)) => continue, // outside the region's country and not said: another country's index has it
            (None, None) => regions::country(region).to_string(),
        };
        let Some(street) = t.tags.get("addr:street").or(t.tags.get("addr:place")) else { continue };
        let city = town(t.tags.get("addr:city"), &uses, &known, areas, lon, lat);
        let postcode = t.tags.get("addr:postcode").unwrap_or("").to_string();
        for num in numbers(&t.tags, &country) {
            let key = (country.clone(), street.to_string(), num.clone(), city.clone());
            let near = seen.entry(key).or_default();
            if near.iter().any(|&(x, y)| (x - lon).abs() < 0.002 && (y - lat).abs() < 0.0015) {
                continue;
            }
            near.push((lon, lat));
            out.push(Row { num, street: street.to_string(), city: city.clone(), postcode: postcode.clone(), country: country.clone(), lat, lon });
        }
    }
    out
}

/// Writes the index: the engine's tables, every row with its keys, the indexes and `area`, and `meta`.
pub fn write(path: &Path, region: &str, rows: &[Row], source: &str) -> rusqlite::Result<usize> {
    let _ = std::fs::remove_file(path);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let c = Connection::open(path)?;
    c.execute_batch("PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF;")?;
    c.execute_batch(address::SCHEMA)?;
    c.execute_batch("BEGIN")?;
    for r in rows {
        Address { num: &r.num, street: &r.street, city: &r.city, state: "", postcode: &r.postcode, country: &r.country, lat: r.lat, lon: r.lon }.insert(&c)?;
    }
    c.execute_batch("COMMIT")?;
    c.execute_batch(address::FINISH)?;
    // a postcode offered with a town only a stray address or two gives it (a postcode typed for the wrong town) is left out
    // of the postcode list; those addresses are still found
    c.execute_batch("DELETE FROM area WHERE zip != '' AND n < 3
        AND n * 50 < (SELECT MAX(b.n) FROM area b WHERE b.country = area.country AND b.zip = area.zip);")?;
    let mut countries: Vec<&str> = rows.iter().map(|r| r.country.as_str()).collect();
    countries.sort_unstable();
    countries.dedup();
    let own = regions::country(region);
    if let Some(i) = countries.iter().position(|c| *c == own) {
        countries.remove(i);
        countries.insert(0, own); // the region's own country first
    }
    for (k, v) in [("schema", "2".to_string()), ("region", region.to_string()), ("countries", countries.join(",")), ("publisher", "OpenStreetMap".into()),
                   ("source", source.to_string()), ("count", rows.len().to_string())] {
        c.execute("INSERT INTO meta VALUES (?1,?2)", [k, &v])?;
    }
    c.execute_batch("VACUUM")?;
    Ok(rows.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::osm::write::{pbf, El};

    #[test]
    fn addresses_from_the_pbf_found_by_the_engine() {
        let dir = std::env::temp_dir().join(format!("lb-tools-addr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("li.osm.pbf");
        let b = |id: i64, x: f64, y: f64| vec![El::Node(id, y, x, &[]), El::Node(id + 1, y, x + 0.0002, &[]), El::Node(id + 2, y + 0.0002, x + 0.0002, &[]), El::Node(id + 3, y + 0.0002, x, &[])];
        let mut els = vec![
            El::Node(1, 47.1395, 9.5215, &[("addr:housenumber", "12"), ("addr:street", "Städtle"), ("addr:postcode", "9490"), ("addr:city", "Vaduz"), ("addr:country", "LI")]),
            El::Node(2, 47.1396, 9.5216, &[("addr:housenumber", "12"), ("addr:street", "Städtle"), ("addr:postcode", "9490"), ("addr:city", "Vaduz"), ("addr:country", "LI")]),
            El::Node(3, 47.1372, 9.5190, &[("addr:housenumber", "5;7"), ("addr:street", "Gerberweg")]),
            El::Node(4, 47.2380, 9.5980, &[("addr:housenumber", "1"), ("addr:street", "Marktplatz"), ("addr:city", "Feldkirch"), ("addr:country", "AT")]),
            El::Node(5, 47.3000, 9.7000, &[("addr:housenumber", "2"), ("addr:street", "Marktplatz")]),
            El::Node(6, 40.0, 3.0, &[("addr:housenumber", "9"), ("addr:street", "Far")]),
            El::Node(7, 47.1380, 9.5200, &[("addr:housenumber", "6"), ("addr:street", "Josef Rheinberger Strasse"), ("addr:city", "Josef Rheinberger Strasse"), ("addr:postcode", "9490")]),
            El::Node(8, 47.1385, 9.5205, &[("addr:housenumber", "1"), ("addr:street", "Herrengasse"), ("addr:city", "VADUZ")]),
        ];
        els.extend(b(10, 9.5100, 47.1650));
        els.push(El::Way(100, &[10, 11, 12, 13, 10], &[("building", "yes"), ("addr:housenumber", "7"), ("addr:street", "Landstrasse"), ("addr:postcode", "9494"), ("addr:city", "Schaan")]));
        std::fs::write(&src, pbf(&els)).unwrap();
        let things = crate::map::osm::read(&src, &wanted).unwrap();
        let li = vec![vec![(9.47, 47.04), (9.64, 47.04), (9.64, 47.28), (9.47, 47.28), (9.47, 47.04)]];
        let vaduz = vec![vec![(9.50, 47.12), (9.54, 47.12), (9.54, 47.15), (9.50, 47.15), (9.50, 47.12)]];
        let areas = Admin { country: Some(("LI".into(), li)), towns: vec![("Vaduz".into(), vaduz)], districts: Vec::new(), cities: Vec::new() };
        let rows = rows(&things, "LI", &areas, &[]);
        let mut shown: Vec<String> = rows.iter().map(|r| format!("{} {} {} {} {}", r.street, r.num, r.postcode, r.city, r.country)).collect();
        shown.sort();
        assert_eq!(shown, vec!["Gerberweg 5  Vaduz LI", "Gerberweg 7  Vaduz LI", "Herrengasse 1  Vaduz LI", "Josef Rheinberger Strasse 6 9490 Vaduz LI",
                               "Landstrasse 7 9494 Schaan LI", "Marktplatz 1  Feldkirch AT", "Städtle 12 9490 Vaduz LI"],
                   "one row per number; Städtle 12 mapped twice once; an address abroad without a country left to its own country's index; \
                    the town from the municipality when none is given, or when the one given is a rare unknown name; a known name in its own spelling");
        let index = dir.join("LI.sqlite");
        assert_eq!(write(&index, "LI", &rows, "OpenStreetMap contributors (ODbL 1.0)").unwrap(), 7);
        let c = Connection::open(&index).unwrap();
        let meta: String = c.query_row("SELECT value FROM meta WHERE key='countries'", [], |r| r.get(0)).unwrap();
        assert_eq!(meta, "LI,AT");
        drop(c);
        // the engine finds them in an installed library
        let data = dir.join("data");
        std::fs::create_dir_all(data.join("derived/geo/addresses")).unwrap();
        std::fs::copy(&index, data.join("derived/geo/addresses/LI.sqlite")).unwrap();
        let map = lastbook_engine::map::Map::new(lastbook_engine::paths::Paths::new(dir.clone(), Some(data)));
        let r = map.find_addresses("Städtle 12, Vaduz", 8, None);
        assert_eq!(r.len(), 1);
        assert_eq!((r[0]["name"].as_str(), r[0]["detail"].as_str()), (Some("Städtle 12"), Some("9490 Vaduz, Liechtenstein")));
        assert_eq!(map.find_addresses("Landstrasse 7, 9494 Schaan", 8, None)[0]["detail"], "9494 Schaan, Liechtenstein");
        let pc = map.places("9490", 20, None);
        assert_eq!((pc[0]["kind"].as_str(), pc[0]["name"].as_str()), (Some("Postcode"), Some("9490")));
        lastbook_engine::db::close_all();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_stray_postcode_town_is_left_out_of_the_postcode_list() {
        let dir = std::env::temp_dir().join(format!("lb-tools-addr-stray-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let row = |n: usize, city: &str| Row { num: n.to_string(), street: "Feldstrasse".into(), city: city.into(), postcode: "9490".into(), country: "LI".into(),
                                              lat: 47.14 + n as f64 * 1e-5, lon: 9.52 };
        let mut rows: Vec<Row> = (1..=60).map(|n| row(n, "Vaduz")).collect();
        rows.push(row(61, "Schaan"));
        let index = dir.join("LI.sqlite");
        write(&index, "LI", &rows, "test").unwrap();
        let c = Connection::open(&index).unwrap();
        let towns: Vec<String> = c.prepare("SELECT city FROM area WHERE zip='9490'").unwrap().query_map([], |r| r.get(0)).unwrap().flatten().collect();
        assert_eq!(towns, vec!["Vaduz"]);
        assert_eq!(c.query_row("SELECT count(*) FROM addr WHERE city='Schaan'", [], |r| r.get::<_, i64>(0)).unwrap(), 1, "the address itself stays");
        drop(c);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn numbers_of_one_address() {
        let t = |v: &[(&str, &str)]| crate::map::osm::Tags(v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect());
        assert_eq!(numbers(&t(&[("addr:housenumber", "12;14")]), "LI"), vec!["12", "14"]);
        assert_eq!(numbers(&t(&[("addr:housenumber", "846/1")]), "CZ"), vec!["846/1", "846", "1"]);
        assert_eq!(numbers(&t(&[("addr:conscriptionnumber", "846"), ("addr:streetnumber", "1")]), "CZ"), vec!["846/1", "846", "1"]);
        assert_eq!(numbers(&t(&[("addr:housenumber", "12/3")]), "AT"), vec!["12/3"], "an Austrian door number stays one");
    }
}
