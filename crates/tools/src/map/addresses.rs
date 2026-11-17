//! The address index (`derived/geo/addresses/<REGION>.sqlite`, docs/ADDRESSES.md) from the PBF's `addr:*` tags, written
//! through the engine's own schema and keys (`map::address::SCHEMA`, `Address::insert`, `FINISH`), so the builder and the
//! search can never disagree on a key.
//!
//! An address is a node, or a building (the middle of its outline). Its country is `addr:country`, else the region's
//! country when the point lies inside it; its town is `addr:city`, else the municipality it lies in. A street named in
//! two languages (Triq ir-Repubblika, Republic Street) files it under both, and a town's other names (Floriana for
//! Il-Furjana, Luxemburg for Luxembourg) join its key, so the search finds it as people there write it.

use crate::map::admin::Admin;
use crate::map::geo;
use crate::map::osm::{Kind, Tags, Thing};
use crate::map::places;
use lastbook_engine::map::address::{self, countries, Address};
use lastbook_engine::map::text::{fold, name_key};
use lastbook_engine::regions;
use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// The elements an address comes from.
pub fn wanted(_: Kind, t: &[(&str, &str)]) -> bool {
    t.iter().any(|(k, v)| !v.is_empty() && matches!(*k, "addr:housenumber" | "addr:conscriptionnumber" | "addr:streetnumber"))
}

/// Streets named in more than one language, for the other names of the addresses on them.
pub fn street_wanted(kind: Kind, t: &[(&str, &str)]) -> bool {
    kind == Kind::Way && t.iter().any(|(k, _)| *k == "highway") && t.iter().any(|(k, _)| *k == "name") && t.iter().any(|(k, _)| k.starts_with("name:"))
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
    /// The same address again, under another name of its street.
    pub alias: bool,
}

/// A postcode in its country's own form ("VLT 1117", "1012 AB", "1611" shown L-1611), letters folded as the official
/// codes are written ("ŻTN 3000" is ZTN 3000). None when it has no shape of the country's postcodes ("1210" in Malta):
/// the address is kept without it.
pub fn postcode(written: &str, country: &str) -> Option<String> {
    let up: String = written.trim().chars().map(|c| if c.is_ascii() { c.to_ascii_uppercase().to_string() } else { fold(&c.to_string()).to_uppercase() }).collect();
    let (_, key, _) = countries::postcode_candidates(&up).into_iter().find(|(c, _, partial)| *c == country && !partial)?;
    for s in countries::get(country)?.postcode {
        let compact: String = s.chars().filter(|c| *c != ' ').collect();
        if compact.chars().count() == key.chars().count() && countries::shape(&key, &compact) {
            let mut k = key.chars();
            return Some(s.chars().map(|p| if p == ' ' { ' ' } else { k.next().unwrap_or(' ') }).collect());
        }
    }
    Some(key)
}

/// A street's names: `name`, then those in the languages of the country's street names.
fn street_names(t: &Tags, langs: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = t.get("name").into_iter().map(str::to_string).collect();
    for l in langs {
        if let Some(n) = t.get(&format!("name:{l}")) {
            if !out.iter().any(|o| o == n) {
                out.push(n.to_string());
            }
        }
    }
    out
}

