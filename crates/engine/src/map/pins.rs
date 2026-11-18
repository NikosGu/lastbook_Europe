//! Help points and resources: the clustered pins of the visible area (`/api/map/pins`), the records inside one cluster
//! (`/api/map/members`) and the nearest of each kind (`/api/map/near`, and the assistant). Service points come from the
//! `poi` table of map.sqlite and national.sqlite; resources (lakes, reservoirs, ponds and dams) from the `resource`
//! table of water.sqlite.

use super::text::thousands;
use super::{Map, MAPDB, NATIONAL, WATER};
use rusqlite::types::Value as Sql;
use serde_json::{json, Map as Obj, Value};
use std::collections::HashMap;

/// Kinds of service point drawn on the map's own tiles in the original Lastbook's day; kept for the assistant.
pub const TILE_POI: &[&str] = &["hospital", "clinic", "doctors", "pharmacy", "chemist", "fire_station", "police", "drinking_water", "toilet",
    "shelter", "community_centre", "supermarket", "doityourself", "hardware", "spring"];

/// Every kind of point the map knows, with the name people see. OpenStreetMap's shop=chemist is a drugstore in Europe (dm,
/// Rossmann, Müller): no prescription medicines, so it is never named a pharmacy, which is amenity=pharmacy alone.
pub const HELP: &[(&str, &str)] = &[
    ("hospital", "Hospital"), ("clinic", "Clinic"), ("doctors", "Doctor"), ("pharmacy", "Pharmacy"), ("chemist", "Drugstore"),
    ("dentist", "Dentist"), ("fire_station", "Fire station"), ("police", "Police"), ("drinking_water", "Drinking water tap"),
    ("fountain", "Fountain (not necessarily drinkable)"), ("water_tower", "Water tower"), ("water_works", "Water works"),
    ("spring", "Spring (untested)"), ("toilet", "Public toilet"), ("shelter", "Shelter (structure)"),
    ("community_centre", "Community centre"), ("town_hall", "Town hall"), ("school", "School"), ("library", "Library"),
    ("supermarket", "Supermarket"), ("convenience", "Convenience store"), ("doityourself", "Hardware / DIY store"),
    ("hardware", "Hardware store"), ("garden_centre", "Garden centre"), ("post_office", "Post office"),
    ("camp_site", "Campsite"), ("veterinary", "Vet"), ("nursing_home", "Care home"), ("fuel", "Fuel station"),
    ("car_repair", "Car repair"), ("bicycle_shop", "Bicycle shop"), ("wastewater_plant", "Wastewater plant"),
    ("comms_tower", "Communications tower"), ("park", "Park"), ("courthouse", "Courthouse"),
    ("college", "College"), ("university", "University"), ("kindergarten", "Kindergarten"), ("beach", "Beach"),
    ("embassy", "Embassy"), ("dam", "Dam"), ("reservoir", "Reservoir"), ("lake", "Lake or pond"), ("pond", "Pond"),
];

/// Kinds read from water.sqlite's resource table rather than service points.
pub const RESOURCE_KINDS: &[&str] = &["dam", "reservoir", "lake", "pond"];

const CLUSTER_PX: f64 = 46.0;

/// The name people see for a kind, or None for a kind the map does not know.
pub fn help(cls: &str) -> Option<&'static str> {
    HELP.iter().find(|(k, _)| *k == cls).map(|(_, v)| *v)
}

pub fn is_resource(cls: &str) -> bool {
    RESOURCE_KINDS.contains(&cls)
}

/// Great-circle distance in km, and the compass direction from the first point to the second.
pub fn haversine(la1: f64, lo1: f64, la2: f64, lo2: f64) -> (f64, &'static str) {
    let (p1, p2) = (la1.to_radians(), la2.to_radians());
    let (dp, dl) = (p2 - p1, (lo2 - lo1).to_radians());
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    let d = 2.0 * 6371.0 * a.sqrt().asin();
    let b = ((dl.sin() * p2.cos()).atan2(p1.cos() * p2.sin() - p1.sin() * p2.cos() * dl.cos()).to_degrees() + 360.0) % 360.0;
    const DIRS: [&str; 8] = ["north", "north-east", "east", "south-east", "south", "south-west", "west", "north-west"];
    (d, DIRS[(((b + 22.5) / 45.0).floor() as usize) % 8])
}

