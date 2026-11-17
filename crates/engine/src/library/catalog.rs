//! The catalog: `library-data/catalog.json`, read again when it changes, without the videos the library has turned
//! down (`video-exclusions.json`). Each record keeps the fields the pack gave it; what the interface is shown of one is
//! [`Catalog::public`]. Ported from the original Lastbook's server.py (catalog, excluded_videos, trust, kind_label, year_of,
//! public, clean_title).

use super::words::truthy;
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, LazyLock};

/// Who a source is, as people are shown it, and how far search trusts it: (pattern on publisher, collection and
/// category; code; label; weight), the first that matches. The interface says the label by its code (web/locales:
/// app.trust.<code>); the English label stays beside it.
const TRUST: [(&str, &str, &str, f64); 7] = [
    (r"World Health|WHO\b|ICRC|IFRC|Red Cross|Médecins|MSF|UNICEF|Sphere|St John Ambulance|CAWST", "humanitarian", "International health & humanitarian guidance", 1.5),
    (r"Hesperian|Where There Is No|Where Women|Book for Midwives|Community Guide to Environmental|Health Actions|Workers' Guide", "hesperian", "Hesperian community health guide", 1.45),
    (r"FEMA|Ready\.gov|CDC|Centers for Disease|EPA|Environmental Protection|NOAA|National Weather|USDA|Food Safety|National Park|USGS|Department|Agency|U\.S\.|US Army|Army|Navy|Census|NIOSH|OSHA|NASA|FAA|Cal OES|California|County of|City of", "government", "Government guidance", 1.4),
    (r"National Center for Home Food Preservation|Extension|University|UC |College|Agricultural|Food Hero|UWyo", "university", "University / extension service", 1.35),
    (r"MIT|OpenStax|OpenCourseWare|NIST", "course", "Course or textbook", 1.1),
    (r"Kiwix|Wikipedia|Wikibooks|Appropedia|MDWiki", "encyclopedia", "Community-written encyclopedia", 1.0),
    (r"Coalcracker|Corporals Corner|Canterbury|Primitive Technology|creator-", "hobbyist", "Hobbyist demonstration video", 0.8),
];
/// A manual for one model (`equipment_class` or `model`), when no pattern matched; and everything else.
const MANUFACTURER: (&str, &str, f64) = ("manufacturer", "Manufacturer manual (model-specific)", 1.05);
const REFERENCE: (&str, &str, f64) = ("reference", "Reference", 1.0);
/// Every trust code a record can have.
pub const TRUST_CODES: [&str; 9] = ["humanitarian", "hesperian", "government", "university", "course", "encyclopedia", "hobbyist", "manufacturer", "reference"];

/// What kind of item, in words: (kind, label). The interface says them by the kind itself (web/locales: app.kind.<kind>).
pub const KINDS: [(&str, &str); 7] = [("pdf", "PDF"), ("html", "Saved web page"), ("video", "Video"), ("transcript", "Transcript"), ("pptx", "Slides"), ("zim", "Encyclopedia"), ("metadata", "Video details")];

static TRUST_RX: LazyLock<Vec<Regex>> = LazyLock::new(|| TRUST.iter().map(|(rx, _, _, _)| Regex::new(rx).unwrap()).collect());
static YEAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(19|20)\d\d").unwrap());
static TITLE_YEAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(19[5-9]\d|20[0-2]\d)\b").unwrap());
static CHAPTER_NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d{3}(\s\d+)?)\s+").unwrap());
static PDF_SUFFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.pdf$").unwrap());
static FILE_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\w\-]+$").unwrap());

