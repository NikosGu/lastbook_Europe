//! The zips setup downloads, and their entries in the manifest, made so that setup's own checks accept them
//! (crates/engine/src/setup/install.rs, `check_zip` with `map_member`; api.rs, `do_map` and `do_addresses`):
//!
//!   Lastbook-Map-<REGION>.zip        map-set.json and the files it lists, at the top (setup puts them under
//!                                    LastbookData/); deflated, the elevation tiles stored (PNG already compresses)
//!   Lastbook-Addresses-<REGION>.zip  derived/geo/addresses/<REGION>.sqlite alone
//!
//! Each zip is checked with the installer's own `check_zip` before its entry is given.

use lastbook_engine::regions;
use lastbook_engine::setup::install;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::Path;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime};

pub const MAP_INCLUDES: &str = "Streets, buildings, places, help points, water and elevation, with Europe's main roads zoomed out";

fn sha256(path: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// Writes a zip of `members` (name in the zip, file on disk), checked against `allowed` as setup will check it, and
/// returns its size, SHA-256 and the bytes it unpacks to.
fn zip(out: &Path, members: &[(String, std::path::PathBuf)], allowed: install::Allowed) -> Result<(u64, String, u64), String> {
    let partial = out.with_extension("partial");
    {
        let mut z = zip::ZipWriter::new(std::fs::File::create(&partial).map_err(|e| e.to_string())?);
        let when = DateTime::from_date_and_time(2026, 11, 17, 0, 0, 0).map_err(|e| e.to_string())?;
        for (name, file) in members {
            let stored = name.ends_with("terrain.sqlite");
            let o = SimpleFileOptions::default()
                .compression_method(if stored { CompressionMethod::Stored } else { CompressionMethod::Deflated })
                .last_modified_time(when)
                .unix_permissions(0o644)
                .large_file(std::fs::metadata(file).map(|m| m.len()).unwrap_or(0) >= 1 << 32);
            z.start_file(name, o).map_err(|e| e.to_string())?;
            let mut src = std::fs::File::open(file).map_err(|e| format!("{}: {e}", file.display()))?;
            std::io::copy(&mut src, &mut z).map_err(|e| e.to_string())?;
        }
        z.finish().map_err(|e| e.to_string())?.flush().map_err(|e| e.to_string())?;
    }
    std::fs::rename(&partial, out).map_err(|e| e.to_string())?;
    let installed: u64 = members.iter().map(|(_, f)| std::fs::metadata(f).map(|m| m.len()).unwrap_or(0)).sum();
    install::check_zip(out, Some(installed), Some(allowed)).map_err(|e| format!("setup would refuse {}: {e:?}", out.display()))?;
    let size = std::fs::metadata(out).map_err(|e| e.to_string())?.len();
    Ok((size, sha256(out)?, installed))
}

/// The region map zip of a built map set (`map/` of mapset.rs), and its entry under `maps` in the manifest, the zip at
/// `<url_base>/<name>` (a GitHub release holds its files side by side).
pub fn map(map_dir: &Path, out_dir: &Path, url_base: &str) -> Result<Value, String> {
    let set: Value = serde_json::from_slice(&std::fs::read(map_dir.join("map-set.json")).map_err(|e| format!("{}: {e}", map_dir.display()))?).map_err(|e| e.to_string())?;
    let region = set["name"].as_str().filter(|r| regions::known(r)).ok_or("map-set.json names no known region")?.to_string();
    let mut members = vec![("map-set.json".to_string(), map_dir.join("map-set.json"))];
    for rel in set["files"].as_object().ok_or("map-set.json lists no files")?.keys() {
        if !install::map_member(rel, false) {
            return Err(format!("{rel} is not a file a region map may hold"));
        }
        members.push((rel.clone(), map_dir.join(rel)));
    }
    std::fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
    let name = format!("Lastbook-Map-{region}.zip");
    let (size, sha, installed) = zip(&out_dir.join(&name), &members, &install::map_member)?;
    Ok(json!({"id": region, "title": regions::name(&region), "kind": "region", "includes": MAP_INCLUDES, "installed_bytes": installed,
              "download_bytes": size, "available": true,
              "files": [{"name": name, "url": format!("{}/{name}", url_base.trim_end_matches('/')), "size": size, "sha256": sha, "kind": "zip"}]}))
}

/// The address index zip of a region (`addresses/<REGION>.sqlite` of mapset.rs), and its entry under `address_indexes`,
/// the zip at `<url_base>/<name>`.
pub fn addresses(index: &Path, out_dir: &Path, url_base: &str) -> Result<Value, String> {
    let region = index.file_stem().and_then(|s| s.to_str()).filter(|r| regions::known(r)).ok_or("the index must be named after its region, as LI.sqlite")?.to_string();
    let rel = format!("derived/geo/addresses/{region}.sqlite");
    let count: u64 = rusqlite::Connection::open_with_flags(index, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .and_then(|c| c.query_row("SELECT value FROM meta WHERE key='count'", [], |r| r.get::<_, String>(0)))
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    std::fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
    let name = format!("Lastbook-Addresses-{region}.zip");
    let dirs = ["", "derived", "derived/geo", "derived/geo/addresses"];
    let allowed = |n: &str, is_dir: bool| if is_dir { dirs.contains(&n) } else { n == rel };
    let (size, sha, installed) = zip(&out_dir.join(&name), &[(rel.clone(), index.to_path_buf())], &allowed)?;
    Ok(json!({"id": region, "title": regions::name(&region), "kind": "region-addresses", "records": count, "installed_bytes": installed,
              "download_bytes": size, "available": true,
              "files": [{"name": name, "url": format!("{}/{name}", url_base.trim_end_matches('/')), "size": size, "sha256": sha, "kind": "zip"}]}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lastbook_engine::setup::download::Control;
    use lastbook_engine::setup::{manifest, net};

    #[test]
    fn setup_accepts_the_zips_and_their_entries() {
        let dir = std::env::temp_dir().join(format!("lb-tools-pack-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let map = dir.join("map");
        std::fs::create_dir_all(map.join("derived/geo")).unwrap();
        std::fs::write(map.join("derived/geo/national.sqlite"), vec![7u8; 5000]).unwrap();
        std::fs::write(map.join("derived/geo/terrain.sqlite"), vec![9u8; 3000]).unwrap();
        let status = r#"{"state":"complete","features":1}"#;
        std::fs::write(map.join("derived/geo/national-status.json"), status).unwrap();
        std::fs::write(map.join("map-set.json"), json!({"name": "LI", "files": {"derived/geo/national.sqlite": 5000, "derived/geo/terrain.sqlite": 3000,
            "derived/geo/national-status.json": status.len()}}).to_string()).unwrap();
        let out = dir.join("out");
        let base = lastbook_engine::dist::download_base();
        let m = map_entry_checked(&map, &out, &base);
        let idx = dir.join("LI.sqlite");
        std::fs::write(&idx, vec![1u8; 4096]).unwrap();
        let a = addresses(&idx, &out, &base).unwrap();
        // side by side in the data release, as a GitHub release holds its files
        assert_eq!(a["files"][0]["url"], format!("{base}/Lastbook-Addresses-LI.zip"));
        // the manifest takes both entries as they are
        let v = manifest::validate(&json!({"version": 1, "packs": [], "models": [], "maps": [m.clone()], "address_indexes": [a.clone()]}), &net::Allow::Product).unwrap();
        assert_eq!(v["maps"][0]["available"], true);
        assert_eq!(v["maps"][0]["download_bytes"], m["files"][0]["size"]);
        assert_eq!(v["address_indexes"][0]["installed_bytes"], 4096);
        // setup unpacks the map as a map, and the address index where it belongs
        let control = Control::new();
        let lib = install::unpack(&out.join("Lastbook-Map-LI.zip"), &dir.join("unpack"), m["installed_bytes"].as_u64(), &control, &mut |_, _| {}, Some(&install::map_member), None).unwrap();
        assert_eq!(install::map_name(&lib), "LI");
        assert!(lib.join("derived/geo/terrain.sqlite").is_file());
        let mut z = zip::ZipArchive::new(std::fs::File::open(out.join("Lastbook-Addresses-LI.zip")).unwrap()).unwrap();
        assert_eq!(z.len(), 1);
        assert_eq!(z.by_index(0).unwrap().name().unwrap(), "derived/geo/addresses/LI.sqlite");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn map_entry_checked(map_dir: &Path, out: &Path, base: &str) -> Value {
        let e = super::map(map_dir, out, base).unwrap();
        assert_eq!((e["id"].as_str(), e["kind"].as_str()), (Some("LI"), Some("region")));
        assert_eq!(e["files"][0]["url"], format!("{base}/Lastbook-Map-LI.zip"));
        assert_eq!(e["installed_bytes"].as_u64().unwrap(), 5000 + 3000 + 33 + std::fs::metadata(map_dir.join("map-set.json")).unwrap().len());
        let mut z = zip::ZipArchive::new(std::fs::File::open(out.join("Lastbook-Map-LI.zip")).unwrap()).unwrap();
        assert_eq!(z.by_name("derived/geo/terrain.sqlite").unwrap().compression(), CompressionMethod::Stored);
        assert_eq!(z.by_name("derived/geo/national.sqlite").unwrap().compression(), CompressionMethod::Deflated);
        e
    }
}
