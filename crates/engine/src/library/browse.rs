//! Browsing the whole collection: shelves in plain words, one collection's items, and the video rows. Ported from the
//! original Lastbook's server.py (SHELVES, shelf_of, shelves, shelf, collection_items, VROWS, vrow_of, vcard, vsort,
//! guide_videos, videos_home, videos_list, videos_related). Typed filters fold accents, as search does.
//!
//! The interface says a shelf's or a row's title by its id (web/locales: app.shelf.name.<id>, app.vrow.name.<id>) and a
//! row's note by its `note_code` (app.vrow.note.<code>); the English title and note stay beside them.

use super::catalog::Record;
use super::topics::GuideOrder;
use super::words::fold;
use super::Library;
use regex::Regex;
use serde_json::{json, Value};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock};

/// (id, title, pattern on the catalog category). Food's `garden` is not `garden-power`, which is checked in code.
const SHELVES: [(&str, &str, &str); 12] = [
    ("health", "Health & first aid", r"medic|first-aid|care|health|dent|mental|obstet|midwi|clinic|surg"),
    ("water", "Water & sanitation", r"water|rain|sanit|wash|hygien"),
    ("food", "Food: growing, keeping & cooking", r"agri|food|seed|crop|livestock|dairy|ferment|bee|farm|soil|grow|irrigat|oil-crop|grain"),
    ("safety", "Disasters, safety & outdoors", r"survival|shelter|disaster|prepar|safety|hazard|wildfire|knot|weather|polar|defen|radiolog|rescue|fire"),
    ("power", "Power & energy", r"energy|power|solar|generator|offgrid|battery|inverter|charge-controller"),
    ("comms", "Radio, communication & navigation", r"communic|radio|navigation"),
    ("repair", "Tools, repair & equipment", r"engine|mower|tractor|vehicle|hvac|compressor|bicycle|aerial|backhoe|bearing|sharpen|weld|repair|equipment|seal|pump|snow|motorcycle|truck|car|marine|forklift|workshop|hand-tool"),
    ("making", "Making things & materials", r"manufact|craft|glass|ceram|textil|wood|paper|metal|materials|machin|leather|amate|lime|refractor|typeset|mill|mineral|production|raw-material"),
    ("build", "Building & infrastructure", r"civil|infrastructure|building"),
    ("community", "Community, teaching & recovery", r"communit|educat|teach|accessib|recover|accountab|organiz|continuity|inventory"),
    ("science", "Science, maths & engineering courses", r"math|physic|chemistr|biolog|astronom|earth|ecolog|science|engineer|control|metrolog|quality|thermo|electromag|circuit|microelec|comput|digital|space|aviation|aircraft|laborator|measurement"),
    ("creators", "Bushcraft & primitive-skills videos", r"^creator-"),
];
/// The shelf of everything no other shelf takes.
const OTHER: (&str, &str) = ("other", "Other references");
static SHELF_RX: LazyLock<Vec<Regex>> = LazyLock::new(|| SHELVES.iter().map(|s| Regex::new(s.2).unwrap()).collect());
static LONG_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\p{L}{4,}").unwrap());

/// (id, title, the exact catalog categories): practical first, hobby channels and courses after.
const VROWS: [(&str, &str, &str); 12] = [
    ("health", "First aid & health", "first-aid medical care-continuity"),
    ("water", "Water & sanitation", "water-safety-planning water-treatment rainwater water-sanitation water-sampling sanitary-inspection pumps"),
    ("food", "Growing & preserving food", "food-preservation agriculture food-growing seed-saving grain-mill-assembly grain-mill-maintenance grain-mill-operation"),
    ("power", "Power, generators & solar", "generator generators offgrid-power"),
    ("outdoors", "Shelter, fire & outdoor skills", "field-survival fire-and-shelter knots weather communications-navigation"),
    ("repair", "Tools, engines & repair", "small-engine-repair lawn-mower garden-power-equipment snow-blower woodworking welding welding-equipment sharpening workshop milling-layout-and-fit bearing-maintenance seals-installation road-vehicles marine heavy-ag-machinery machining-tool-geometry measurement"),
    ("making", "Making things by hand", "craft-manufacturing-including-ceramics-and-textiles glass-manufacturing-processes refractory-installation refractory-process-control typesetting-printing-binding bark-to-amate-paper"),
    ("coalcracker", "Coalcracker Bushcraft", "creator-coalcracker-bushcraft"),
    ("corporals", "Corporals Corner", "creator-corporals-corner"),
    ("canterbury", "Dave Canterbury", "creator-david-canterbury"),
    ("primitive", "Primitive Technology", "creator-primitive-technology"),
    ("courses", "Courses, lectures & more", ""),
];
/// The rows that are not one kind of video: the guides' picks, and every video. (id, title)
const GUIDES: (&str, &str) = ("guides", "Picked for the guides");
const ALL: (&str, &str) = ("all", "All videos");
/// The rows' notes: (code, English). A row's own note has the row's id as its code; the note the channels share, a code of
/// its own.
const NOTES: [(&str, &str); 4] = [
    ("guides", "Chosen to go with the survival guides"),
    ("hobbyist", "Hobbyist channel"),
    ("primitive", "Hobbyist channel — no narration"),
    ("courses", "University lectures (MIT and others)"),
];
/// The guides' row's note, which it has on the videos page only.
const GUIDES_NOTE: (&str, &str) = NOTES[0];