/// Every name of each place, by the key of each: what a town in an address may also be called.
pub fn town_names(things: &[Thing], country: &str) -> HashMap<String, Vec<String>> {
    let langs = places::langs(country);
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for t in things.iter().filter(|t| t.tags.has("place")) {
        let Some(name) = t.tags.get("name") else { continue };
        let names: Vec<String> = std::iter::once(name.to_string()).chain(places::other_names(&t.tags, &langs)).collect();
        for n in &names {
            let e = out.entry(name_key(n)).or_default();
            for o in &names {
                if !e.contains(o) {
                    e.push(o.clone());
                }
            }
        }
    }
    out
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
    // a street's other names, in the languages the region's addresses are seen written in: some of Malta's in English,
    // Luxembourg's in each street's own name only (its Luxembourgish and German renderings would more than double the
    // index for nothing). A language counts when at least 2% of the streets named in it have an address written so.
    let used: HashSet<&str> = things.iter().filter_map(|t| t.tags.get("addr:street")).collect();
    let streets: Vec<&Thing> = things.iter().filter(|t| t.kind == Kind::Way && t.tags.has("highway")).collect();
    let langs: Vec<&str> = countries::get(regions::country(region)).map(|c| c.langs).unwrap_or(&[]).iter().copied().filter(|l| {
        let key = format!("name:{l}");
        let named: Vec<&str> = streets.iter().filter_map(|t| t.tags.get(&key).filter(|n| Some(*n) != t.tags.get("name"))).collect();
        let seen = named.iter().filter(|n| used.contains(*n)).count();
        if !named.is_empty() {
            eprintln!("{region}: {} streets named in {l} besides their name, {seen} of those names on addresses", named.len());
        }
        seen > 0 && seen * 50 >= named.len()
    }).collect();
    // by the street's town and name, and by its name alone for an address across a town's edge
    let mut by_town: HashMap<(String, String), Vec<String>> = HashMap::new();
    let mut by_name: HashMap<String, Vec<String>> = HashMap::new();
    for t in streets {
        let names = street_names(&t.tags, &langs);
        if names.len() < 2 {
            continue;
        }
        let town = areas.town_at(t.point.0, t.point.1).unwrap_or("").to_string();
        for n in &names {
            for e in [by_town.entry((town.clone(), n.clone())).or_default(), by_name.entry(n.clone()).or_default()] {
                for o in names.iter().filter(|o| *o != n) {
                    if !e.contains(o) {
                        e.push(o.clone());
                    }
                }
            }
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
        let code = t.tags.get("addr:postcode").and_then(|p| postcode(p, &country)).unwrap_or_default();
        let here = areas.town_at(lon, lat).unwrap_or("").to_string();
        let others = by_town.get(&(here, street.to_string())).or_else(|| by_name.get(street)).cloned().unwrap_or_default();
        for num in numbers(&t.tags, &country) {
            for (i, name) in std::iter::once(street.to_string()).chain(others.iter().cloned()).enumerate() {
                let near = seen.entry((country.clone(), name.clone(), num.clone(), city.clone())).or_default();
                if near.iter().any(|&(x, y)| (x - lon).abs() < 0.002 && (y - lat).abs() < 0.0015) {
                    continue;
                }
                near.push((lon, lat));
                out.push(Row { num: num.clone(), street: name, city: city.clone(), postcode: code.clone(), country: country.clone(), lat, lon, alias: i > 0 });
            }
        }
    }
    out
}

/// Writes the index: the engine's tables, every row with its keys, the indexes and `area`, and `meta`. `towns` gives a
/// town's other names (`town_names`): their keys join its `citykey`, which the search reads word by word. A town with a
/// name the search reads as a street (St Julian's: the street "St" in a town "Julian's…") keeps its own name only, or
/// that name typed alone would find streets of San Ġiljan instead of the town the place finder knows it as. Returns the
/// number of addresses, the rows under a street's other names not counted.
pub fn write(path: &Path, region: &str, rows: &[Row], source: &str, towns: &HashMap<String, Vec<String>>) -> rusqlite::Result<usize> {
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
    let mut cities: Vec<&str> = rows.iter().map(|r| r.city.as_str()).collect();
    cities.sort_unstable();
    cities.dedup();
    for city in cities {
        let own = name_key(city);
        let mut key = own.clone();
        let names: Vec<&String> = towns.get(&own).into_iter().flatten().collect();
        let street = |n: &str| address::parse(n).kind == address::Kind::Address || address::parse(&name_key(n)).kind == address::Kind::Address;
        if names.iter().any(|n| street(n)) {
            continue;
        }
        for k in names.into_iter().map(|n| name_key(n)) {
            if !k.is_empty() && !format!(" {key} ").contains(&format!(" {k} ")) {
                key = format!("{key} {k}");
            }
        }
        if key != own {
            c.execute("UPDATE addr SET citykey = ?1 WHERE city = ?2", params![key, city])?;
        }
    }
    c.execute_batch("COMMIT")?;
    c.execute_batch(address::FINISH)?;
    // a postcode offered with a town only a stray address or few give it (a postcode typed for the wrong town, the state's
    // name for Bremerhaven's) is left out of the postcode list; those addresses are still found
    c.execute_batch("DELETE FROM area WHERE zip != '' AND n <= 5
        AND n * 100 < (SELECT MAX(b.n) FROM area b WHERE b.country = area.country AND b.zip = area.zip);")?;
    let count = rows.iter().filter(|r| !r.alias).count();
    let mut countries: Vec<&str> = rows.iter().map(|r| r.country.as_str()).collect();
    countries.sort_unstable();
    countries.dedup();
    let own = regions::country(region);
    if let Some(i) = countries.iter().position(|c| *c == own) {
        countries.remove(i);
        countries.insert(0, own); // the region's own country first
    }
    for (k, v) in [("schema", "2".to_string()), ("region", region.to_string()), ("countries", countries.join(",")), ("publisher", "OpenStreetMap".into()),
                   ("source", source.to_string()), ("count", count.to_string())] {
        c.execute("INSERT INTO meta VALUES (?1,?2)", [k, &v])?;
    }
    c.execute_batch("VACUUM")?;
    Ok(count)
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
        assert_eq!(write(&index, "LI", &rows, "OpenStreetMap contributors (ODbL 1.0)", &HashMap::new()).unwrap(), 7);
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
        let _ = std::fs::remove_dir_all(&dir); // a temporary folder left behind (Windows, a file still held) fails no test
    }

    #[test]
    fn a_stray_postcode_town_is_left_out_of_the_postcode_list() {
        let dir = std::env::temp_dir().join(format!("lb-tools-addr-stray-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let row = |n: usize, pc: &str, city: &str| Row { num: n.to_string(), street: "Feldstrasse".into(), city: city.into(), postcode: pc.into(),
                                                        country: "DE".into(), lat: 53.54 + n as f64 * 1e-5, lon: 8.58, alias: false };
        // Bremerhaven's 27568, five of them written with the state's name; a village's own six beside a town's 600
        let mut rows: Vec<Row> = (1..=600).map(|n| row(n, "27568", "Bremerhaven")).collect();
        rows.extend((601..=605).map(|n| row(n, "27568", "Bremen")));
        rows.extend((1..=600).map(|n| row(n, "27619", "Schiffdorf")));
        rows.extend((601..=606).map(|n| row(n, "27619", "Spaden")));
        let index = dir.join("DE-HB.sqlite");
        write(&index, "DE-HB", &rows, "test", &HashMap::new()).unwrap();
        let c = Connection::open(&index).unwrap();
        let towns = |zip: &str| -> Vec<String> {
            c.prepare("SELECT city FROM area WHERE zip=?1 ORDER BY n DESC").unwrap().query_map([zip], |r| r.get(0)).unwrap().flatten().collect()
        };
        assert_eq!(towns("27568"), vec!["Bremerhaven"]);
        assert_eq!(towns("27619"), vec!["Schiffdorf", "Spaden"], "six addresses are a place of the postcode's own");
        assert_eq!(c.query_row("SELECT count(*) FROM addr WHERE city='Bremen'", [], |r| r.get::<_, i64>(0)).unwrap(), 5, "the addresses themselves stay");
        drop(c);
        let _ = std::fs::remove_dir_all(&dir); // a temporary folder left behind (Windows, a file still held) fails no test
    }

    #[test]
    fn postcodes_in_their_countrys_form() {
        for (written, country, want) in [("VLT 1117", "MT", Some("VLT 1117")), ("VLT1117", "MT", Some("VLT 1117")), ("vlt 1117", "MT", Some("VLT 1117")),
                                         ("ŻTN 3000", "MT", Some("ZTN 3000")), ("MLĦ 2777", "MT", Some("MLH 2777")), ("1210", "MT", None),
                                         ("L-1648", "LU", Some("1648")), ("1648", "LU", Some("1648")), ("28195", "DE", Some("28195")), ("2819", "DE", None),
                                         ("FL-9490", "LI", Some("9490")), ("1012ab", "NL", Some("1012 AB")), ("D02X285", "IE", Some("D02 X285"))] {
            assert_eq!(postcode(written, country).as_deref(), want, "{written} in {country}");
        }
    }

    #[test]
    fn two_languages_in_malta() {
        let dir = std::env::temp_dir().join(format!("lb-tools-addr-mt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("mt.osm.pbf");
        std::fs::write(&src, pbf(&[
            El::Node(1, 35.8975, 14.5125, &[]), El::Node(2, 35.8995, 14.5145, &[]),
            El::Way(10, &[1, 2], &[("highway", "pedestrian"), ("name", "Triq ir-Repubblika"), ("name:en", "Republic Street"), ("name:mt", "Triq ir-Repubblika")]),
            El::Node(3, 35.8985, 14.5135, &[("addr:housenumber", "74"), ("addr:street", "Triq ir-Repubblika"), ("addr:city", "Il-Belt Valletta"), ("addr:postcode", "VLT1117")]),
            El::Node(4, 35.8900, 14.5000, &[("addr:housenumber", "2"), ("addr:street", "Triq San Anton"), ("addr:postcode", "1210")]),
            El::Node(5, 35.8905, 14.5005, &[("place", "town"), ("name", "Il-Furjana"), ("name:en", "Floriana"), ("name:ja", "フロリアーナ")]),
            El::Node(6, 35.8960, 14.5100, &[]), El::Node(7, 35.8970, 14.5110, &[]),
            El::Way(11, &[6, 7], &[("highway", "residential"), ("name", "Triq il-Merkanti"), ("name:en", "Merchants Street")]),
            // written in English, as some of Malta's addresses are: the reason English names are used at all
            El::Node(8, 35.8965, 14.5105, &[("addr:housenumber", "5"), ("addr:street", "Merchants Street"), ("addr:city", "Valletta")]),
            El::Node(9, 35.9185, 14.4895, &[("place", "town"), ("name", "San Ġiljan"), ("name:en", "St. Julian's"), ("name:it", "San Giuliano")]),
            El::Node(12, 35.9180, 14.4890, &[("addr:housenumber", "1"), ("addr:street", "Triq San Ġorġ"), ("addr:city", "San Ġiljan")]),
        ])).unwrap();
        let things = crate::map::osm::read(&src, &|k, t| wanted(k, t) || street_wanted(k, t) || places::wanted(k, t)).unwrap();
        let sq = |x: f64, y: f64| vec![vec![(x, y), (x + 0.01, y), (x + 0.01, y + 0.01), (x, y + 0.01), (x, y)]];
        let malta = vec![vec![(14.1, 35.7), (14.7, 35.7), (14.7, 36.1), (14.1, 36.1), (14.1, 35.7)]];
        let areas = Admin { country: Some(("MT".into(), malta)), towns: vec![("Il-Belt Valletta".into(), sq(14.505, 35.894)), ("Il-Furjana".into(), sq(14.495, 35.885))],
                            districts: Vec::new(), cities: Vec::new() };
        let rows = rows(&things, "MT", &areas, &[]);
        let shown: Vec<String> = rows.iter().map(|r| format!("{} {} | {} | {} | {}", r.num, r.street, r.city, r.postcode, r.alias)).collect();
        assert_eq!(shown, vec!["74 Triq ir-Repubblika | Il-Belt Valletta | VLT 1117 | false", "74 Republic Street | Il-Belt Valletta | VLT 1117 | true",
                               "2 Triq San Anton | Il-Furjana |  | false", "5 Merchants Street | Il-Belt Valletta |  | false",
                               "5 Triq il-Merkanti | Il-Belt Valletta |  | true", "1 Triq San Ġorġ | San Ġiljan |  | false"],
                   "a street under both its names; a postcode of no Maltese shape left out; a rare unknown town name (Valletta) the municipality's");
        let towns = town_names(&things, "MT");
        assert_eq!(towns["floriana"], vec!["Il-Furjana", "Floriana"], "names in the country's and the app's languages, not every language");
        let data = dir.join("data");
        std::fs::create_dir_all(data.join("derived/geo/addresses")).unwrap();
        let index = data.join("derived/geo/addresses/MT.sqlite");
        assert_eq!(write(&index, "MT", &rows, "test", &towns).unwrap(), 4, "four addresses, six rows");
        let c = Connection::open(&index).unwrap();
        let key = |city: &str| -> String { c.query_row("SELECT DISTINCT citykey FROM addr WHERE city=?1", [city], |r| r.get(0)).unwrap() };
        assert_eq!(key("Il-Furjana"), "il furjana floriana");
        assert_eq!(key("San Ġiljan"), "san giljan", "St Julian's alone reads as the street St in a town Julian's: left to the place finder");
        drop(c);
        let map = lastbook_engine::map::Map::new(lastbook_engine::paths::Paths::new(dir.clone(), Some(data)));
        let found = |q: &str| map.find_addresses(q, 8, None).first().map(|a| format!("{} | {}", a["name"].as_str().unwrap_or(""), a["detail"].as_str().unwrap_or("")));
        assert_eq!(found("74 Republic Street, Valletta VLT 1117").as_deref(), Some("74 Republic Street | Il-Belt Valletta VLT 1117, Malta"));
        assert_eq!(found("Triq ir-Repubblika 74, Il-Belt Valletta").as_deref(), Some("74 Triq ir-Repubblika | Il-Belt Valletta VLT 1117, Malta"));
        assert_eq!(found("5 Triq il-Merkanti, Valletta").as_deref(), Some("5 Triq il-Merkanti | Il-Belt Valletta, Malta"));
        assert_eq!(found("2 Triq San Anton, Floriana").as_deref(), Some("2 Triq San Anton | Il-Furjana, Malta"), "the town by its English name");
        assert_eq!(map.places("VLT1117", 5, None)[0]["name"], "VLT 1117");
        lastbook_engine::db::close_all();
        let _ = std::fs::remove_dir_all(&dir); // a temporary folder left behind (Windows, a file still held) fails no test
    }

    #[test]
    fn a_streets_renderings_no_address_uses_are_left_out() {
        // Luxembourg's streets carry Luxembourgish and German renderings of their names, its addresses only the name itself
        let dir = std::env::temp_dir().join(format!("lb-tools-addr-lu-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("lu.osm.pbf");
        std::fs::write(&src, pbf(&[
            El::Node(1, 49.6000, 6.1330, &[]), El::Node(2, 49.6010, 6.1340, &[]),
            El::Way(10, &[1, 2], &[("highway", "primary"), ("name", "Avenue de la Gare"), ("name:lb", "Garewee"), ("name:de", "Bahnhofsallee")]),
            El::Node(3, 49.6005, 6.1335, &[("addr:housenumber", "12"), ("addr:street", "Avenue de la Gare"), ("addr:city", "Luxembourg"), ("addr:postcode", "1611")]),
        ])).unwrap();
        let things = crate::map::osm::read(&src, &|k, t| wanted(k, t) || street_wanted(k, t)).unwrap();
        let rows = rows(&things, "LU", &Admin::default(), &[]);
        assert_eq!(rows.iter().map(|r| r.street.as_str()).collect::<Vec<_>>(), vec!["Avenue de la Gare"]);
        let _ = std::fs::remove_dir_all(&dir); // a temporary folder left behind (Windows, a file still held) fails no test
    }

    #[test]
    fn bremen_and_its_exclave() {
        let dir = std::env::temp_dir().join(format!("lb-tools-addr-hb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("hb.osm.pbf");
        std::fs::write(&src, pbf(&[
            El::Node(1, 53.0760, 8.8070, &[("addr:housenumber", "21"), ("addr:street", "Am Markt"), ("addr:postcode", "28195"), ("addr:city", "Bremen")]),
            El::Node(2, 53.5460, 8.5770, &[("addr:housenumber", "10"), ("addr:street", "Bürgermeister-Smidt-Straße"), ("addr:postcode", "27568")]),
            El::Node(3, 53.3000, 8.7000, &[("addr:housenumber", "1"), ("addr:street", "Dorfstraße")]),
            El::Node(4, 53.2500, 8.7500, &[("addr:housenumber", "5"), ("addr:street", "Hauptstraße"), ("addr:city", "Ritterhude"), ("addr:country", "DE")]),
        ])).unwrap();
        let things = crate::map::osm::read(&src, &wanted).unwrap();
        // the state is two pieces, each a city that is its own district, with Lower Saxony between them
        let bremen = vec![(8.48, 53.01), (8.99, 53.01), (8.99, 53.23), (8.48, 53.23), (8.48, 53.01)];
        let bremerhaven = vec![(8.48, 53.46), (8.65, 53.46), (8.65, 53.60), (8.48, 53.60), (8.48, 53.46)];
        let g = Connection::open_in_memory().unwrap();
        crate::map::gpkg::write::table(&g, "gis_osm_adminareas_a_free", &["fclass", "name"]);
        let poly = crate::map::gpkg::Kind::Polygon;
        crate::map::gpkg::write::row(&g, "gis_osm_adminareas_a_free", poly, &[bremerhaven.clone(), bremen.clone()], &[("fclass", "admin_level4"), ("name", "Bremen")]);
        crate::map::gpkg::write::row(&g, "gis_osm_adminareas_a_free", poly, &[bremen], &[("fclass", "admin_level6"), ("name", "Bremen")]);
        crate::map::gpkg::write::row(&g, "gis_osm_adminareas_a_free", poly, &[bremerhaven], &[("fclass", "admin_level6"), ("name", "Bremerhaven")]);
        let areas = Admin::load(&g, "DE");
        assert_eq!((areas.towns.len(), areas.cities.len()), (0, 2), "no level-8 municipality: the two districts are the towns");
        let rows = rows(&things, "DE-HB", &areas, &[]);
        let shown: Vec<String> = rows.iter().map(|r| format!("{} {}, {} {}", r.street, r.num, r.postcode, r.city)).collect();
        assert_eq!(shown, vec!["Am Markt 21, 28195 Bremen", "Bürgermeister-Smidt-Straße 10, 27568 Bremerhaven", "Hauptstraße 5,  Ritterhude"],
                   "the exclave's address with its own town; Lower Saxony's between them only when it says its country");
        let _ = std::fs::remove_dir_all(&dir); // a temporary folder left behind (Windows, a file still held) fails no test
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
