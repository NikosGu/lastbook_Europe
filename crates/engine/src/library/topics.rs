//! The guides with what each item opens filled in from the catalog, and items that open nothing in this library left
//! out. A guide is `content/topics/<id>.json` in English; `content/topics/<lang>/<id>.json` is the same guide in another
//! language, with the same items, and the interface's language picks it where it is there. Ported from the original
//! Lastbook's server.py (topics, zim_names, item_here, resolve_item, topics_resolved).

use super::catalog::Catalog;
use super::words::truthy;
use super::Library;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

impl Library {
    /// Every guide in English, as its file has it, in file-name order. A file that does not parse is `{id, error}`.
    pub fn topics(&self) -> Vec<Value> {
        self.topics_in("en")
    }

    /// Every guide in `lang` where its file in that language is there and reads, else in English.
    pub fn topics_in(&self, lang: &str) -> Vec<Value> {
        let read = |f: &Path| std::fs::read(f).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice::<Value>(&b).map_err(|e| e.to_string()));
        guide_files(&self.paths.content().join("topics"), lang)
            .into_iter()
            .map(|(en, own)| match own.and_then(|f| read(&f).ok()).map_or_else(|| read(&en), Ok) {
                Ok(t) => t,
                Err(e) => json!({"id": en.file_stem().and_then(|s| s.to_str()).unwrap_or(""), "error": e}),
            })
            .collect()
    }

    /// The encyclopedia files this library really holds. Its folder decides, so one rule covers every size of pack.
    pub fn zim_names(&self) -> Vec<String> {
        let Some(lib) = self.lib() else { return Vec::new() };
        zim_names(&lib)
    }

    /// The guides in English with each item's source filled in.
    pub fn topics_resolved(&self) -> Vec<Value> {
        self.topics_resolved_in("en")
    }

    /// The guides in `lang` with each item's source filled in, as server.py topics_resolved: an item that opens nothing
    /// here is left out, and a step left with nothing to open goes. A warning whose document is not in this library is
    /// left out too, as a reading is (a pack may leave out the documents that may not be passed on); only with no
    /// library in reach (not set up yet, or on a drive that is away) does it keep its words without its source, as the
    /// original did. A guide that reads from documents of which this library holds none says so, in place of its short
    /// line and intro, in the words of `content/uncovered.json`. What the library lacks comes from `content/gaps.json`,
    /// unless a guide in another language carries it in its own words.
    pub fn topics_resolved_in(&self, lang: &str) -> Vec<Value> {
        let cat = self.catalog();
        let lib = self.lib().unwrap_or_default();
        let present = !cat.records.is_empty();
        let gaps: Value = std::fs::read(self.paths.content().join("gaps.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(json!({}));
        let none_here = if present { self.uncovered_words(lang) } else { None };
        let zims = self.zim_names();
        let here = |items: &Value| -> Vec<Value> {
            items.as_array().into_iter().flatten().filter(|i| item_here(i, &zims, &cat)).map(|i| resolve_item(i, &cat, &lib)).collect()
        };
        let mut res = Vec::new();
        for t in self.topics_in(lang) {
            let Value::Object(mut t) = t else { continue };
            let id = t.get("id").and_then(Value::as_str).unwrap_or("").to_string();
            let reads_from = openers(&t);
            if !t.get("gaps").is_some_and(Value::is_object) {
                t.insert("gaps".into(), gaps.get(&id).cloned().unwrap_or(json!({})));
            }
            for k in ["prepare", "learn"] {
                let v = here(t.get(k).unwrap_or(&Value::Null));
                t.insert(k.into(), Value::Array(v));
            }
            let limits: Vec<Value> = t
                .get("limits")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|i| match item_here(i, &zims, &cat) {
                    true => Some(resolve_item(i, &cat, &lib)),
                    false if !present => Some(json!({"text": i.get("text")})),
                    false => None,
                })
                .collect();
            t.insert("limits".into(), Value::Array(limits));
            for k in ["choices", "route"] {
                let steps: Vec<Value> = t
                    .get(k)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|c| {
                        let mut c = c.as_object()?.clone();
                        let had = truthy(c.get("read").unwrap_or(&Value::Null));
                        let read = here(c.get("read").unwrap_or(&Value::Null));
                        let useful = truthy(c.get("goto").unwrap_or(&Value::Null)) || !read.is_empty() || !had;
                        c.insert("read".into(), Value::Array(read));
                        useful.then_some(Value::Object(c))
                    })
                    .collect();
                t.insert(k.into(), Value::Array(steps));
            }
            if let Some((short, intro)) = none_here.as_ref().filter(|_| reads_from > 0 && openers(&t) == 0) {
                t.insert("short".into(), json!(short));
                t.insert("intro".into(), json!(intro));
                t.insert("uncovered".into(), json!(true));
            }
            res.push(Value::Object(t));
        }
        res
    }

    /// What a guide says when this library holds none of the documents it reads from: the short line and intro of
    /// `content/uncovered.json` in `lang`, else in English. None when the file is not there.
    fn uncovered_words(&self, lang: &str) -> Option<(String, String)> {
        let v: Value = serde_json::from_slice(&std::fs::read(self.paths.content().join("uncovered.json")).ok()?).ok()?;
        let words = |l: &str| Some((v[l]["short"].as_str()?.to_string(), v[l]["intro"].as_str()?.to_string()));
        words(lang).or_else(|| words("en"))
    }

    /// Every item a guide names, in the order server.py's guide_videos and prewarm_previews take them.
    pub fn guide_items(t: &Value, order: GuideOrder) -> Vec<Value> {
        let reads = |k: &str| -> Vec<Value> { t[k].as_array().into_iter().flatten().flat_map(|c| c["read"].as_array().cloned().unwrap_or_default()).collect() };
        let list = |k: &str| -> Vec<Value> { t[k].as_array().cloned().unwrap_or_default() };
        match order {
            GuideOrder::Videos => [reads("choices"), list("learn"), list("prepare"), reads("route")].concat(),
            GuideOrder::Previews => [reads("choices"), reads("route"), list("prepare"), list("learn")].concat(),
        }
    }
}

