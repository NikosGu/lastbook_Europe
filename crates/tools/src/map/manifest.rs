//! The data release's manifest-v1.json, which setup fetches to learn what it can install: the model catalogue it starts
//! from (packaging/data/manifest-base.json), every region's map and address index whose entry is among the release's
//! files (map-entry-<REGION>.json, addresses-entry-<REGION>.json, as pack-map and pack-addresses print them), and the
//! starter pack's (starter-entry.json, as build-pack prints it). It is checked as setup checks a fetched manifest:
//! every address in the data release on this project's hosts, every entry kept and available.

use lastbook_engine::dist;
use lastbook_engine::setup::{manifest, net};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The entries a file holds: one entry, a list of them, or a manifest's list under `key`.
fn listed(v: &Value, key: &str) -> Vec<Value> {
    match v {
        Value::Array(a) => a.clone(),
        Value::Object(o) if o.contains_key(key) => o[key].as_array().cloned().unwrap_or_default(),
        Value::Object(_) => vec![v.clone()],
        _ => Vec::new(),
    }
}

/// The release's files and their sizes, from `gh release view --json assets`: only those uploaded whole.
fn uploaded(assets: &Value) -> BTreeMap<String, u64> {
    assets["assets"].as_array().into_iter().flatten()
        .filter(|a| a["state"].as_str().is_none_or(|s| s == "uploaded"))
        .filter_map(|a| Some((a["name"].as_str()?.to_string(), a["size"].as_u64()?)))
        .collect()
}