/// A row's note, as (code, English).
fn vrow_note(id: &str) -> Option<(&'static str, &'static str)> {
    let code = match id {
        "coalcracker" | "corporals" | "canterbury" => "hobbyist",
        "primitive" | "courses" => id,
        _ => return None,
    };
    NOTES.iter().copied().find(|n| n.0 == code)
}

/// The shelf a record is on, from its category.
pub fn shelf_of(r: &Record) -> &'static str {
    let c = r.str("category").to_lowercase();
    if c.starts_with("creator-") {
        return "creators";
    }
    for (i, (id, _, _)) in SHELVES.iter().enumerate() {
        if *id == "creators" {
            continue;
        }
        let garden = *id == "food" && c.match_indices("garden").any(|(at, _)| !c[at + 6..].starts_with("-power"));
        if garden || SHELF_RX[i].is_match(&c) {
            return id;
        }
    }
    "other"
}

/// The video row a record is in, from its exact category.
pub fn vrow_of(r: &Record) -> &'static str {
    let c = r.str("category");
    VROWS.iter().find(|(_, _, cats)| cats.split(' ').any(|x| x == c)).map(|v| v.0).unwrap_or("courses")
}

/// A number field, or 0.
fn num(r: &Record, k: &str) -> f64 {
    r.get(k).as_f64().unwrap_or(0.0)
}

fn by_text(a: &str, b: &str) -> Ordering {
    a.cmp(b)
}

fn offset_page<T: Clone>(items: &[T], offset: usize, limit: usize) -> (Vec<T>, bool) {
    let start = offset.min(items.len());
    (items[start..(offset + limit).min(items.len())].to_vec(), items.len() > offset + limit)
}

impl Library {
    /// Each shelf with how many things to read and to watch are on it.
    pub fn shelves(&self) -> Value {
        let cat = self.catalog();
        let mut counts: HashMap<&str, (u64, u64)> = HashMap::new();
        for r in &cat.records {
            if matches!(r.kind.as_str(), "metadata" | "transcript" | "zim") {
                continue;
            }
            let c = counts.entry(shelf_of(r)).or_default();
            if r.kind == "video" {
                c.1 += 1;
            } else {
                c.0 += 1;
            }
        }
        let row = |id: &str, title: &str| {
            let (read, video) = counts.get(id).copied().unwrap_or_default();
            json!({"id": id, "title": title, "read": read, "video": video})
        };
        let mut out: Vec<Value> = SHELVES.iter().map(|(id, t, _)| row(id, t)).collect();
        out.push(row(OTHER.0, OTHER.1));
        Value::Array(out)
    }

    /// One shelf's collections, most trusted first, each with its first six items. `kind` is `read`, `video` or
    /// `all`; `q` filters titles, collections and publishers.
    pub fn shelf(&self, sid: &str, kind: &str, q: &str, offset: usize) -> Value {
        let limit = 40;
        let cat = self.catalog();
        let allowed: Option<&[&str]> = match kind {
            "read" => Some(&["pdf", "html", "pptx"]),
            "video" => Some(&["video", "transcript"]),
            _ => None,
        };
        let fq = fold(q);
        let mut rs: Vec<&Arc<Record>> = cat
            .records
            .iter()
            .filter(|r| !matches!(r.kind.as_str(), "metadata" | "transcript" | "zim") && shelf_of(r) == sid)
            .filter(|r| allowed.is_none_or(|a| a.contains(&r.kind.as_str())))
            .filter(|r| fq.is_empty() || fold(&format!("{} {} {}", r.title, r.str("collection"), r.str("publisher"))).contains(&fq))
            .collect();
        rs.sort_by(|a, b| b.trust.1.partial_cmp(&a.trust.1).unwrap_or(Ordering::Equal).then(by_text(a.str("collection"), b.str("collection"))).then(by_text(&a.title, &b.title)));
        // a 300-chapter manual shows as one entry
        let mut groups: Vec<(String, Vec<&Arc<Record>>)> = Vec::new();
        let mut at: HashMap<String, usize> = HashMap::new();
        for r in &rs {
            let c = if r.truthy("collection") { r.str("collection").to_string() } else { "Other".to_string() };
            let i = *at.entry(c.clone()).or_insert_with(|| {
                groups.push((c, Vec::new()));
                groups.len() - 1
            });
            groups[i].1.push(r);
        }
        let lib = self.lib().unwrap_or_default();
        let items: Vec<Value> = groups
            .iter()
            .map(|(c, v)| json!({"collection": c, "count": v.len(), "trust": v[0].trust.0, "trust_code": v[0].trust_code, "items": v.iter().take(6).map(|x| cat.public(x, false, &lib)).collect::<Vec<_>>()}))
            .collect();
        let (page, more) = offset_page(&items, offset, limit);
        json!({"total": rs.len(), "collections": page, "more": more})
    }