/// One catalog record.
#[derive(Debug)]
pub struct Record {
    /// Every field as the catalog has it.
    pub raw: Map<String, Value>,
    pub id: String,
    pub kind: String,
    pub title: String,
    pub path: String,
    /// The label people are shown for the source, and its ranking weight.
    pub trust: (&'static str, f64),
    /// The label's code (one of [`TRUST_CODES`]).
    pub trust_code: &'static str,
    /// The languages the record is in (`lang`, a code or a list of codes; English when missing).
    pub langs: Vec<String>,
}

impl Record {
    pub fn new(raw: Map<String, Value>) -> Record {
        let s = |k: &str| raw.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let langs = match raw.get("lang") {
            Some(Value::String(l)) => vec![primary(l)],
            Some(Value::Array(ls)) => ls.iter().filter_map(Value::as_str).map(primary).collect(),
            _ => Vec::new(),
        };
        let langs = if langs.is_empty() { vec!["en".to_string()] } else { langs };
        let mut r = Record { id: s("id"), kind: s("kind"), title: s("title"), path: s("path"), trust: (REFERENCE.1, REFERENCE.2), trust_code: REFERENCE.0, langs, raw };
        let (code, label, weight) = trust_of(&r);
        (r.trust, r.trust_code) = ((label, weight), code);
        r
    }

    /// A field, or null.
    pub fn get(&self, k: &str) -> &Value {
        self.raw.get(k).unwrap_or(&Value::Null)
    }

    /// A text field, or "".
    pub fn str(&self, k: &str) -> &str {
        self.get(k).as_str().unwrap_or("")
    }

    /// A field as the original Lastbook wrote it: "" when missing, "None" when null.
    pub fn py_str(&self, k: &str) -> String {
        match self.raw.get(k) {
            None => String::new(),
            Some(v) => py_str(v),
        }
    }

    pub fn truthy(&self, k: &str) -> bool {
        truthy(self.get(k))
    }

    /// Whether the record is in this language (a primary code such as `it`).
    pub fn in_lang(&self, lang: &str) -> bool {
        self.langs.iter().any(|l| l == lang)
    }

    /// What kind of item, in words ([`KINDS`]); a kind it does not know, as it is.
    pub fn kind_label(&self) -> String {
        KINDS.iter().find(|(k, _)| *k == self.kind).map_or(self.kind.as_str(), |(_, label)| label).to_string()
    }

    /// The year it is from: its edition or dates, else a year in its title.
    pub fn year(&self) -> Option<String> {
        for k in ["edition", "publication_date", "upload_date"] {
            let v = self.get(k);
            let s = if truthy(v) { py_str(v) } else { String::new() };
            if let Some(m) = YEAR.find(&s) {
                return Some(m.as_str().to_string());
            }
        }
        TITLE_YEAR.find(&self.title).map(|m| m.as_str().to_string())
    }

    /// The title people are shown: no chapter file number, no `.pdf`, and a file name's underscores as spaces.
    pub fn clean_title(&self) -> String {
        let t = CHAPTER_NUMBER.replace(&self.title, "");
        let t = PDF_SUFFIX.replace(&t, "").into_owned();
        if FILE_NAME.is_match(&t) && t.contains('_') {
            t.replace('_', " ")
        } else {
            t
        }
    }