/// The manifest from `base` and the entries in the folder `entries`, and what it holds. With `assets`, an entry whose
/// file is not among the release's files at its size is left out, and said so.
pub fn build(base: &Value, entries: &Path, assets: Option<&Value>) -> Result<(Value, Value), String> {
    if base["version"].as_u64() != Some(1) {
        return Err("the base is not a manifest of version 1".into());
    }
    let mut m = base.clone();
    let on_release = assets.map(uploaded);
    let mut left_out = Vec::new();
    let mut names: Vec<String> = std::fs::read_dir(entries).map_err(|e| format!("{}: {e}", entries.display()))?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .collect();
    names.sort();
    for name in names {
        let region = |prefix: &str| name.strip_prefix(prefix).and_then(|n| n.strip_suffix(".json")).map(str::to_string);
        let (key, id) = if let Some(r) = region("map-entry-") {
            ("maps", Some(r))
        } else if let Some(r) = region("addresses-entry-") {
            ("address_indexes", Some(r))
        } else if name == "starter-entry.json" {
            ("packs", None)
        } else {
            continue;
        };
        let path = entries.join(&name);
        let v: Value = serde_json::from_slice(&std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?).map_err(|e| format!("{name}: {e}"))?;
        for e in listed(&v, key) {
            let eid = e["id"].as_str().unwrap_or("").to_string();
            if id.as_ref().is_some_and(|id| *id != eid) {
                return Err(format!("{name} holds the entry of {eid:?}"));
            }
            let files = e["files"].as_array().cloned().unwrap_or_default();
            for f in &files {
                // a file of the data release, by its own name: a release holds its files side by side
                let want = format!("{}/{}", dist::download_base(), f["name"].as_str().unwrap_or(""));
                if f["url"].as_str() != Some(want.as_str()) {
                    return Err(format!("{name}: {} is not {want}", f["url"]));
                }
            }
            if let Some(release) = &on_release {
                let missing: Vec<&str> = files.iter().filter(|f| release.get(f["name"].as_str().unwrap_or("")) != f["size"].as_u64().as_ref())
                    .filter_map(|f| f["name"].as_str()).collect();
                if !missing.is_empty() || files.is_empty() {
                    left_out.push(json!({"entry": name, "id": eid, "not_in_the_release": missing}));
                    continue;
                }
            }
            let list = m[key].as_array_mut().ok_or_else(|| format!("the base has no {key} list"))?;
            list.retain(|x| x["id"] != e["id"]);
            list.push(e);
        }
    }
    for key in ["maps", "address_indexes"] {
        if let Some(list) = m[key].as_array_mut() {
            list.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        }
    }
    // as setup takes it: the whole manifest refused for one bad entry, so every entry must come through, available
    let v = manifest::validate(&m, &net::Allow::Product).ok_or("setup would refuse this manifest: an entry is not as it should be")?;
    let mut held = Map::new();
    for key in manifest::KEYS {
        let given = m[key].as_array().map_or(0, Vec::len);
        let kept = v[key].as_array().cloned().unwrap_or_default();
        if kept.len() != given {
            return Err(format!("setup would keep {} of the {given} {key}", kept.len()));
        }
        if key != "models" {
            if let Some(off) = kept.iter().zip(m[key].as_array().into_iter().flatten()).find(|(k, x)| x["available"] == true && k["available"] != true) {
                return Err(format!("setup would not offer {} of {key}", off.0["id"]));
            }
        }
        held.insert(key.to_string(), json!(kept.iter().filter_map(|x| x["id"].as_str()).collect::<Vec<_>>()));
    }
    // a region with its map and no address index, or the other way round, is listed: setup offers what there is
    let ids = |key: &str| -> BTreeSet<String> { m[key].as_array().into_iter().flatten().filter_map(|x| x["id"].as_str().map(str::to_string)).collect() };
    let (maps, indexes) = (ids("maps"), ids("address_indexes"));
    held.insert("maps_without_addresses".into(), json!(maps.difference(&indexes).collect::<Vec<_>>()));
    held.insert("addresses_without_maps".into(), json!(indexes.difference(&maps).collect::<Vec<_>>()));
    held.insert("left_out".into(), Value::Array(left_out));
    Ok((m, Value::Object(held)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, kind: &str, name: &str, size: u64) -> Value {
        json!({"id": id, "title": id, "kind": kind, "installed_bytes": size * 2, "available": true,
               "files": [{"name": name, "url": format!("{}/{name}", dist::download_base()), "size": size, "sha256": "a".repeat(64), "kind": "zip"}]})
    }

    fn folder(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("lb-tools-manifest-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn base() -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/data/manifest-base.json");
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    #[test]
    fn the_catalogue_with_every_region_and_the_starter_pack_found() {
        let dir = folder("all");
        for (id, size) in [("LU", 300), ("LI", 100)] {
            std::fs::write(dir.join(format!("map-entry-{id}.json")), entry(id, "region", &format!("Lastbook-Map-{id}.zip"), size).to_string()).unwrap();
            std::fs::write(dir.join(format!("addresses-entry-{id}.json")), entry(id, "region-addresses", &format!("Lastbook-Addresses-{id}.zip"), size / 10).to_string()).unwrap();
        }
        std::fs::write(dir.join("MT.json"), "not an entry").unwrap();
        std::fs::write(dir.join("check-LI.json"), "{}").unwrap();
        let mut starter = entry("starter", "", "Lastbook-starter.zip", 5000);
        starter["records"] = json!(42);
        std::fs::write(dir.join("starter-entry.json"), starter.to_string()).unwrap();
        let (m, held) = build(&base(), &dir, None).unwrap();
        assert_eq!(held["maps"], json!(["LI", "LU"]), "sorted by region");
        assert_eq!(held["address_indexes"], json!(["LI", "LU"]));
        assert_eq!(held["packs"], json!(["starter"]));
        assert_eq!(held["models"], base()["models"].as_array().unwrap().iter().map(|x| x["id"].clone()).collect::<Value>(), "the catalogue kept");
        assert_eq!(m["maps"][0]["files"][0]["url"], format!("{}/Lastbook-Map-LI.zip", dist::download_base()));
        assert!(manifest::validate(&m, &net::Allow::Product).is_some());
        // with the release's files: one upload not finished, one at another size, and the rest there
        let assets = json!({"assets": [
            {"name": "Lastbook-Map-LI.zip", "size": 100, "state": "uploaded"}, {"name": "Lastbook-Addresses-LI.zip", "size": 10, "state": "uploaded"},
            {"name": "Lastbook-Map-LU.zip", "size": 300, "state": "starter"}, {"name": "Lastbook-Addresses-LU.zip", "size": 31},
            {"name": "Lastbook-starter.zip", "size": 5000}]});
        let (m, held) = build(&base(), &dir, Some(&assets)).unwrap();
        assert_eq!((held["maps"].clone(), held["address_indexes"].clone()), (json!(["LI"]), json!(["LI"])));
        assert_eq!(held["left_out"].as_array().unwrap().len(), 2, "{held}");
        assert_eq!(m["packs"][0]["records"], 42);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_entry_elsewhere_or_of_another_region_stops_it() {
        let dir = folder("bad");
        // pack-map's old layout, a maps/ folder, which a GitHub release does not have
        let mut e = entry("LI", "region", "Lastbook-Map-LI.zip", 100);
        e["files"][0]["url"] = json!(format!("{}/maps/Lastbook-Map-LI.zip", dist::download_base()));
        std::fs::write(dir.join("map-entry-LI.json"), e.to_string()).unwrap();
        assert!(build(&base(), &dir, None).unwrap_err().contains("is not"));
        std::fs::write(dir.join("map-entry-LI.json"), entry("LU", "region", "Lastbook-Map-LU.zip", 100).to_string()).unwrap();
        assert!(build(&base(), &dir, None).unwrap_err().contains("LU"));
        // a file setup would find empty: the entry would not be offered
        std::fs::write(dir.join("map-entry-LI.json"), entry("LI", "region", "Lastbook-Map-LI.zip", 0).to_string()).unwrap();
        assert!(build(&base(), &dir, None).is_err());
        assert!(build(&json!({"version": 2}), &dir, None).is_err());
        // nothing found yet: the catalogue alone
        std::fs::remove_file(dir.join("map-entry-LI.json")).unwrap();
        let (m, held) = build(&base(), &dir, None).unwrap();
        assert_eq!((held["maps"].clone(), m["models"].clone()), (json!([]), base()["models"].clone()));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
