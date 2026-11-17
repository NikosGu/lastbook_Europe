//! Place search (`/api/places`, and the assistant): street addresses and streets from the address indexes, towns,
//! postcodes and named places from places.sqlite, postcodes and towns from the address indexes, and lakes and dams from
//! water.sqlite. Names are compared by their [`loose`] keys, so accents, ß and ss, ü and ue all meet.
//!
//! The answer is the array the original Lastbook sent. Its `X-Query-Kind` header says how the engine read the query
//! (`address`, `postcode` or `place`), so the page can say "address not found" without guessing from the text.

use super::address::{self, Kind};
use super::text::{fts_term, loose, name_key, variants, words};
use super::{haversine, param, Map, NATIONAL, PLACES, WATER};
use crate::http::{Request, Response};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

/// Words that name a kind of water, in the languages of the 31 countries: they help ranking but are not searched for,
/// so "Lago di Garda" finds Garda and "Bodensee" stays one word.
const WATER_WORDS: &[&str] = &["lake", "lakes", "reservoir", "res", "dam", "pond", "ponds", "lough", "loch", "tarn", "see", "stausee", "talsperre", "teich",
    "weiher", "lac", "barrage", "étang", "retenue", "lago", "diga", "invaso", "laghetto", "stagno", "bacino", "embalse", "presa", "laguna", "pantano", "llac",
    "embassament", "pantà", "estany", "barragem", "albufeira", "lagoa", "represa", "meer", "plas", "stuwmeer", "vijver", "jezioro", "zbiornik", "zapora", "staw",
    "jezero", "nádrž", "přehrada", "rybník", "jazero", "priehrada", "tó", "víztározó", "lacul", "baraj", "iaz", "езеро", "язовир", "λίμνη", "φράγμα", "sjö",
    "sjön", "damm", "dammen", "vatn", "vatnet", "sjø", "sjøen", "demning", "tjern", "sø", "søen", "järvi", "lampi", "tekojärvi", "järv", "ezers", "ežeras",
    "tvenkinys", "akumulacija", "brana", "għadira"];

fn water_words() -> &'static HashSet<String> {
    static W: std::sync::OnceLock<HashSet<String>> = std::sync::OnceLock::new();
    W.get_or_init(|| WATER_WORDS.iter().map(|w| loose(w)).collect())
}

/// 0 to 8: places near where the person is looking rank higher (strongly within ~60 km, gently out to ~600 km).
fn near_bonus(near: Option<(f64, f64)>, lat: f64, lon: f64) -> f64 {
    let Some((a, b)) = near else { return 0.0 };
    let km = haversine(a, b, lat, lon).0;
    6.0 * (-km / 60.0).exp() + 2.0 * (-km / 600.0).exp()
}

/// All the words, then (for a longer query) each set with one word left out: "echo park lake" still finds "Echo Lake".
fn word_sets(words: &[String]) -> Vec<Vec<String>> {
    let mut out = vec![words.to_vec()];
    if words.len() > 1 {
        for i in 0..words.len() {
            out.push(words.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, w)| w.clone()).collect());
        }
    }
    out
}

/// Has this FTS table the `key` column of the European files? Older files are searched by their own words.
fn has_key(c: &Connection, table: &str) -> bool {
    c.prepare(&format!("SELECT * FROM {table} LIMIT 0")).map(|st| st.column_names().any(|c| c == "key")).unwrap_or(false)
}

/// The FTS5 query for a set of words: each a prefix, in the key's own form or, in an older file, any way it may be
/// written there.
fn fts_query(ws: &[String], keyed: bool) -> String {
    ws.iter()
        .map(|w| if keyed { fts_term(w) } else { format!("({})", variants(w).iter().map(|v| fts_term(v)).collect::<Vec<_>>().join(" OR ")) })
        .collect::<Vec<_>>()
        .join(" ")
}

