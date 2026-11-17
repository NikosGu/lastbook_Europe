//! A built region's zips checked as setup takes them and as a person would use them: the manifest entries setup's own
//! check accepts, the zips' sizes and SHA-256 as the entries give them, `install::check_zip` and the unpacking with
//! setup's member rules; then the engine on the unpacked folder: the region's town found by the place search inside
//! its boxes, the addresses above a floor and one of them found as typed, and detail in every box (a missing extract,
//! Madeira's say, leaves a box empty).

use lastbook_engine::map::Map;
use lastbook_engine::paths::Paths;
use lastbook_engine::regions;
use lastbook_engine::setup::{download::Control, install, manifest, net};
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

/// What the place search may answer for a town.
const TOWNS: &[&str] = &["national capital", "city", "town", "village", "municipality", "suburb", "hamlet", "locality", "quarter", "neighbourhood"];

fn sha256(path: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
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

/// The zip an entry names, in `packs`, as the entry gives it: its size and SHA-256.
fn file(packs: &Path, entry: &Value) -> Result<(std::path::PathBuf, u64), String> {
    let f = &entry["files"][0];
    let path = packs.join(f["name"].as_str().ok_or("an entry without a file")?);
    let size = std::fs::metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?.len();
    if Some(size) != f["size"].as_u64() || sha256(&path)? != f["sha256"].as_str().unwrap_or("") {
        return Err(format!("{} is not the size or SHA-256 its entry gives", path.display()));
    }
    Ok((path, entry["installed_bytes"].as_u64().ok_or("an entry without installed_bytes")?))
}

/// Checks the region's zips and entries in `packs` (as `pack-map` and `pack-addresses` wrote them, the entries saved as
/// map-entry-<REGION>.json and addresses-entry-<REGION>.json), unpacking into `work`. Every check is listed, passed or
/// not; the first that cannot go on is the error.
pub fn check(region: &str, packs: &Path, work: &Path, town: &str, min_addresses: u64) -> Result<Value, String> {
    let read = |name: String| -> Result<Value, String> {
        serde_json::from_slice(&std::fs::read(packs.join(&name)).map_err(|e| format!("{name}: {e}"))?).map_err(|e| format!("{name}: {e}"))
    };
    let (m, a) = (read(format!("map-entry-{region}.json"))?, read(format!("addresses-entry-{region}.json"))?);
    let mut checks = Vec::new();
    let mut note = |name: &str, ok: bool, detail: String| checks.push(json!({"check": name, "ok": ok, "detail": detail}));
    let manifest = json!({"version": 1, "packs": [], "models": [], "maps": [m.clone()], "address_indexes": [a.clone()]});
    // setup's own check keeps only the entries it accepts
    let kept = manifest::validate(&manifest, &net::Allow::Product).is_some_and(|v| v["maps"][0]["id"] == region && v["address_indexes"][0]["id"] == region);
    note("manifest entries", kept, format!("{} and {}", m["id"], a["id"]));
    // the zips as setup takes them
    let (map_zip, map_bytes) = file(packs, &m)?;
    let (addr_zip, addr_bytes) = file(packs, &a)?;
    let rel = format!("derived/geo/addresses/{region}.sqlite");
    let dirs = ["", "derived", "derived/geo", "derived/geo/addresses"];
    let addr_member = |n: &str, is_dir: bool| if is_dir { dirs.contains(&n) } else { n == rel };
    install::check_zip(&map_zip, Some(map_bytes), Some(&install::map_member)).map_err(|e| format!("setup would refuse the map: {e:?}"))?;
    install::check_zip(&addr_zip, Some(addr_bytes), Some(&addr_member)).map_err(|e| format!("setup would refuse the addresses: {e:?}"))?;
    note("setup's zip check", true, format!("{} and {} bytes installed", map_bytes, addr_bytes));
    let _ = std::fs::remove_dir_all(work);
    let lib = install::unpack(&map_zip, &work.join("unpack"), Some(map_bytes), &Control::new(), &mut |_, _| {}, Some(&install::map_member), None)
        .map_err(|e| format!("unpacking the map: {e:?}"))?;
    // as setup does: the index there, at its size, is all an address zip must give
    let verify = |l: &Path| -> Result<(), lastbook_engine::setup::SetupError> {
        match std::fs::metadata(l.join(&rel)) {
            Ok(m) if m.len() == addr_bytes => Ok(()),
            _ => Err(lastbook_engine::setup::SetupError::refused("unpack", "The address index is missing or the wrong size.")),
        }
    };
    let unpacked = install::unpack(&addr_zip, &work.join("unpack-addresses"), Some(addr_bytes), &Control::new(), &mut |_, _| {}, Some(&addr_member), Some(&verify))
        .map_err(|e| format!("unpacking the addresses: {e:?}"))?;
    std::fs::create_dir_all(lib.join("derived/geo/addresses")).map_err(|e| e.to_string())?;
    std::fs::rename(unpacked.join(&rel), lib.join(&rel)).map_err(|e| e.to_string())?;
    note("the map's name", install::map_name(&lib) == region, install::map_name(&lib));
    // what a person would ask of it
    let map = Map::new(Paths::new(work.join("home"), Some(lib.clone())));
    let first = map.places(town, 5, None).into_iter().next().unwrap_or(json!({}));
    let (lat, lon) = (first["lat"].as_f64().unwrap_or(0.0), first["lon"].as_f64().unwrap_or(0.0));
    let found = first["kind"].as_str().is_some_and(|k| TOWNS.contains(&k)) && regions::contains(region, lat, lon);
    note("the town found", found, format!("{town}: {} ({}) at {lat:.4}, {lon:.4}", first["name"], first["kind"]));
    let index = Connection::open_with_flags(lib.join(&rel), OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| e.to_string())?;
    let count: u64 = index.query_row("SELECT value FROM meta WHERE key='count'", [], |r| r.get::<_, String>(0)).ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    note("addresses", count >= min_addresses, format!("{count}, at least {min_addresses}"));
    let one: Option<(String, String, String)> = index.query_row("SELECT num, street, city FROM addr ORDER BY rowid LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).ok();
    if let Some((num, street, city)) = one {
        let q = if city.is_empty() { format!("{street} {num}") } else { format!("{street} {num}, {city}") };
        let got = map.find_addresses(&q, 5, None);
        note("an address found as typed", !got.is_empty(), format!("{q}: {}", got.first().map(|g| g["name"].to_string()).unwrap_or_default()));
    }
    drop(index);
    let national = Connection::open_with_flags(lib.join("derived/geo/national.sqlite"), OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| e.to_string())?;
    for b in regions::boxes(region).unwrap_or(&[]) {
        let n: i64 = national.query_row("SELECT count(*) FROM feat_idx WHERE maxz >= 30 AND minx <= ?3 AND maxx >= ?1 AND miny <= ?4 AND maxy >= ?2",
                                        rusqlite::params![b[0], b[1], b[2], b[3]], |r| r.get(0)).unwrap_or(0);
        note("detail in the box", n > 0, format!("{b:?}: {n} features"));
    }
    drop(national);
    let tiles: i64 = Connection::open_with_flags(lib.join("derived/geo/terrain.sqlite"), OpenFlags::SQLITE_OPEN_READ_ONLY)
        .and_then(|c| c.query_row("SELECT count(*) FROM tile", [], |r| r.get(0))).unwrap_or(0);
    note("elevation tiles", tiles > 0, tiles.to_string());
    drop(map);
    lastbook_engine::db::close_all();
    let ok = checks.iter().all(|c| c["ok"] == true);
    Ok(json!({"region": region, "ok": ok, "checks": checks}))
}