    /// Everything in one collection, by title.
    pub fn collection_items(&self, name: &str, offset: usize) -> Value {
        let cat = self.catalog();
        let mut rs: Vec<&Arc<Record>> = cat.records.iter().filter(|r| r.str("collection") == name && r.get("collection").is_string() && r.kind != "metadata").collect();
        rs.sort_by(|a, b| a.title.cmp(&b.title));
        let lib = self.lib().unwrap_or_default();
        let (page, more) = offset_page(&rs, offset, 60);
        json!({"total": rs.len(), "items": page.iter().map(|r| cat.public(r, false, &lib)).collect::<Vec<_>>(), "more": more})
    }

    /// A video as a card.
    fn vcard(&self, r: &Record) -> Value {
        let p = self.public(r, false);
        let mut o = serde_json::Map::new();
        for k in ["id", "title", "publisher", "duration", "thumb", "transcript", "trust", "trust_code", "year"] {
            o.insert(k.into(), p.get(k).cloned().unwrap_or(Value::Null));
        }
        Value::Object(o)
    }

    /// Videos in order: most trusted, then with a transcript, then with a picture, then by title.
    fn vsort(&self, rs: &mut [&Arc<Record>]) {
        let cat = self.catalog();
        let key = |r: &Record| (cat.transcript_of(&r.path).is_none() && !r.truthy("captions"), cat.thumb_of(&r.path).is_none_or(str::is_empty));
        rs.sort_by(|a, b| {
            b.trust.1.partial_cmp(&a.trust.1).unwrap_or(Ordering::Equal).then(key(a).cmp(&key(b))).then(a.title.cmp(&b.title))
        });
    }

    /// The videos the guides name, each once, with its guide.
    pub fn guide_videos(&self) -> Vec<Value> {
        let cat = self.catalog();
        let (mut out, mut seen) = (Vec::new(), HashSet::new());
        for t in self.topics() {
            for i in Library::guide_items(&t, GuideOrder::Videos) {
                let Some(r) = i["doc"].as_str().and_then(|d| cat.get(d)) else { continue };
                if r.kind == "video" && seen.insert(r.id.clone()) {
                    let mut c = self.vcard(r);
                    c["guide"] = t["id"].clone();
                    c["guideTitle"] = t.get("title").cloned().unwrap_or(Value::Null);
                    out.push(c);
                }
            }
        }
        out
    }

    /// The videos page: the guides' picks, then a row per kind of video with its first 16.
    pub fn videos_home(&self) -> Value {
        let cat = self.catalog();
        let mut rows: HashMap<&str, Vec<&Arc<Record>>> = HashMap::new();
        for r in cat.records.iter().filter(|r| r.kind == "video") {
            rows.entry(vrow_of(r)).or_default().push(r);
        }
        let guides = self.guide_videos();
        let mut res = vec![json!({"id": GUIDES.0, "title": GUIDES.1, "note": GUIDES_NOTE.1, "note_code": GUIDES_NOTE.0, "count": guides.len(),
                                  "items": guides.iter().take(16).cloned().collect::<Vec<_>>()})];
        for (vid, title, _) in VROWS {
            let mut rs = rows.remove(vid).unwrap_or_default();
            self.vsort(&mut rs);
            let note = vrow_note(vid);
            if matches!(note, Some(("hobbyist" | "primitive", _))) {
                rs.sort_by(|a, b| b.str("upload_date").cmp(a.str("upload_date"))); // channels: newest first, not A–Z
            }
            if !rs.is_empty() {
                res.push(json!({"id": vid, "title": title, "note": note.map(|n| n.1), "note_code": note.map(|n| n.0), "count": rs.len(),
                                "items": rs.iter().take(16).map(|r| self.vcard(r)).collect::<Vec<_>>()}));
            }
        }
        Value::Array(res)
    }