impl Map {
    pub(super) fn places_route(&self, req: &Request) -> Response {
        let q: String = param(req, "q").unwrap_or_default().chars().take(100).collect();
        let near = param(req, "near").and_then(|v| {
            let p: Vec<f64> = v.split(',').filter_map(|x| x.trim().parse::<f64>().ok()).filter(|f| f.is_finite()).collect();
            (p.len() >= 2).then(|| (p[0], p[1]))
        });
        Response::json(&json!(self.places(&q, 20, near))).with("X-Query-Kind", self.query_kind(&q).as_str())
    }

    /// How the engine reads a query: an address, a postcode, or a place by name (see [`address::parse`]).
    pub fn query_kind(&self, q: &str) -> Kind {
        address::parse(q).kind
    }

    /// Everything the query may name, best first: for an address, its exact house numbers (or the street); otherwise
    /// towns, postcodes and named places, with lakes and dams (first when the query names water).
    pub fn places(&self, q: &str, limit: usize, near: Option<(f64, f64)>) -> Vec<Value> {
        let parsed = address::parse(q);
        if parsed.kind == Kind::Address {
            let found = self.addresses(&parsed, limit.min(8), near);
            if !found.is_empty() {
                return found;
            }
            let found = self.streets(&parsed, limit.min(8), near);
            if !found.is_empty() {
                return found;
            }
        }
        let mut out = self.place_index(q, limit, near);
        let areas = self.areas(&parsed, limit, near);
        if parsed.kind == Kind::Postcode {
            // a postcode from the addresses themselves comes first, in place of the gazetteer's own entry for it
            if !areas.is_empty() {
                let mut merged = areas;
                merged.extend(out.into_iter().filter(|p| !p["kind"].as_str().is_some_and(|k| k.to_lowercase().contains("postcode"))));
                out = merged;
            }
            out.truncate(limit);
            if out.is_empty() {
                out = self.addresses(&parsed, limit.min(8), near);
            }
        } else {
            for a in areas {
                let same = |p: &Value| {
                    name_key(p["name"].as_str().unwrap_or("")) == name_key(a["name"].as_str().unwrap_or(""))
                        && haversine(p["lat"].as_f64().unwrap_or(0.0), p["lon"].as_f64().unwrap_or(0.0), a["lat"].as_f64().unwrap_or(0.0), a["lon"].as_f64().unwrap_or(0.0)).0 < 10.0
                };
                if out.len() < limit && !out.iter().any(same) {
                    out.push(a);
                }
            }
        }
        let water = self.water_places(q, 10, near);
        let n = water.len();
        if n > 0 {
            // a query that names water puts lakes and dams first; otherwise towns lead
            let names_water = words(q).iter().any(|w| water_words().contains(w));
            out = if names_water {
                water.into_iter().chain(out).collect()
            } else {
                let rest = out.split_off(out.len().min(8));
                out.into_iter().chain(water).chain(rest).collect()
            };
        }
        out.truncate(limit + n);
        out
    }