/// A number rounded as the original Lastbook rounded it: half to even on the exact value.
pub fn round_to(v: f64, digits: usize) -> f64 {
    format!("{v:.digits$}").parse().unwrap_or(v)
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0)
}

/// Truth as the original Lastbook judged a JSON value: no value, empty text, zero and false are all "nothing there".
fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// What the line about a resource record says, each part a code the interface words in the person's language
/// (web/locales: map.pin.<code>), with its params: its area (`ha`, in hectares: whole from 10 up, else to one decimal),
/// as water.sqlite records it for a lake, reservoir or pond (a dam has none). A pin of one record and a record of a
/// cluster send the parts as `summary_parts`, the "Nearest" panel as `detail_parts`, each with its English `text`. The
/// English line (`summary`, `detail`) stays beside them, for the assistant and for an interface that does not know the
/// codes.
pub const SUMMARY_CODES: [&str; 1] = ["area_ha"];

/// One part of the line: its code (one of [`SUMMARY_CODES`]) and params beside the English.
fn part(code: &str, params: Value, text: impl Into<String>) -> Value {
    debug_assert!(SUMMARY_CODES.contains(&code), "{code} is not in SUMMARY_CODES");
    json!({"code": code, "params": params, "text": text.into()})
}

/// An area as the line gives it: whole hectares from 10 up, else to one decimal.
fn hectares(ha: f64) -> String {
    format!("{} ha", if ha >= 10.0 { thousands(ha.round() as i64) } else { round_to(ha, 1).to_string() })
}

/// The same area as a number, for the interface to write in the person's language.
fn hectares_param(ha: f64) -> Value {
    if ha >= 10.0 {
        json!(ha.round() as i64)
    } else {
        json!(round_to(ha, 1))
    }
}

/// One line about a resource record: its area, when it has one.
pub fn resource_summary(detail: &Value) -> String {
    summary_text(&summary_parts(detail))
}

/// The English line of the parts, as [`resource_summary`] gives it.
fn summary_text(parts: &[Value]) -> String {
    parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join(" · ")
}

/// The line about a resource record as its parts ([`SUMMARY_CODES`]), in the order the English line has them.
fn summary_parts(detail: &Value) -> Vec<Value> {
    area_ha(detail).map(|ha| part("area_ha", json!({"ha": hectares_param(ha)}), hectares(ha))).into_iter().collect()
}

/// A record's area in hectares (`area_ha`), when it has one.
pub fn area_ha(detail: &Value) -> Option<f64> {
    truthy(detail.get("area_ha")).then(|| number(&detail["area_ha"]))
}

fn number(v: &Value) -> f64 {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())).unwrap_or(0.0)
}

fn detail_of(raw: Option<String>) -> Value {
    raw.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| json!({}))
}

/// The same shop is often mapped twice (a pharmacy node and a chemist building): same-name points within ~80 m go.
pub fn dedupe_named(rows: Vec<Value>) -> Vec<Value> {
    let mut kept: HashMap<String, Vec<(f64, f64)>> = HashMap::new();
    let mut out = Vec::new();
    for r in rows {
        if let Some(name) = r["name"].as_str().filter(|n| !n.is_empty()) {
            let near = kept.entry(name.to_lowercase()).or_default();
            let (la, lo) = (f(&r["lat"]), f(&r["lon"]));
            if near.iter().any(|(y, x)| (la - y).abs() < 0.0007 && (lo - x).abs() < 0.0009) {
                continue;
            }
            near.push((la, lo));
        }
        out.push(r);
    }
    out
}

fn by_km(a: &Value, b: &Value) -> std::cmp::Ordering {
    f(&a["km"]).total_cmp(&f(&b["km"]))
}

impl Map {
    /// The file holding the resource records, water.sqlite, when one of these kinds is a resource and the map has it.
    fn resource_db(&self, kinds: &[&str]) -> Option<std::rc::Rc<rusqlite::Connection>> {
        kinds.iter().any(|k| is_resource(k)).then(|| self.db(WATER)).flatten()
    }

    /// The files that hold service points: map.sqlite and national.sqlite.
    fn poi_sources(&self) -> Vec<std::rc::Rc<rusqlite::Connection>> {
        [MAPDB, NATIONAL].iter().filter_map(|f| self.db(f)).collect()
    }

