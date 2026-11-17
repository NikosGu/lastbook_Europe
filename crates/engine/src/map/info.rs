//! What the map holds and says about itself (`/api/map/info`), the data version tiles are cached under, the open pack's
//! description, a household's home area, and what the assistant can and cannot read (`coverage`).

use super::{Map, ATLAS, BUILDINGS, CROPS, MAPDB, NATIONAL, PLACES, STATUS, TERRAIN, WATER};
use crate::regions;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn secs(t: SystemTime) -> f64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

fn sql_json(v: rusqlite::types::Value) -> Value {
    use rusqlite::types::Value as V;
    match v {
        V::Null => Value::Null,
        V::Integer(i) => json!(i),
        V::Real(r) => json!(r),
        V::Text(s) => json!(s),
        V::Blob(_) => Value::Null,
    }
}

impl Map {
    /// The national map's build state (`derived/geo/national-status.json`), or `{"state": "not-built"}`.
    pub fn national_status(&self) -> Value {
        self.path(STATUS)
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .filter(|v| v.is_object())
            .unwrap_or_else(|| json!({"state": "not-built"}))
    }

    /// Changes whenever map data changes: every 20 s while the national map builds, and when the water, crop,
    /// elevation or building files are written. Tiles and pins are cached under it.
    pub fn data_version(&self) -> String {
        let status = self.national_status();
        let mut v = if status["state"] == "building" {
            format!("n{}", (secs(SystemTime::now()) / 20.0).floor() as i64)
        } else {
            match &status["features"] {
                Value::Null => "n0".to_string(),
                Value::String(s) => format!("n{s}"),
                n => format!("n{n}"),
            }
        };
        for f in [WATER.to_string(), format!("{WATER}-wal"), CROPS.to_string(), TERRAIN.to_string(), BUILDINGS.to_string(), format!("{BUILDINGS}-wal")] {
            if let Some(m) = self.path(&f).and_then(|p| std::fs::metadata(p).ok()).and_then(|m| m.modified().ok()) {
                v.push_str(&format!(".{}", secs(m) as i64 / 60));
            }
        }
        v
    }