    /// One row of videos (or `all`, or `guides`), filtered by words in the title or publisher and by length
    /// (`short`, `medium`, `long`), sorted (`short`, `new`, `az`, else as the row is).
    pub fn videos_list(&self, row: &str, q: &str, length: &str, sort: &str, offset: usize) -> Value {
        let cat = self.catalog();
        let mut rs: Vec<&Arc<Record>> = if row == "guides" {
            self.guide_videos().iter().filter_map(|v| v["id"].as_str().and_then(|id| cat.get(id))).collect()
        } else {
            cat.records.iter().filter(|r| r.kind == "video" && (row.is_empty() || row == "all" || vrow_of(r) == row)).collect()
        };
        let words: Vec<String> = fold(q).split_whitespace().map(str::to_string).collect();
        if !words.is_empty() {
            rs.retain(|r| {
                let hay = fold(&format!("{} {}", r.title, r.str("publisher")));
                words.iter().all(|w| hay.contains(w.as_str()))
            });
        }
        let d = |r: &Record| num(r, "duration");
        match length {
            "short" => rs.retain(|r| d(r) != 0.0 && d(r) < 600.0),
            "medium" => rs.retain(|r| (600.0..1800.0).contains(&d(r))),
            "long" => rs.retain(|r| d(r) >= 1800.0),
            _ => {}
        }
        match sort {
            "short" => rs.sort_by(|a, b| d(a).partial_cmp(&d(b)).unwrap_or(Ordering::Equal)),
            "new" => rs.sort_by(|a, b| b.str("upload_date").cmp(a.str("upload_date"))),
            "az" => rs.sort_by_key(|r| r.title.to_lowercase()),
            _ if row != "guides" => self.vsort(&mut rs),
            _ => {}
        }
        // the row the title names: `id` is its code
        let (id, title) = VROWS.iter().find(|v| v.0 == row).map(|v| (v.0, v.1)).unwrap_or(if row == GUIDES.0 { GUIDES } else { ALL });
        let note = vrow_note(row);
        let (page, more) = offset_page(&rs, offset, 48);
        json!({"id": id, "title": title, "note": note.map(|n| n.1), "note_code": note.map(|n| n.0), "total": rs.len(),
               "items": page.iter().map(|r| self.vcard(r)).collect::<Vec<_>>(), "more": more})
    }

    /// Videos like this one: from its row by other publishers, then by the same publisher, sharing title words first.
    pub fn videos_related(&self, vid: &str) -> Value {
        let limit = 12;
        let cat = self.catalog();
        let Some(v) = cat.get(vid) else { return json!([]) };
        let row = vrow_of(v);
        let pub_of = |r: &Record| r.get("publisher").clone();
        let words = |t: &str| -> HashSet<String> { LONG_WORD.find_iter(&fold(t)).map(|m| m.as_str().to_string()).collect() };
        let mine = words(&v.title);
        let shared = |r: &Record| words(&r.title).intersection(&mine).count();
        let videos = || cat.records.iter().filter(|r| r.kind == "video" && r.id != vid);
        let mut same_row: Vec<&Arc<Record>> = videos().filter(|r| vrow_of(r) == row && pub_of(r) != pub_of(v)).collect();
        let mut same_pub: Vec<&Arc<Record>> = videos().filter(|r| pub_of(r) == pub_of(v)).collect();
        self.vsort(&mut same_row);
        same_row.sort_by_key(|r| std::cmp::Reverse(shared(r)));
        same_pub.sort_by(|a, b| shared(b).cmp(&shared(a)).then(a.title.cmp(&b.title)));
        let out: Vec<Value> = same_row.iter().take(limit / 2).chain(same_pub.iter().take(limit)).take(limit).map(|r| self.vcard(r)).collect();
        Value::Array(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_has_words_in_the_interface() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/locales/en.json");
        let words: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        for id in SHELVES.iter().map(|s| s.0).chain([OTHER.0]) {
            assert!(words[format!("app.shelf.name.{id}")].is_string(), "web/locales/en.json has no app.shelf.name.{id}");
        }
        for id in VROWS.iter().map(|v| v.0).chain([GUIDES.0, ALL.0]) {
            assert!(words[format!("app.vrow.name.{id}")].is_string(), "web/locales/en.json has no app.vrow.name.{id}");
        }
        // every note is some row's, and every note has words
        let given: HashSet<&str> = VROWS.iter().filter_map(|v| vrow_note(v.0)).map(|n| n.0).chain([GUIDES_NOTE.0]).collect();
        assert_eq!(given, NOTES.iter().map(|n| n.0).collect());
        for (code, _) in NOTES {
            assert!(words[format!("app.vrow.note.{code}")].is_string(), "web/locales/en.json has no app.vrow.note.{code}");
        }
    }
}