    pub fn image_only(&self) -> bool {
        self.str("searchability").starts_with("image-only")
    }
}

fn primary(lang: &str) -> String {
    lang.split(['-', '_']).next().unwrap_or("").to_ascii_lowercase()
}

/// A value as the original Lastbook wrote it.
pub fn py_str(v: &Value) -> String {
    match v {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

/// (code, label, weight) of the source.
fn trust_of(r: &Record) -> (&'static str, &'static str, f64) {
    let s = format!("{} {} {}", r.py_str("publisher"), r.py_str("collection"), r.py_str("category"));
    for (rx, (_, code, label, w)) in TRUST_RX.iter().zip(TRUST.iter()) {
        if rx.is_match(&s) {
            return (code, label, *w);
        }
    }
    if r.truthy("equipment_class") || r.truthy("model") {
        return MANUFACTURER;
    }
    REFERENCE
}

/// A path from the catalog, inside the library folder, or None if it would lead out of it.
pub fn inside(lib: &Path, rel: &str) -> Option<PathBuf> {
    if rel.is_empty() || rel.contains('\0') || rel.contains('\\') {
        return None;
    }
    let p = Path::new(rel);
    if p.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
        return None;
    }
    Some(lib.join(p))
}

/// The catalog as read, with the lookups the engine uses.
#[derive(Default)]
pub struct Catalog {
    /// One record per id, in the order the ids first appear; the last record with an id wins, as in the original Lastbook's catalog.
    pub records: Vec<Arc<Record>>,
    by_id: HashMap<String, Arc<Record>>,
    by_path: HashMap<String, Arc<Record>>,
    /// A video's transcript record, by the video's path.
    video_tx: HashMap<String, Arc<Record>>,
    /// A video's thumbnail, by the video's path.
    thumbs: HashMap<String, String>,
}

impl Catalog {
    /// From catalog.json's records, without those turned down (ids, and their `metadata-` records).
    pub fn new(list: Vec<Value>, excluded: &HashSet<String>) -> Catalog {
        let mut recs: Vec<Record> = list.into_iter().filter_map(|v| if let Value::Object(m) = v { Some(Record::new(m)) } else { None }).filter(|r| !r.id.is_empty()).collect();
        if !excluded.is_empty() {
            let bad_paths: HashSet<String> = recs.iter().filter(|r| excluded.contains(&r.id)).map(|r| r.path.clone()).collect();
            recs.retain(|r| {
                !(excluded.contains(&r.id) || excluded.contains(r.str("video_id")) || (r.truthy("video_path") && bad_paths.contains(r.str("video_path"))))
            });
        }
        let mut c = Catalog::default();
        let mut order: Vec<String> = Vec::new();
        for r in recs {
            let r = Arc::new(r);
            if c.by_id.insert(r.id.clone(), r.clone()).is_none() {
                order.push(r.id.clone());
            }
            c.by_path.insert(r.path.clone(), r.clone());
            if r.kind == "transcript" && r.truthy("video_path") {
                c.video_tx.insert(r.str("video_path").to_string(), r.clone());
            }
            if r.kind == "metadata" && r.truthy("video_path") && r.truthy("thumbnail_path") {
                c.thumbs.insert(r.str("video_path").to_string(), r.str("thumbnail_path").to_string());
            }
        }
        c.records = order.iter().map(|id| c.by_id[id].clone()).collect();
        c
    }

    pub fn get(&self, id: &str) -> Option<&Arc<Record>> {
        self.by_id.get(id)
    }

    pub fn by_path(&self, path: &str) -> Option<&Arc<Record>> {
        self.by_path.get(path)
    }

    /// The transcript record of the video at this path.
    pub fn transcript_of(&self, video_path: &str) -> Option<&Arc<Record>> {
        self.video_tx.get(video_path)
    }

    pub fn thumb_of(&self, video_path: &str) -> Option<&str> {
        self.thumbs.get(video_path).map(String::as_str)
    }

    /// The fields the interface needs, with friendly labels (server.py public): `trust` with its code beside it, `trust_code`,
    /// and `kindLabel` beside `kind`, which the interface says them by. Never local sign-in or worker state.
    pub fn public(&self, r: &Record, full: bool, lib: &Path) -> Value {
        let pages = if r.truthy("pages") { r.get("pages").clone() } else { r.get("slides").clone() };
        let mut out = json!({
            "id": r.id, "title": r.clean_title(), "kind": r.kind, "kindLabel": r.kind_label(), "publisher": r.get("publisher"),
            "collection": r.get("collection"), "path": r.path, "pages": pages, "duration": r.get("duration"), "year": r.year(),
            "trust": r.trust.0, "trust_code": r.trust_code, "bytes": r.get("bytes"),
        });
        let o = out.as_object_mut().unwrap();
        if r.kind == "video" {
            o.insert("transcript".into(), json!(self.video_tx.contains_key(&r.path) || r.truthy("captions")));
            let thumb = self.thumbs.get(&r.path).filter(|t| !t.is_empty() && inside(lib, t).is_some_and(|p| p.is_file()));
            o.insert("thumb".into(), json!(thumb));
        }
        if r.image_only() {
            o.insert("imageOnly".into(), json!(true));
        }
        if full {
            for k in ["notes", "edition", "retrieved_at", "source_url", "license", "license_url", "searchability", "upload_date", "publication_date", "model", "course", "level", "caption_type"] {
                if r.truthy(k) {
                    o.insert(k.into(), r.get(k).clone());
                }
            }
            if r.kind == "transcript" && r.truthy("video_path") {
                if let Some(v) = self.by_path.get(r.str("video_path")) {
                    o.insert("videoId".into(), json!(v.id));
                }
            }
            if r.kind == "video" {
                if let Some(tx) = self.video_tx.get(&r.path) {
                    o.insert("transcriptId".into(), json!(tx.id));
                    o.insert("caption_type".into(), tx.get("caption_type").clone());
                }
            }
        }
        out
    }
}

/// The ids in `video-exclusions.json` (its `videos` keys), with their `metadata-` records.
pub fn exclusions(v: &Value) -> HashSet<String> {
    let ids: Vec<String> = v["videos"].as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
    ids.iter().flat_map(|i| [i.clone(), format!("metadata-{i}")]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(v: Value) -> Record {
        Record::new(v.as_object().unwrap().clone())
    }

    #[test]
    fn trust_labels_and_weights() {
        let who = rec(json!({"publisher": "World Health Organization"}));
        assert_eq!((who.trust, who.trust_code), (("International health & humanitarian guidance", 1.5), "humanitarian"));
        assert_eq!(rec(json!({"publisher": "Hesperian Health Guides"})).trust.1, 1.45);
        assert_eq!(rec(json!({"collection": "FEMA library"})).trust.0, "Government guidance");
        assert_eq!(rec(json!({"category": "creator-coalcracker-bushcraft"})).trust.1, 0.8);
        assert_eq!(rec(json!({"model": "X100"})).trust.0, "Manufacturer manual (model-specific)");
        assert_eq!(rec(json!({"publisher": null, "collection": "Odds"})).trust, ("Reference", 1.0));
        assert_eq!(rec(json!({"publisher": "WHOLESALE"})).trust.0, "Reference"); // WHO\b
        // each label has its code, the English beside it
        for (fields, code, label) in [
            (json!({"publisher": "Hesperian Health Guides"}), "hesperian", "Hesperian community health guide"),
            (json!({"collection": "FEMA library"}), "government", "Government guidance"),
            (json!({"publisher": "Oregon State University Extension"}), "university", "University / extension service"),
            (json!({"publisher": "OpenStax"}), "course", "Course or textbook"),
            (json!({"collection": "Wikibooks"}), "encyclopedia", "Community-written encyclopedia"),
            (json!({"category": "creator-coalcracker-bushcraft"}), "hobbyist", "Hobbyist demonstration video"),
            (json!({"model": "X100"}), "manufacturer", "Manufacturer manual (model-specific)"),
            (json!({"publisher": "WHOLESALE"}), "reference", "Reference"),
        ] {
            let r = rec(fields);
            assert_eq!((r.trust_code, r.trust.0), (code, label));
            assert!(TRUST_CODES.contains(&r.trust_code));
        }
    }

    #[test]
    fn titles_years_and_labels() {
        assert_eq!(rec(json!({"title": "012 3 Bleeding.pdf"})).clean_title(), "Bleeding");
        assert_eq!(rec(json!({"title": "first_aid_guide"})).clean_title(), "first aid guide");
        assert_eq!(rec(json!({"title": "first_aid guide"})).clean_title(), "first_aid guide");
        assert_eq!(rec(json!({"title": "Water", "edition": "3rd edition, 2017"})).year().as_deref(), Some("2017"));
        assert_eq!(rec(json!({"title": "Water", "edition": 1998})).year().as_deref(), Some("1998"));
        assert_eq!(rec(json!({"title": "Field manual 1985", "edition": ""})).year().as_deref(), Some("1985"));
        assert_eq!(rec(json!({"title": "Room 2100"})).year(), None);
        assert_eq!(rec(json!({"kind": "zim"})).kind_label(), "Encyclopedia");
        assert_eq!(rec(json!({"kind": "odd"})).kind_label(), "odd");
        assert_eq!(rec(json!({})).langs, vec!["en"]);
        assert!(rec(json!({"lang": "it-IT"})).in_lang("it"));
        assert!(rec(json!({"lang": ["de", "fr"]})).in_lang("fr"));
    }

    #[test]
    fn catalog_lookups_and_exclusions() {
        let list = vec![
            json!({"id": "v1", "kind": "video", "path": "videos/v1.mp4", "title": "One", "captions": []}),
            json!({"id": "t1", "kind": "transcript", "path": "transcripts/v1.txt", "video_path": "videos/v1.mp4", "title": "One", "caption_type": "auto"}),
            json!({"id": "metadata-v1", "kind": "metadata", "path": "metadata/videos/v1.json", "video_path": "videos/v1.mp4", "thumbnail_path": "metadata/videos/v1.jpg", "title": "x"}),
            json!({"id": "v2", "kind": "video", "path": "videos/v2.mp4", "title": "Two"}),
            json!({"id": "t2", "kind": "transcript", "path": "transcripts/v2.txt", "video_path": "videos/v2.mp4", "title": "Two"}),
            json!({"id": "v1", "kind": "video", "path": "videos/v1.mp4", "title": "One again"}),
            json!("not a record"),
        ];
        let c = Catalog::new(list.clone(), &HashSet::new());
        assert_eq!(c.records.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["v1", "t1", "metadata-v1", "v2", "t2"]);
        assert_eq!(c.get("v1").unwrap().title, "One again");
        assert_eq!(c.transcript_of("videos/v1.mp4").unwrap().id, "t1");
        assert_eq!(c.thumb_of("videos/v1.mp4"), Some("metadata/videos/v1.jpg"));
        let lib = std::env::temp_dir();
        let p = c.public(c.get("v1").unwrap(), true, &lib);
        assert_eq!(p["transcript"], json!(true));
        assert_eq!(p["thumb"], Value::Null); // no such file
        assert_eq!(p["transcriptId"], "t1");
        assert_eq!(p["caption_type"], "auto");
        // the interface says the trust by its code and the kind by the kind itself; the English stays beside them
        assert_eq!((&p["trust_code"], &p["trust"], &p["kind"], &p["kindLabel"]), (&json!("reference"), &json!("Reference"), &json!("video"), &json!("Video")));
        assert_eq!(c.public(c.get("t2").unwrap(), true, &lib)["videoId"], "v2");
        // a video turned down goes, with its details and its transcript
        let ex = exclusions(&json!({"videos": {"v2": {"why": "x"}}}));
        assert!(ex.contains("metadata-v2"));
        let c = Catalog::new(list, &ex);
        assert!(c.get("v2").is_none() && c.get("t2").is_none());
        assert!(c.get("v1").is_some());
    }

    #[test]
    fn catalog_paths_stay_inside() {
        let lib = Path::new("/lib");
        assert_eq!(inside(lib, "text/a.txt"), Some(PathBuf::from("/lib/text/a.txt")));
        for bad in ["", "/etc/passwd", "../x", "text/../../x", "a\\b", "a\0"] {
            assert_eq!(inside(lib, bad), None, "{bad:?}");
        }
    }

    #[test]
    fn every_code_has_words_in_the_interface() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/locales/en.json");
        let words: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        // the codes listed are the codes a record can get
        let given: Vec<&str> = TRUST.iter().map(|t| t.1).chain([MANUFACTURER.0, REFERENCE.0]).collect();
        assert_eq!(given, TRUST_CODES);
        for code in TRUST_CODES {
            assert!(words[format!("app.trust.{code}")].is_string(), "web/locales/en.json has no app.trust.{code}");
        }
        for (kind, _) in KINDS {
            assert!(words[format!("app.kind.{kind}")].is_string(), "web/locales/en.json has no app.kind.{kind}");
        }
    }
}