/// The two orders guide items are walked in.
#[derive(Clone, Copy)]
pub enum GuideOrder {
    Videos,
    Previews,
}

/// Each guide's English file, in file-name order (the English files decide which guides there are), with the same
/// guide's file in `lang` when there is one. A language is two small letters, so `lang` never leaves `dir`.
pub fn guide_files(dir: &Path, lang: &str) -> Vec<(PathBuf, Option<PathBuf>)> {
    let own = (lang != "en" && lang.len() == 2 && lang.bytes().all(|b| b.is_ascii_lowercase())).then(|| dir.join(lang));
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "json")).collect();
    files.sort();
    files
        .into_iter()
        .map(|en| {
            let own = own.as_ref().and_then(|d| en.file_name().map(|n| d.join(n))).filter(|p| p.is_file());
            (en, own)
        })
        .collect()
}

/// The `.zim` files in `collections/`, sorted.
pub fn zim_names(lib: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(lib.join("collections")).into_iter().flatten().flatten().filter_map(|e| e.file_name().into_string().ok()).filter(|n| n.ends_with(".zim")).collect();
    v.sort();
    v
}

/// How many of a guide's items open a document or an encyclopedia article: its readings, warnings, preparing and
/// learning items.
fn openers(t: &Map<String, Value>) -> usize {
    let opens = |i: &&Value| truthy(i.get("doc").unwrap_or(&Value::Null)) || truthy(i.get("zim").unwrap_or(&Value::Null));
    let list = |k: &str| t.get(k).and_then(Value::as_array).map_or(0, |a| a.iter().filter(opens).count());
    let reads = |k: &str| {
        let steps = t.get(k).and_then(Value::as_array).map(|a| a.as_slice()).unwrap_or_default();
        steps.iter().map(|c| c.get("read").and_then(Value::as_array).map_or(0, |r| r.iter().filter(opens).count())).sum::<usize>()
    };
    list("prepare") + list("learn") + list("limits") + reads("choices") + reads("route")
}

/// Is what this guide item opens in this library? An item that opens nothing is hidden, not sent Home with no message.
pub fn item_here(i: &Value, zims: &[String], cat: &Catalog) -> bool {
    if let Some(z) = i.get("zim").filter(|z| truthy(z)) {
        let parts: Vec<&str> = z.as_str().unwrap_or("").split('/').collect();
        return parts.len() > 1 && zims.iter().any(|n| n == parts[1]);
    }
    match i.get("doc").filter(|d| truthy(d)) {
        None => true,
        Some(d) => d.as_str().is_some_and(|d| cat.get(d).is_some()),
    }
}

