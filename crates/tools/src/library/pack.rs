//! build-pack: a LastbookData folder as the zip setup downloads, Lastbook-<id>.zip, and its manifest entry.
//!
//!   lastbook-tools build-pack --data derived/packs/starter-li/LastbookData --out derived/packs [--base https://...]
//!
//! The zip holds exactly the files pack-manifest.tsv lists (each checked against its size and SHA-256 on the way in),
//! and pack-manifest.tsv itself, under LastbookData/: what setup's installer accepts (crates/engine/src/setup/install.rs).
//! Files that are compressed already (PDFs, pictures, videos, encyclopedias) are stored, the rest deflated. Members
//! are in path order and dated pack.json's "built", so the same folder gives the same zip. Before the entry is
//! printed the zip goes through the installer's own check, and the entry through setup's manifest check.

use super::build::manifest_rows;
use super::{sha256_file, Args};
use lastbook_engine::dist;
use lastbook_engine::setup::install::{self, DATA_NAME};
use lastbook_engine::setup::manifest;
use lastbook_engine::setup::net::Allow;
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;
use zip::write::SimpleFileOptions;

const HELP: &str = "lastbook-tools build-pack --data LastbookData --out FOLDER [--base URL]

  --data   a LastbookData folder made by build-library (it holds pack.json and pack-manifest.tsv)
  --out    the folder to write Lastbook-<id>.zip into
  --base   the address the zip will be published at, without the file name (default: the download site's)
Prints the pack's entry for the manifest (manifest-v1.json, \"packs\").";

/// Already compressed: deflating them again only costs time.
const STORED: [&str; 14] = ["pdf", "zim", "mp4", "m4v", "webm", "mkv", "mp3", "m4a", "ogg", "jpg", "jpeg", "png", "webp", "zip"];

pub fn main(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{HELP}");
        return 0;
    }
    let run = || -> Result<Value, String> {
        let a = Args::parse(args, &[])?;
        let base = a.get("base").map(str::to_string).unwrap_or_else(dist::download_base);
        build_pack(Path::new(a.need("data")?), Path::new(a.need("out")?), &base).map(|(_, entry)| entry)
    };
    match run() {
        Ok(entry) => {
            println!("{}", serde_json::to_string_pretty(&entry).unwrap_or_default());
            0
        }
        Err(e) => {
            eprintln!("build-pack: {e}\n\n{HELP}");
            1
        }
    }
}

/// pack-manifest.tsv's rows, read back.
fn listed(data: &Path) -> Result<Vec<(String, u64, String)>, String> {
    let path = data.join("pack-manifest.tsv");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e} (build-library writes it)", path.display()))?;
    let mut rows = Vec::new();
    for line in text.lines().skip(1).filter(|l| !l.trim().is_empty()) {
        let parts: Vec<&str> = line.split('\t').collect();
        match (parts.first(), parts.get(1).and_then(|n| n.parse::<u64>().ok()), parts.get(2)) {
            (Some(p), Some(n), Some(h)) => rows.push((p.to_string(), n, h.to_string())),
            _ => return Err(format!("{}: a line that is not path, bytes and sha256: {line:?}", path.display())),
        }
    }
    Ok(rows)
}

/// The zip's date for every member: pack.json's "built" (YYYY-MM-DD), else 2026-01-01.
fn zip_date(pack: &Value) -> zip::DateTime {
    let d: Vec<u16> = pack["built"].as_str().unwrap_or("").split('-').filter_map(|p| p.parse().ok()).collect();
    match d.as_slice() {
        [y, m, day] => zip::DateTime::from_date_and_time(*y, *m as u8, *day as u8, 0, 0, 0).ok(),
        _ => None,
    }
    .unwrap_or_else(|| zip::DateTime::from_date_and_time(2026, 1, 1, 0, 0, 0).unwrap())
}