    /// Towns, postcodes and named places from places.sqlite.
    fn place_index(&self, q: &str, limit: usize, near: Option<(f64, f64)>) -> Vec<Value> {
        let Some(c) = self.db(PLACES) else { return Vec::new() };
        let keyed = has_key(&c, "place_fts");
        let words: Vec<String> = if keyed { name_key(q).split_whitespace().map(str::to_string).collect() } else { q.split(|ch: char| !ch.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect() };
        if words.is_empty() {
            return Vec::new();
        }
        type Row = (String, String, String, f64, f64, i64, String);
        let mut rows: Vec<Row> = Vec::new();
        let mut full = false;
        let Ok(mut st) = c.prepare("SELECT p.name,p.kind,p.detail,p.lat,p.lon,p.rank,p.source FROM place_fts f JOIN place p ON p.id=f.rowid \
                                    WHERE place_fts MATCH ?1 ORDER BY p.rank, bm25(place_fts) LIMIT 200") else { return Vec::new() };
        for ws in word_sets(&words) {
            if full && ws.len() < words.len() {
                break; // every word matched somewhere: no need to loosen
            }
            if let Ok(found) = st.query_map([fts_query(&ws, keyed)], |r| {
                Ok((r.get::<_, Option<String>>(0)?.unwrap_or_default(), r.get::<_, Option<String>>(1)?.unwrap_or_default(), r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    r.get(3)?, r.get(4)?, r.get::<_, Option<i64>>(5)?.unwrap_or(9), r.get::<_, Option<String>>(6)?.unwrap_or_default()))
            }) {
                rows.extend(found.flatten());
            }
            if ws.len() == words.len() {
                full = !rows.is_empty();
            }
        }
        let mut seen_rows = HashSet::new();
        rows.retain(|r| seen_rows.insert((r.0.clone(), r.1.clone(), r.2.clone(), r.3.to_bits(), r.4.to_bits(), r.5, r.6.clone())));
        let ql = name_key(q);
        let national = self.db(NATIONAL);
        let mut size: HashMap<(i64, i64), i64> = HashMap::new();
        // the gazetteer may not say how big a place is: the national map's city or town of that name nearby does
        let mut importance = |r: &Row| -> i64 {
            let Some(nat) = &national else { return 3 };
            if name_key(&r.0) != ql || size.len() > 80 {
                return 3;
            }
            let k = ((r.3 * 100.0).round() as i64, (r.4 * 100.0).round() as i64);
            *size.entry(k).or_insert_with(|| {
                nat.query_row(
                    "SELECT MIN(CASE f.cls WHEN 'city' THEN 0 WHEN 'town' THEN 1 ELSE 2 END) FROM feat_idx i JOIN feat f ON f.id=i.id \
                     WHERE i.minx BETWEEN ?1 AND ?2 AND i.miny BETWEEN ?3 AND ?4 AND i.minz<=10 AND f.layer='place' AND f.name=?5",
                    rusqlite::params![r.4 - 0.08, r.4 + 0.08, r.3 - 0.08, r.3 + 0.08, r.0],
                    |x| x.get::<_, Option<i64>>(0),
                )
                .ok()
                .flatten()
                .unwrap_or(2)
            })
        };
        let home = near.or_else(|| self.home_area());
        type Rank = (bool, bool, i64, i64, i64); // not the name, not its start, size, nearness, the gazetteer's rank
        let mut keyed_rows: Vec<(Rank, Row)> = rows
            .into_iter()
            .map(|r| {
                let n = name_key(&r.0);
                let exact = n == ql;
                let imp = if r.5 > 2 && exact { importance(&r) } else { 0 };
                ((!exact, !n.starts_with(&ql), imp, -(near_bonus(home, r.3, r.4).round_ties_even() as i64), r.5), r)
            })
            .collect();
        keyed_rows.sort_by_key(|r| r.0);
        let on_map = self.pack_info().is_some_and(|p| !p.is_null() && p != json!({}) && p != json!([]) && p != json!(""));
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for (_, (name, kind, detail, lat, lon, _, source)) in keyed_rows {
            if !seen.insert((name.to_lowercase(), super::pins::round_to(lat, 2).to_bits(), super::pins::round_to(lon, 2).to_bits())) {
                continue;
            }
            out.push(json!({"name": name, "kind": kind, "detail": detail, "lat": lat, "lon": lon, "source": source, "onMap": on_map}));
            if out.len() >= limit {
                break;
            }
        }
        out
    }

    /// Named lakes, reservoirs and dams matching the query (words for water only help ranking).
    pub fn water_places(&self, q: &str, limit: usize, near: Option<(f64, f64)>) -> Vec<Value> {
        let Some(c) = self.db(WATER) else { return Vec::new() };
        let all = words(q);
        let core: Vec<String> = all.iter().filter(|w| !water_words().contains(*w)).cloned().collect();
        if core.is_empty() {
            return Vec::new();
        }
        let keyed = has_key(&c, "water_fts");
        // an older file is searched by the words as typed, in every way they may be written there
        let typed: Vec<String> = q.split(|ch: char| !ch.is_alphanumeric()).filter(|w| !w.is_empty() && !water_words().contains(&loose(w))).map(str::to_string).collect();
        let searched = if keyed { core.clone() } else { typed };
        type Row = (String, String, f64, f64, Option<String>);
        let mut rows: Vec<Row> = Vec::new();
        let fts = c.prepare("SELECT r.kind,r.name,r.lat,r.lon,r.detail FROM water_fts f JOIN resource r ON r.id=f.rowid WHERE water_fts MATCH ?1 LIMIT 6000");
        match fts {
            Ok(mut st) => {
                let mut full = false;
                for ws in word_sets(&searched) {
                    if !rows.is_empty() && ws.len() < searched.len() && full {
                        break; // every word matched: no need to loosen
                    }
                    if let Ok(found) = st.query_map([fts_query(&ws, keyed)], |r| Ok((r.get(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default(), r.get(2)?, r.get(3)?, r.get(4)?))) {
                        rows.extend(found.flatten());
                    }
                    if ws.len() == searched.len() {
                        full = !rows.is_empty();
                    }
                }
            }
            Err(_) => {
                // the index is not built yet (an import still running): a slower scan
                let cond = core.iter().map(|_| "(r.name LIKE ? OR (r.kind='dam' AND r.detail LIKE ?))").collect::<Vec<_>>().join(" AND ");
                let sql = format!("SELECT r.kind,r.name,r.lat,r.lon,r.detail FROM resource r WHERE r.kind IN ('dam','reservoir','lake') AND r.name!='' AND {cond} LIMIT 300");
                let params: Vec<String> = core.iter().flat_map(|w| [format!("%{w}%"), format!("%{w}%")]).collect();
                if let Ok(mut st) = c.prepare(&sql) {
                    if let Ok(found) = st.query_map(rusqlite::params_from_iter(params.iter()), |r| Ok((r.get(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default(), r.get(2)?, r.get(3)?, r.get(4)?))) {
                        rows.extend(found.flatten());
                    }
                }
            }
        }
        let mut seen_rows = HashSet::new();
        rows.retain(|r| seen_rows.insert((r.0.clone(), r.1.clone(), r.2.to_bits(), r.3.to_bits(), r.4.clone())));
        let ql = loose(q.trim());
        let mut out: Vec<(f64, Value)> = Vec::new();
        for (kind, name, lat, lon, raw) in rows {
            let d: Value = raw.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| json!({}));
            let other = d["other_names"].as_str().unwrap_or("");
            let text = loose(&format!("{name} {other}"));
            let area = super::pins::area_ha(&d).unwrap_or(1.0);
            let score = all.iter().filter(|w| text.contains(w.as_str())).count() as f64 * 2.0
                + f64::from(u8::from(text.contains(core[0].as_str())))
                + f64::from(u8::from(kind != "dam"))
                + f64::from(u8::from(loose(&name) == ql)) * 3.0
                + (area + 1.0).log10() / 3.0
                + near_bonus(near, lat, lon);
            let mut parts: Vec<String> = ["county", "region", "state", "country"].iter().filter_map(|k| d[*k].as_str()).filter(|s| !s.is_empty()).map(str::to_string).collect();
            let place = parts.join(", ");
            parts = vec![place];
            if !other.is_empty() {
                parts.push(format!("also called {other}"));
            }
            if let Some((a, b)) = near {
                let km = haversine(a, b, lat, lon).0;
                parts.push(if km >= 1.0 { format!("{} km away", super::text::thousands(km.round_ties_even() as i64)) } else { "nearby".into() });
            }
            let shown = match kind.as_str() {
                "dam" => "Dam",
                "reservoir" => "Reservoir",
                "lake" => "Lake",
                "pond" => "Pond",
                _ => "Water",
            };
            out.push((score, json!({"name": name, "kind": shown, "detail": parts.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" · "),
                                    "lat": lat, "lon": lon, "source": "water", "onMap": true})));
        }
        out.sort_by(|a, b| b.0.total_cmp(&a.0));
        out.into_iter().take(limit).map(|(_, v)| v).collect()
    }
}