/// A guide item with its source's public record (and the source's notes) as `src`.
pub fn resolve_item(i: &Value, cat: &Catalog, lib: &Path) -> Value {
    let mut i: Map<String, Value> = i.as_object().cloned().unwrap_or_default();
    if let Some(d) = i.get("doc").filter(|d| truthy(d)).cloned() {
        let src = match d.as_str().and_then(|d| cat.get(d)) {
            Some(r) => {
                let mut p = cat.public(r, false, lib);
                p["notes"] = r.get("notes").clone();
                p
            }
            None => Value::Null,
        };
        i.insert("src".into(), src);
    }
    Value::Object(i)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{body_json, get, library, repo};
    use super::*;
    use std::collections::HashMap;

    const REVIEW: &str = "machine-assisted, needs native-speaker review";

    fn read(p: &Path) -> Value {
        serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
    }

    /// The documents the packs list (`content/library/*.json`), each with the one whose pages it repeats in another
    /// language (`same_pages_as`), if any.
    fn pack_documents() -> HashMap<String, PackDoc> {
        let mut docs = HashMap::from([("lastbook-sources".to_string(), PackDoc { lang: "en".into(), same_pages_as: None })]);
        for f in std::fs::read_dir(repo().join("content/library")).unwrap().flatten() {
            for d in read(&f.path())["documents"].as_array().unwrap() {
                let doc = PackDoc { lang: d["lang"].as_str().unwrap_or("").to_string(), same_pages_as: d["same_pages_as"].as_str().map(str::to_string) };
                docs.insert(d["id"].as_str().unwrap().to_string(), doc);
            }
        }
        docs
    }

    /// A document a pack lists: its language, and the document whose pages it repeats in another language, if any.
    struct PackDoc {
        lang: String,
        same_pages_as: Option<String>,
    }

    /// Every item a guide opens, with where it sits: the choices' and steps' readings, warnings, preparing, learning.
    fn items(t: &Value) -> Vec<(String, &Value)> {
        let mut v = Vec::new();
        for k in ["choices", "route"] {
            for (n, c) in t[k].as_array().into_iter().flatten().enumerate() {
                v.extend(c["read"].as_array().into_iter().flatten().map(|i| (format!("{k}[{n}]"), i)));
            }
        }
        for k in ["limits", "prepare", "learn"] {
            v.extend(t[k].as_array().into_iter().flatten().map(|i| (k.to_string(), i)));
        }
        v
    }

    /// What a guide is made of, without its words or what its items open: ids, links, and where each item sits, with
    /// whether it opens one page or a whole document.
    fn frame(t: &Value) -> Vec<String> {
        let mut v = vec![format!("{} {} {}", t["id"], t["mode"], t["related"])];
        v.extend(t["choices"].as_array().into_iter().flatten().map(|c| format!("choice {} {}", c["id"], c["goto"])));
        v.push(format!("{} steps", t["route"].as_array().map_or(0, Vec::len)));
        v.extend(items(t).into_iter().map(|(at, i)| format!("{at} {}", if i.get("page").is_some() { "page" } else { "document" })));
        v
    }

    /// What each item of a guide opens: the document (an edition in another language counts as the document whose
    /// pages it repeats) and the page.
    fn opens(t: &Value, docs: &HashMap<String, PackDoc>) -> Vec<(String, Value)> {
        let doc = |i: &Value| {
            let d = i["doc"].as_str().unwrap_or("");
            docs.get(d).and_then(|p| p.same_pages_as.clone()).unwrap_or_else(|| d.to_string())
        };
        items(t).into_iter().map(|(_, i)| (doc(i), i["page"].clone())).collect()
    }

    /// The words a person reads in a guide, with where they are.
    fn texts(t: &Value) -> Vec<(String, String)> {
        let s = |v: &Value| v.as_str().unwrap_or("").to_string();
        let mut v: Vec<(String, String)> = ["title", "short", "intro"].iter().map(|k| (k.to_string(), s(&t[*k]))).collect();
        for (n, c) in t["choices"].as_array().into_iter().flatten().enumerate() {
            v.push((format!("choices[{n}].label"), s(&c["label"])));
        }
        for (n, c) in t["route"].as_array().into_iter().flatten().enumerate() {
            v.push((format!("route[{n}].title"), s(&c["title"])));
        }
        for (at, i) in items(t) {
            let k = if at == "limits" { "text" } else { "label" };
            v.push((format!("{at}.{k}"), s(&i[k])));
        }
        v
    }

    #[test]
    fn the_interface_language_picks_each_guide_file() {
        let dir = repo().join("content/topics");
        let en = guide_files(&dir, "en");
        assert!(en.len() >= 10 && en.iter().all(|(_, own)| own.is_none()));
        let it = guide_files(&dir, "it");
        assert_eq!(it.iter().map(|f| &f.0).collect::<Vec<_>>(), en.iter().map(|f| &f.0).collect::<Vec<_>>(), "the English files decide which guides there are");
        assert!(it.iter().any(|(_, own)| own.is_some()), "content/topics/it/ has guides");
        for bad in ["", "xx", "../topics", "it/..", "IT", "ita", ".."] {
            assert!(guide_files(&dir, bad).iter().all(|(_, own)| own.is_none()), "{bad}");
        }
        // through the route the page asks: each guide in Italian where it is there, the same guides as in English
        let l = library();
        let (en, it) = (body_json(&get(&l, "/api/topics")), body_json(&get(&l, "/api/topics?lang=it")));
        let ids = |v: &Value| v.as_array().unwrap().iter().map(|t| t["id"].clone()).collect::<Vec<_>>();
        assert_eq!(ids(&it), ids(&en));
        let water = |v: &Value| v.as_array().unwrap().iter().find(|t| t["id"] == "water").cloned().unwrap();
        assert_eq!(water(&it)["title"], read(&dir.join("it/water.json"))["title"]);
        assert_ne!(water(&it)["title"], water(&en)["title"]);
        assert_eq!(body_json(&get(&l, "/api/topics?lang=../..")), en, "a language that is not one stays English");
    }

    #[test]
    /// A translation has the English guide's items, in the same order; each opens the English item's page, or a page of
    /// a document in the translation's own language (which must say the same thing: content/SCHEMA.md).
    fn a_guide_in_another_language_has_the_items_of_the_english_one() {
        let dir = repo().join("content/topics");
        let gaps = read(&repo().join("content/gaps.json"));
        let docs = pack_documents();
        let mut seen = 0;
        for lang in ["it", "de", "fr", "es", "pl"] {
            for (en, own) in guide_files(&dir, lang) {
                let Some(own) = own else { continue };
                seen += 1;
                let (e, t, name) = (read(&en), read(&own), own.display().to_string());
                let text = std::fs::read_to_string(&own).unwrap();
                assert!(text.trim_start().trim_start_matches('{').trim_start().starts_with("\"_review\""), "{name}: starts with \"_review\", as web/locales do");
                assert_eq!(t["_review"], REVIEW, "{name}");
                assert_eq!(frame(&t), frame(&e), "{name}: the same choices and the same items as the English guide, in the same order");
                for (k, (o, eo)) in opens(&t, &docs).into_iter().zip(opens(&e, &docs)).enumerate() {
                    let own = docs.get(&o.0).is_some_and(|d| d.lang == lang);
                    assert!(o == eo || own, "{name}: item {k} opens {o:?}; it opens the English item's page, or a page in {lang}");
                }
                for ((at, tw), (_, ew)) in texts(&t).iter().zip(texts(&e)) {
                    assert!(ew.is_empty() || !tw.trim().is_empty(), "{name}: {at} has no words");
                }
                let id = e["id"].as_str().unwrap();
                for k in ["missing", "disagree"] {
                    let n = |v: &Value| v[k].as_array().map_or(0, Vec::len);
                    assert_eq!(n(&t["gaps"]), n(&gaps[id]), "{name}: gaps.{k} says in its words what content/gaps.json says");
                }
            }
        }
        assert!(seen >= 14, "the Italian guides at least are all there");
    }

    #[test]
    fn every_guide_opens_a_page_of_a_document_a_pack_lists() {
        let docs = pack_documents();
        for (d, p) in &docs {
            assert!(p.same_pages_as.as_ref().is_none_or(|s| docs.contains_key(s)), "{d}: same_pages_as names a document no pack lists");
        }
        let gaps = read(&repo().join("content/gaps.json"));
        for (en, _) in guide_files(&repo().join("content/topics"), "en") {
            let t = read(&en);
            let id = t["id"].as_str().unwrap_or("");
            assert_eq!(en.file_stem().and_then(|s| s.to_str()), Some(id));
            for (at, i) in items(&t) {
                let doc = i["doc"].as_str();
                assert!(i.get("zim").is_none(), "{id} {at}: no encyclopedia in any pack yet");
                assert!(doc.is_some_and(|d| docs.contains_key(d)), "{id} {at}: {doc:?} is in no sources list in content/library");
                assert!(i.get("page").is_none() || i["page"].as_u64().is_some_and(|p| p >= 1), "{id} {at}: a page counts from 1");
                if at == "limits" {
                    assert!(i["text"].as_str().is_some_and(|s| !s.trim().is_empty()), "{id}: a warning says something");
                }
            }
            // a guide with nothing to read says why: what the library lacks is in gaps.json
            if items(&t).is_empty() {
                assert!(gaps[id]["missing"].as_array().is_some_and(|m| !m.is_empty()), "{id}: no readings, and gaps.json does not say what is missing");
            }
        }
    }

    /// A library whose catalog holds these documents (and no file), in a folder of its own.
    fn holding(held: &[&str]) -> (PathBuf, Library) {
        let data = super::super::tests::temp("held");
        std::fs::create_dir_all(data.join("library-data")).unwrap();
        let recs: Vec<Value> =
            held.iter().map(|id| json!({"id": id, "kind": "pdf", "path": format!("documents/{id}.pdf"), "title": id, "collection": "Test", "category": "test", "publisher": "Test", "pages": 500})).collect();
        std::fs::write(data.join("library-data/catalog.json"), serde_json::to_vec(&recs).unwrap()).unwrap();
        let l = Library::new(crate::paths::Paths::new(repo(), Some(data.clone())));
        (data, l)
    }

    #[test]
    fn a_library_without_some_documents_shows_only_what_it_holds() {
        // the starter documents that may be passed on, as the published pack holds them
        let starter = read(&repo().join("content/library/starter.json"));
        let open: Vec<&str> = starter["documents"].as_array().unwrap().iter().filter(|d| d["redistribute"] == "yes").filter_map(|d| d["id"].as_str()).collect();
        assert!(!open.is_empty() && open.len() < starter["documents"].as_array().unwrap().len());
        let (data, l) = holding(&open);
        guides_show_only(&l, &open);
        // with no library in reach (a drive that is away), a warning keeps its words and no guide is said to be empty
        let away = Library::new(crate::paths::Paths::new(repo(), Some(data.join("away"))));
        for t in away.topics_resolved_in("it") {
            let file = l.topics_in("it").into_iter().find(|f| f["id"] == t["id"]).unwrap();
            assert_eq!(t["limits"].as_array().map_or(0, Vec::len), file["limits"].as_array().map_or(0, Vec::len), "{}", t["id"]);
            assert!(t.get("uncovered").is_none() && t["intro"] == file["intro"], "{}", t["id"]);
        }
        let _ = std::fs::remove_dir_all(&data);
        // one document only: the guides that read from none of it say so
        let (data, l) = holding(&["ravenna-rischio-blackout"]);
        assert!(guides_show_only(&l, &["ravenna-rischio-blackout"]) > 0, "some guide reads only from documents this library leaves out");
        let _ = std::fs::remove_dir_all(&data);
    }

    /// Checks that each guide, in each language, opens only the documents held, shows no warning whose document is not
    /// held, and says so where none of its documents is held; how many guides said so.
    fn guides_show_only(l: &Library, held: &[&str]) -> usize {
        let words = read(&repo().join("content/uncovered.json"));
        let mut said_empty = 0;
        for lang in ["en", "it", "de", "fr", "es", "pl"] {
            let files: HashMap<String, Value> = l.topics_in(lang).into_iter().map(|t| (t["id"].as_str().unwrap_or("").to_string(), t)).collect();
            let resolved = l.topics_resolved_in(lang);
            assert_eq!(resolved.len(), files.len(), "{lang}: every guide is there, emptied or not");
            for t in &resolved {
                let id = t["id"].as_str().unwrap();
                let file = &files[id];
                // every reading, warning, preparing and learning item left opens a document this library holds
                for (at, i) in items(t) {
                    let d = i["doc"].as_str().unwrap_or("");
                    assert!(held.contains(&d) && !i["src"].is_null(), "{lang} {id} {at}: {d} is not in this library");
                }
                // a warning whose document is not here is not shown, words and all
                let warnings_here = file["limits"].as_array().into_iter().flatten().filter(|i| held.contains(&i["doc"].as_str().unwrap_or(""))).count();
                assert_eq!(t["limits"].as_array().map_or(0, Vec::len), warnings_here, "{lang} {id}");
                // a guide that reads from documents this library has none of says so; the others keep their words
                if !items(file).is_empty() && items(t).is_empty() {
                    let w = if words[lang].is_object() { &words[lang] } else { &words["en"] };
                    assert_eq!((&t["short"], &t["intro"], &t["uncovered"]), (&w["short"], &w["intro"], &json!(true)), "{lang} {id}");
                    said_empty += 1;
                } else {
                    assert_eq!((&t["short"], &t["intro"], t.get("uncovered")), (&file["short"], &file["intro"], None), "{lang} {id}");
                }
            }
        }
        said_empty
    }
}