    /// The open download's own description (`pack.json`), or None when this is not a pack. Read once: a pack does not
    /// change while it is open.
    pub fn pack_info(&self) -> Option<Value> {
        self.pack
            .get_or_init(|| self.path("pack.json").and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()))
            .clone()
    }

    /// A household's own opening place (`web/home-area.json`, a local file that is never packaged), as latitude and
    /// longitude, for ranking place names. None everywhere else.
    pub fn home_area(&self) -> Option<(f64, f64)> {
        let v: Value = serde_json::from_slice(&std::fs::read(self.paths.web().join("home-area.json")).ok()?).ok()?;
        let num = |k: &str| v.get(k).and_then(|x| x.as_f64().or_else(|| x.as_str().and_then(|s| s.trim().parse().ok())));
        Some((num("lat")?, num("lon")?))
    }

    fn distinct(&self, file: &str, sql: &str) -> Vec<String> {
        let Some(c) = self.db(file) else { return Vec::new() };
        let Ok(mut st) = c.prepare(sql) else { return Vec::new() };
        st.query_map([], |r| r.get::<_, Option<String>>(0)).map(|r| r.flatten().flatten().collect()).unwrap_or_default()
    }

    /// What this map really holds, decided from its own files: which pin kinds (`layers`), the box its service points
    /// fall in (`bounds`, None for a map of everything), whether it has street detail, and whether it has contours.
    /// Worked out once.
    pub fn map_contents(&self) -> Value {
        self.contents
            .get_or_init(|| {
                let mut kinds = BTreeSet::new();
                let mut boxes: Vec<[f64; 4]> = Vec::new();
                for file in [MAPDB, NATIONAL] {
                    kinds.extend(self.distinct(file, "SELECT DISTINCT cls FROM poi"));
                    if let Some(c) = self.db(file) {
                        let b = c.query_row("SELECT MIN(lon),MIN(lat),MAX(lon),MAX(lat) FROM poi", [], |r| {
                            Ok([r.get::<_, Option<f64>>(0)?, r.get(1)?, r.get(2)?, r.get(3)?])
                        });
                        if let Ok([Some(a), Some(b), Some(c), Some(d)]) = b {
                            boxes.push([a, b, c, d]);
                        }
                    }
                }
                for file in [ATLAS, WATER, CROPS] {
                    kinds.extend(self.distinct(file, "SELECT DISTINCT kind FROM resource"));
                }
                let contours = self.db(TERRAIN).is_some_and(|c| c.query_row("SELECT 1 FROM tile WHERE z>=11 LIMIT 1", [], |_| Ok(())).is_ok());
                let mut bounds = boxes.iter().copied().reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]);
                if bounds.is_some_and(|b| b[2] - b[0] > 90.0) {
                    bounds = None; // a map of everything: fitting it would show the whole world
                }
                json!({"layers": kinds.into_iter().collect::<Vec<_>>(), "bounds": bounds, "detail": !boxes.is_empty(), "contours": contours})
            })
            .clone()
    }

    /// What the map can show and where its data comes from, from the map set's own files.
    pub fn map_info(&self) -> Value {
        let meta: serde_json::Map<String, Value> = self
            .db(MAPDB)
            .and_then(|c| {
                let mut st = c.prepare("SELECT key,value FROM meta").ok()?;
                let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, rusqlite::types::Value>(1)?))).ok()?;
                Some(rows.flatten().map(|(k, v)| (k, sql_json(v))).collect())
            })
            .unwrap_or_default();
        let mut info = self.map_contents();
        info["aerial"] = json!(self.has_aerial());
        info["datasets"] = json!(self.map_datasets(&info));
        let mut national = self.national_status();
        if let Some(o) = national.as_object_mut() {
            o.remove("source"); // paths on the machine that built it
            o.remove("database");
        }
        info["meta"] = Value::Object(meta);
        info["national"] = national;
        info["version"] = json!(self.data_version());
        for k in ["aerial_detail", "aerial_rev", "terrain_circle"] {
            info[k] = Value::Null;
        }
        info
    }

    /// The publisher a map file names in its meta table (the builders write it), or ''.
    pub fn meta_source(&self, file: &str, key: &str) -> String {
        self.db(file)
            .and_then(|c| c.query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get::<_, rusqlite::types::Value>(0)).ok())
            .map(|v| match sql_json(v) {
                Value::String(s) => s,
                Value::Null => String::new(),
                v => v.to_string(),
            })
            .unwrap_or_default()
    }

    /// The panel beside the map says only what this map set's files hold, and names the sources they name themselves.
    fn map_datasets(&self, have: &Value) -> Vec<Value> {
        let bounded = !have["bounds"].is_null();
        let area = if bounded { "the area this map details" } else { "the whole map" };
        let with = |label: &str, src: String| if src.is_empty() { label.to_string() } else { format!("{label} ({src})") };
        let mut out = vec![json!({"label": "Main roads, towns and water (OpenStreetMap, Natural Earth)", "status": "Shown at every zoom level"})];
        if have["detail"] == true {
            out.push(json!({"label": format!("Streets, places and help points (OpenStreetMap) for {area}"),
                            "status": if bounded { "Street detail stops at the edge of this area." } else { "Shown when you zoom in" }}));
        } else {
            out.push(json!({"label": "No street detail or help points", "status": "This download has only the base map. Hospitals, water taps and side streets are not in it."}));
        }
        if self.exists(PLACES) {
            out.push(json!({"label": "Place names and postcodes", "status": "Used by \"Find a place\""}));
        }
        let regions: Vec<String> = self.address_files().iter().filter_map(|p| p.file_stem()?.to_str().map(str::to_string)).collect();
        if let Some(first) = self.address_files().first() {
            let src = self.db_at(first).and_then(|c| c.query_row("SELECT value FROM meta WHERE key='publisher'", [], |r| r.get::<_, String>(0)).ok()).unwrap_or_default();
            let names: Vec<String> = regions.iter().map(|r| regions::name(r)).collect();
            out.push(json!({"label": with("Street addresses", src), "status": format!("Used by \"Find a place\": house numbers in {}", names.join(", "))}));
        }
        if self.exists(TERRAIN) {
            out.push(json!({"label": with("Elevation", self.meta_source(TERRAIN, "source")),
                            "status": if have["detail"] == true { "Shaded relief. Contour lines only where this map has fine detail." } else { "Shaded relief at broad zoom levels" }}));
        }
        if self.exists(BUILDINGS) {
            let src = self.meta_source(BUILDINGS, "source");
            out.push(json!({"label": with("Building outlines", if src.is_empty() { "OpenStreetMap".into() } else { src }), "status": format!("For {area}, shown from street level zoom")}));
        }
        if self.exists(CROPS) {
            out.push(json!({"label": with("Orchards, vineyards and olive groves", self.meta_source(CROPS, "source")), "status": format!("For {area}")}));
        }
        out
    }

    /// What the assistant can and cannot read, reported rather than hidden: videos with text, PDFs that are only page
    /// images, documents the semantic index could not read. Worked out again after five minutes.
    pub fn coverage(&self) -> Value {
        let mut cache = self.coverage_cache.lock().unwrap();
        if let Some((at, v)) = cache.as_ref() {
            if at.elapsed() < Duration::from_secs(300) {
                return v.clone();
            }
        }
        let v = self.work_out_coverage();
        *cache = Some((Instant::now(), v.clone()));
        v
    }

    fn work_out_coverage(&self) -> Value {
        let read = |rel: &str| self.path(rel).and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice::<Value>(&b).ok());
        let mut ids: HashSet<String> = HashSet::new();
        if let Some(Value::Object(v)) = read("library-data/video-exclusions.json").map(|e| e["videos"].clone()) {
            for k in v.keys() {
                ids.insert(k.clone());
                ids.insert(format!("metadata-{k}"));
            }
        }
        let recs: Vec<Value> = read("library-data/catalog.json").and_then(|v| v.as_array().cloned()).unwrap_or_default();
        let s = |r: &Value, k: &str| r[k].as_str().unwrap_or("").to_string();
        // rejected videos never count, nor what belongs to them
        let bad: HashSet<String> = recs.iter().filter(|r| ids.contains(&s(r, "id"))).map(|r| s(r, "path")).collect();
        let mut by_id: HashMap<String, Value> = HashMap::new();
        for r in recs {
            let video_path = s(&r, "video_path");
            if ids.contains(&s(&r, "id")) || ids.contains(&s(&r, "video_id")) || (!video_path.is_empty() && bad.contains(&video_path)) {
                continue;
            }
            by_id.insert(s(&r, "id"), r);
        }
        let tx: HashSet<String> = by_id.values().filter(|r| r["kind"] == "transcript" && !s(r, "video_path").is_empty()).map(|r| s(r, "video_path")).collect();
        let vids: Vec<&Value> = by_id.values().filter(|r| r["kind"] == "video").collect();
        let with_text = vids
            .iter()
            .filter(|r| tx.contains(&s(r, "path")) || r["captions"].as_array().is_some_and(|c| c.iter().any(|c| c.as_str().is_some_and(|c| c.ends_with(".vtt")))))
            .count();
        let pdfs: Vec<&Value> = by_id.values().filter(|r| r["kind"] == "pdf").collect();
        let image_only = pdfs.iter().filter(|r| s(r, "searchability").starts_with("image-only")).count();
        let gaps: i64 = self.db("derived/index/semantic.sqlite").and_then(|c| c.query_row("SELECT count(*) FROM gaps", [], |r| r.get(0)).ok()).unwrap_or(0);
        json!({"videos": vids.len(), "videos_with_text": with_text, "pdfs": pdfs.len(), "pdfs_image_only": image_only,
               "docs_without_text": gaps, "remote_videos": 0, "videos_excluded": ids.iter().filter(|i| i.starts_with("video-")).count()})
    }
}