/// Writes `<out>/Lastbook-<id>.zip`; gives its path and its manifest entry.
pub fn build_pack(data: &Path, out: &Path, base: &str) -> Result<(std::path::PathBuf, Value), String> {
    let pack: Value = install::read_json(&data.join("pack.json")).ok_or_else(|| format!("{}: no pack.json", data.display()))?;
    let id = pack["name"].as_str().filter(|n| manifest::name_ok(n)).ok_or("pack.json has no usable \"name\"")?.to_string();
    if !install::is_library(data) {
        return Err(format!("{}: no library-data/catalog.json", data.display()));
    }
    // the folder must be what its manifest says: every file there, at its size and hash, and nothing else
    let rows = listed(data)?;
    let on_disk = manifest_rows(data)?;
    let names = |r: &[(String, u64, String)]| r.iter().map(|x| x.0.clone()).collect::<Vec<_>>();
    if names(&rows) != names(&on_disk) {
        let extra: Vec<_> = names(&on_disk).into_iter().filter(|n| !rows.iter().any(|r| &r.0 == n)).collect();
        let missing: Vec<_> = names(&rows).into_iter().filter(|n| !on_disk.iter().any(|r| &r.0 == n)).collect();
        return Err(format!("the folder and pack-manifest.tsv disagree: not listed {extra:?}, missing {missing:?}. Build it again."));
    }
    for (listed, here) in rows.iter().zip(&on_disk) {
        if listed != here {
            return Err(format!("{} has changed since pack-manifest.tsv was written. Build it again.", listed.0));
        }
    }
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let file_name = format!("Lastbook-{id}.zip");
    let zip_path = out.join(&file_name);
    let part = out.join(format!("{file_name}.part"));
    let date = zip_date(&pack);
    {
        let f = std::fs::File::create(&part).map_err(|e| format!("{}: {e}", part.display()))?;
        let mut z = zip::ZipWriter::new(f);
        let mut members: Vec<(String, u64)> = rows.iter().map(|r| (r.0.clone(), r.1)).collect();
        members.push(("pack-manifest.tsv".into(), std::fs::metadata(data.join("pack-manifest.tsv")).map(|m| m.len()).unwrap_or(0)));
        members.sort();
        for (rel, size) in members {
            let ext = rel.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
            let method = if STORED.contains(&ext.as_str()) { zip::CompressionMethod::Stored } else { zip::CompressionMethod::Deflated };
            let opts = SimpleFileOptions::default().compression_method(method).unix_permissions(0o644).last_modified_time(date).large_file(size >= u32::MAX as u64);
            z.start_file(format!("{DATA_NAME}/{rel}"), opts).map_err(|e| format!("{rel}: {e}"))?;
            let mut src = std::fs::File::open(data.join(&rel)).map_err(|e| format!("{rel}: {e}"))?;
            std::io::copy(&mut src, &mut z).map_err(|e| format!("{rel}: {e}"))?;
        }
        z.finish().map_err(|e| format!("{}: {e}", part.display()))?.flush().map_err(|e| e.to_string())?;
    }
    std::fs::rename(&part, &zip_path).map_err(|e| format!("{}: {e}", zip_path.display()))?;
    // the installer's own check: names, kinds of member, compression, no overlap, and the unpacked size
    let installed: u64 = rows.iter().map(|r| r.1).sum::<u64>() + std::fs::metadata(data.join("pack-manifest.tsv")).map(|m| m.len()).unwrap_or(0);
    let (_, total) = install::check_zip(&zip_path, Some(installed), None).map_err(|e| format!("setup would refuse this zip: {}", e.message()))?;
    let size = std::fs::metadata(&zip_path).map_err(|e| e.to_string())?.len();
    let sha = sha256_file(&zip_path).map_err(|e| e.to_string())?;
    let records = install::read_json(&data.join("library-data").join("catalog.json")).and_then(|c| c.as_array().map(|a| a.len()));
    let entry = json!({
        "id": id, "title": pack["title"].as_str().unwrap_or(&id), "summary": pack["summary"].as_str().unwrap_or(""),
        "revision": pack["content_revision"].as_str().unwrap_or(""), "map": pack["map"].as_str().unwrap_or("base"), "records": records,
        "installed_bytes": total, "download_bytes": size, "available": true,
        "files": [{"name": file_name, "url": format!("{}/{file_name}", base.trim_end_matches('/')), "size": size, "sha256": sha, "kind": "zip"}],
    });
    // and setup's manifest check, as the app reads a manifest
    let host = base.split("://").nth(1).unwrap_or("").split(['/', ':']).next().unwrap_or("").to_string();
    let allow = if host == dist::DOWNLOAD_HOST { Allow::Product } else { Allow::Only(vec![host]) };
    if manifest::validate(&json!({"version": 1, "packs": [entry.clone()], "models": [], "maps": [], "address_indexes": []}), &allow).is_none() {
        return Err("the entry does not pass setup's manifest check".into());
    }
    Ok((zip_path, entry))
}
