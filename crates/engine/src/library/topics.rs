//! The guides (`content/topics/*.json`) with what each item opens filled in from the catalog, and items that open
//! nothing in this library left out. Ported from the original Lastbook's server.py (topics, zim_names, item_here,
//! resolve_item, topics_resolved).

use super::catalog::Catalog;
use super::words::truthy;
use super::Library;
use serde_json::{json, Map, Value};
use std::path::Path;

impl Library {
    /// Every guide as its file has it, in file-name order. A file that does not parse is `{id, error}`.
    pub fn topics(&self) -> Vec<Value> {
        let dir = self.paths.content().join("topics");
        let mut files: Vec<_> = std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
        files.sort();
        files
            .iter()
            .map(|f| match std::fs::read(f).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice::<Value>(&b).map_err(|e| e.to_string())) {
                Ok(t) => t,
                Err(e) => json!({"id": f.file_stem().and_then(|s| s.to_str()).unwrap_or(""), "error": e}),
            })
            .collect()
    }

    /// The encyclopedia files this library really holds. Its folder decides, so one rule covers every size of pack.
    pub fn zim_names(&self) -> Vec<String> {
        let Some(lib) = self.lib() else { return Vec::new() };
        zim_names(&lib)
    }

    /// The guides with each item's source filled in, as server.py topics_resolved: an item that opens nothing here is
    /// left out, a warning keeps its words without its source, and a step left with nothing to open goes.
    pub fn topics_resolved(&self) -> Vec<Value> {
        let cat = self.catalog();
        let lib = self.lib().unwrap_or_default();
        let gaps: Value = std::fs::read(self.paths.content().join("gaps.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(json!({}));
        let zims = self.zim_names();
        let here = |items: &Value| -> Vec<Value> {
            items.as_array().into_iter().flatten().filter(|i| item_here(i, &zims, &cat)).map(|i| resolve_item(i, &cat, &lib)).collect()
        };
        let mut res = Vec::new();
        for t in self.topics() {
            let Value::Object(mut t) = t else { continue };
            let id = t.get("id").and_then(Value::as_str).unwrap_or("").to_string();
            t.insert("gaps".into(), gaps.get(&id).cloned().unwrap_or(json!({})));
            for k in ["prepare", "learn"] {
                let v = here(t.get(k).unwrap_or(&Value::Null));
                t.insert(k.into(), Value::Array(v));
            }
            let limits: Vec<Value> = t
                .get("limits")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|i| if item_here(i, &zims, &cat) { resolve_item(i, &cat, &lib) } else { json!({"text": i.get("text")}) })
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
            res.push(Value::Object(t));
        }
        res
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

/// The `.zim` files in `collections/`, sorted.
pub fn zim_names(lib: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(lib.join("collections")).into_iter().flatten().flatten().filter_map(|e| e.file_name().into_string().ok()).filter(|n| n.ends_with(".zim")).collect();
    v.sort();
    v
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