    fn resource_near(&self, lat: f64, lon: f64, cls: &str, limit: usize) -> Vec<Value> {
        let Some(c) = self.resource_db(&[cls]) else { return Vec::new() };
        let Ok(mut st) = c.prepare("SELECT r.name,r.lat,r.lon,r.detail,r.source FROM spatial s CROSS JOIN resource r ON r.id=s.id \
                                     WHERE r.kind=?1 AND s.minx>=?2 AND s.maxx<=?3 AND s.miny>=?4 AND s.maxy<=?5") else { return Vec::new() };
        let mut radius = 0.15;
        type Record = (Option<String>, f64, f64, Option<String>, Option<String>); // name, lat, lon, detail, source
        let mut rows: Vec<Record> = Vec::new();
        while radius <= 19.2 {
            rows = st.query_map(rusqlite::params![cls, lon - radius, lon + radius, lat - radius, lat + radius], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
                .map(|r| r.flatten().collect()).unwrap_or_default();
            if rows.len() >= limit {
                break;
            }
            radius *= 2.0;
        }
        let mut out = Vec::new();
        for (name, y, x, raw, source) in rows {
            let parts = summary_parts(&detail_of(raw));
            let (d, dir) = haversine(lat, lon, y, x);
            out.push(json!({"cls": cls, "label": help(cls), "name": name, "lat": y, "lon": x, "km": round_to(d, 2), "dir": dir, "detail": summary_text(&parts),
                            "detail_parts": parts, "source": source}));
        }
        out.sort_by(by_km);
        out.truncate(limit);
        out
    }

    /// The nearest points of each kind, at most `limit` of each (1 to 50), nearest first: what the "Nearest" panel and
    /// the assistant show.
    pub fn near(&self, lat: f64, lon: f64, classes: &[&str], limit: usize) -> Vec<Value> {
        let limit = limit.clamp(1, 50);
        let mut out = Vec::new();
        for &cls in classes {
            if is_resource(cls) {
                out.extend(self.resource_near(lat, lon, cls, limit));
                continue;
            }
            let sources = self.poi_sources();
            let mut d = 0.02;
            let mut rows: Vec<(Option<String>, Option<String>, f64, f64)> = Vec::new();
            while d < 1.5 && rows.len() < limit {
                let mut found = Vec::new();
                for s in &sources {
                    let Ok(mut st) = s.prepare_cached("SELECT cls,name,lon,lat FROM poi WHERE cls=?1 AND lon BETWEEN ?2 AND ?3 AND lat BETWEEN ?4 AND ?5") else { continue };
                    if let Ok(r) = st.query_map(rusqlite::params![cls, lon - d, lon + d, lat - d, lat + d], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))) {
                        found.extend(r.flatten());
                    };
                }
                // the same point in both files counts once
                let mut seen: HashMap<String, usize> = HashMap::new();
                rows = Vec::new();
                for r in found {
                    let k = format!("{:?}|{:?}|{}|{}", r.0, r.1, round_to(r.2, 5), round_to(r.3, 5));
                    match seen.get(&k) {
                        Some(&i) => rows[i] = r,
                        None => {
                            seen.insert(k, rows.len());
                            rows.push(r);
                        }
                    }
                }
                d *= 2.0;
            }
            for (c, name, x, y) in rows {
                let c = c.unwrap_or_default();
                let (dist, dir) = haversine(lat, lon, y, x);
                out.push(json!({"cls": c, "label": help(&c).unwrap_or(&c), "name": name, "lat": y, "lon": x, "km": round_to(dist, 2), "dir": dir}));
            }
        }
        out.sort_by(by_km);
        let (res, svc): (Vec<Value>, Vec<Value>) = out.into_iter().partition(|p| is_resource(p["cls"].as_str().unwrap_or("")));
        let mut out = res;
        out.extend(dedupe_named(svc));
        out.sort_by(by_km);
        let mut per: HashMap<String, usize> = HashMap::new();
        out.into_iter()
            .filter(|p| {
                let n = per.entry(p["cls"].as_str().unwrap_or("").to_string()).or_default();
                *n += 1;
                *n <= limit
            })
            .collect()
    }

    /// Clustered service and resource points for the visible area. Clusters mix kinds and carry their bounds.
    pub fn pins(&self, west: f64, south: f64, east: f64, north: f64, zoom: f64, kinds: &[&str]) -> Value {
        let zoom = zoom.clamp(3.0, 18.0).round_ties_even();
        let px = if zoom <= 5.0 { 72.0 } else if zoom <= 8.0 { 58.0 } else { CLUSTER_PX };
        let mut cx = 360.0 / 2f64.powf(zoom) * px / 256.0; // coarser groups when a whole region is in view
        let mut cy = cx * ((south + north) / 2.0).to_radians().cos().max(0.2);
        let (mut west, mut south, mut east, mut north) = (west, south, east, north);
        if zoom >= 17.0 {
            cx = 1e-6; // only exact duplicates group at street level
            cy = 1e-6;
        } else {
            // snap to the cluster grid, so panning reuses answers and clusters do not jump
            cy = (cy / cx * 8.0).round_ties_even() / 8.0 * cx;
            west = ((west + 180.0) / cx).floor() * cx - 180.0;
            east = ((east + 180.0) / cx).ceil() * cx - 180.0;
            south = ((south + 90.0) / cy).floor() * cy - 90.0;
            north = ((north + 90.0) / cy).ceil() * cy - 90.0;
        }
        let mut kinds: Vec<&str> = kinds.to_vec();
        kinds.sort_unstable();
        kinds.dedup();
        let (west, south, east, north) = (west.max(-180.0), south.max(-85.0), east.min(180.0), north.min(85.0));
        let key = format!("{}|{}|{}|{}|{}|{}|{}|{}", west.to_bits(), south.to_bits(), east.to_bits(), north.to_bits(), cx.to_bits(), cy.to_bits(), kinds.join(","), self.data_version());
        if let Some(v) = self.pins_cache.lock().unwrap().get(&key) {
            return v;
        }
        let v = self.make_pins(west, south, east, north, cx, cy, &kinds);
        self.pins_cache.lock().unwrap().put(key, v.clone());
        v
    }

    #[allow(clippy::too_many_arguments)]
    fn make_pins(&self, west: f64, south: f64, east: f64, north: f64, cx: f64, cy: f64, kinds: &[&str]) -> Value {
        struct Cell {
            kinds: Vec<(String, i64)>,
            count: i64,
            records: i64,
            lat: f64,
            lon: f64,
            bbox: [f64; 4],
            name: String,
            detail: Value,
            source: String,
            cls: String,
        }
        let services: Vec<&str> = kinds.iter().copied().filter(|k| help(k).is_some() && !is_resource(k)).collect();
        let resources: Vec<&str> = kinds.iter().copied().filter(|k| is_resource(k)).collect();
        let mut cells: Vec<Cell> = Vec::new();
        let mut at: HashMap<(i64, i64), usize> = HashMap::new();
        #[allow(clippy::too_many_arguments)]
        let mut add = |kind: String, n: i64, sites: i64, lat: f64, lon: f64, b: [f64; 4], name: String, detail: Value, source: String| {
            let key = (((lon + 180.0) / cx) as i64, ((lat + 90.0) / cy) as i64);
            let i = *at.entry(key).or_insert_with(|| {
                cells.push(Cell { kinds: Vec::new(), count: 0, records: 0, lat: 0.0, lon: 0.0, bbox: b, name, detail, source, cls: kind.clone() });
                cells.len() - 1
            });
            let c = &mut cells[i];
            c.lat += lat * sites as f64;
            c.lon += lon * sites as f64;
            c.count += sites;
            c.records += n;
            match c.kinds.iter_mut().find(|(k, _)| *k == kind) {
                Some((_, m)) => *m += sites,
                None => c.kinds.push((kind, sites)),
            }
            c.bbox = [c.bbox[0].min(b[0]), c.bbox[1].min(b[1]), c.bbox[2].max(b[2]), c.bbox[3].max(b[3])];
        };
        if !services.is_empty() {
            let q = vec!["?"; services.len()].join(",");
            let sql = format!("SELECT cls,COUNT(*),AVG(lat),AVG(lon),MIN(lon),MAX(lon),MIN(lat),MAX(lat),MIN(name) FROM poi \
                               WHERE cls IN ({q}) AND lon BETWEEN ? AND ? AND lat BETWEEN ? AND ? \
                               GROUP BY cls, CAST((lon+180)/? AS INT), CAST((lat+90)/? AS INT)");
            let mut params: Vec<Sql> = services.iter().map(|s| Sql::Text(s.to_string())).collect();
            params.extend([west, east, south, north, cx, cy].map(Sql::Real));
            for c in self.poi_sources() {
                let Ok(mut st) = c.prepare(&sql) else { continue };
                let Ok(rows) = st.query_map(rusqlite::params_from_iter(params.iter()), |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, f64>(2)?, r.get::<_, f64>(3)?, [r.get::<_, f64>(4)?, r.get::<_, f64>(6)?, r.get::<_, f64>(5)?, r.get::<_, f64>(7)?], r.get::<_, Option<String>>(8)?))
                }) else { continue };
                for (k, n, la, lo, b, name) in rows.flatten() {
                    add(k, n, n, la, lo, b, name.unwrap_or_default(), json!({}), String::new());
                }
            }
        }
        if let Some(c) = self.resource_db(&resources) {
            let sql = format!("SELECT r.kind,COUNT(*),AVG(r.lat),AVG(r.lon),MIN(r.lon),MAX(r.lon),MIN(r.lat),MAX(r.lat),r.name,r.detail,r.source,COUNT(DISTINCT r.source_id) \
                               FROM spatial s CROSS JOIN resource r ON r.id=s.id WHERE s.minx>=? AND s.maxx<=? AND s.miny>=? AND s.maxy<=? \
                               AND r.kind IN ({}) GROUP BY r.kind, CAST((r.lon+180)/? AS INT), CAST((r.lat+90)/? AS INT)", vec!["?"; resources.len()].join(","));
            let mut params: Vec<Sql> = [west, east, south, north].map(Sql::Real).to_vec();
            params.extend(resources.iter().map(|s| Sql::Text(s.to_string())));
            params.extend([cx, cy].map(Sql::Real));
            type Group = (String, i64, f64, f64, [f64; 4], Option<String>, Option<String>, Option<String>, i64); // kind, records, lat, lon, bounds, name, detail, source, sites
            let rows: Vec<Group> = c
                .prepare(&sql)
                .and_then(|mut st| {
                    st.query_map(rusqlite::params_from_iter(params.iter()), |r| {
                        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, [r.get(4)?, r.get(6)?, r.get(5)?, r.get(7)?], r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?))
                    })
                    .map(|r| r.flatten().collect())
                })
                .unwrap_or_default();
            for (k, n, la, lo, b, name, detail, source, sites) in rows {
                let detail = if sites == 1 { detail_of(detail) } else { json!({}) };
                add(k, n, sites, la, lo, b, name.unwrap_or_default(), detail, source.unwrap_or_default());
            }
        }
        let mut out: Vec<Value> = Vec::new();
        for c in cells.into_iter().filter(|c| c.count > 0) {
            let mut o = Obj::new();
            let (lat, lon) = (c.lat / c.count as f64, c.lon / c.count as f64);
            let spot = c.count > 1 && c.bbox[2] - c.bbox[0] < 1e-4 && c.bbox[3] - c.bbox[1] < 1e-4; // several places at one point
            if c.count == 1 {
                let parts = summary_parts(&c.detail);
                o.insert("label".into(), json!(help(&c.cls).unwrap_or(&c.cls)));
                o.insert("summary".into(), json!(summary_text(&parts)));
                o.insert("summary_parts".into(), json!(parts));
                o.insert("cls".into(), json!(c.cls));
                o.insert("name".into(), json!(c.name));
                o.insert("detail".into(), c.detail);
                o.insert("source".into(), json!(c.source));
            } else {
                let top = c.kinds.iter().fold(None::<&(String, i64)>, |m, k| if m.is_none_or(|m| k.1 > m.1) { Some(k) } else { m });
                o.insert("cls".into(), json!(top.map(|t| t.0.clone()).unwrap_or(c.cls)));
                for k in ["name", "detail", "source"] {
                    o.insert(k.into(), Value::Null);
                }
            }
            o.insert("kinds".into(), Value::Object(c.kinds.into_iter().map(|(k, n)| (k, json!(n))).collect()));
            o.insert("count".into(), json!(c.count));
            o.insert("records".into(), json!(c.records));
            o.insert("lat".into(), json!(lat));
            o.insert("lon".into(), json!(lon));
            o.insert("bbox".into(), json!(c.bbox));
            o.insert("spot".into(), json!(spot));
            out.push(Value::Object(o));
        }
        out.sort_by_key(|c| -c["count"].as_i64().unwrap_or(0));
        let total: i64 = out.iter().map(|c| c["count"].as_i64().unwrap_or(0)).sum();
        let truncated = out.len() > 2500;
        out.truncate(2500);
        json!({"points": out, "sites": total, "truncated": truncated, "cell": [cx, cy]})
    }

    /// The single records inside a cluster's bounds, nearest to its centre first.
    #[allow(clippy::too_many_arguments)]
    pub fn members(&self, west: f64, south: f64, east: f64, north: f64, kinds: &[&str], lat: f64, lon: f64, limit: usize) -> Value {
        let services: Vec<&str> = kinds.iter().copied().filter(|k| help(k).is_some() && !is_resource(k)).collect();
        let resources: Vec<&str> = kinds.iter().copied().filter(|k| is_resource(k)).collect();
        let eps = 1e-4; // the R-tree keeps float32 bounds (~1e-5 degrees off)
        let mut rows: Vec<Value> = Vec::new();
        if !services.is_empty() {
            let sql = format!("SELECT cls,name,lon,lat FROM poi WHERE cls IN ({}) AND lon BETWEEN ? AND ? AND lat BETWEEN ? AND ? LIMIT 2000", vec!["?"; services.len()].join(","));
            let mut params: Vec<Sql> = services.iter().map(|s| Sql::Text(s.to_string())).collect();
            params.extend([west - eps, east + eps, south - eps, north + eps].map(Sql::Real));
            for c in self.poi_sources() {
                let Ok(mut st) = c.prepare(&sql) else { continue };
                let Ok(found) = st.query_map(rusqlite::params_from_iter(params.iter()), |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, f64>(2)?, r.get::<_, f64>(3)?))) else { continue };
                for (c, n, x, y) in found.flatten() {
                    rows.push(json!({"cls": c, "label": help(&c).unwrap_or(&c), "name": n.unwrap_or_default(), "lon": x, "lat": y, "summary": "", "summary_parts": [],
                                     "detail": {}, "source": ""}));
                }
            }
        }
        if let Some(c) = self.resource_db(&resources) {
            let sql = format!("SELECT r.kind,r.name,r.lat,r.lon,r.detail,r.source FROM spatial s CROSS JOIN resource r ON r.id=s.id \
                               WHERE s.minx>=? AND s.maxx<=? AND s.miny>=? AND s.maxy<=? AND r.kind IN ({}) LIMIT 2000", vec!["?"; resources.len()].join(","));
            let mut params: Vec<Sql> = [west - eps, east + eps, south - eps, north + eps].map(Sql::Real).to_vec();
            params.extend(resources.iter().map(|s| Sql::Text(s.to_string())));
            type Record = (String, Option<String>, f64, f64, Option<String>, Option<String>); // kind, name, lat, lon, detail, source
            let found: Vec<Record> = c
                .prepare(&sql)
                .and_then(|mut st| st.query_map(rusqlite::params_from_iter(params.iter()), |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))).map(|r| r.flatten().collect()))
                .unwrap_or_default();
            for (k, n, y, x, d, src) in found {
                let detail = detail_of(d);
                let parts = summary_parts(&detail);
                rows.push(json!({"cls": k, "label": help(&k), "name": n.unwrap_or_default(), "lat": y, "lon": x, "summary": summary_text(&parts), "summary_parts": parts,
                                 "detail": detail, "source": src.unwrap_or_default()}));
            }
        }
        let (res, svc): (Vec<Value>, Vec<Value>) = rows.into_iter().partition(|r| is_resource(r["cls"].as_str().unwrap_or("")));
        let mut rows = res;
        rows.extend(dedupe_named(svc));
        for r in rows.iter_mut() {
            let (d, dir) = haversine(lat, lon, f(&r["lat"]), f(&r["lon"]));
            r["km"] = json!(d);
            r["dir"] = json!(dir);
        }
        // nearest first
        rows.sort_by(|a, b| round_to(f(&a["km"]), 3).total_cmp(&round_to(f(&b["km"]), 3)));
        let total = rows.len();
        rows.truncate(limit);
        json!({"items": rows, "total": total, "capped": total >= 2000})
    }
}
