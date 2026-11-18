//! Unpacking a pack and switching to it, so that a working library is never broken.
//!
//!   <parent>/LastbookData.setup/unpack/LastbookData    the new library, built here from the pack's zip files
//!   <parent>/LastbookData                             the library the app opens
//!
//! Every member of a zip is checked before anything is written: a relative name under LastbookData/, no `..`, no drive
//! or root, no link, no device, no encryption, stored or deflated only, no two members sharing bytes, and a total no
//! larger than the manifest's installed_bytes. Files are written as `<name>.partial` and renamed when whole, so an unpack
//! that stops carries on where it was. Switching is one rename on one disk. An upgrade first moves the old library aside
//! to LastbookData.old-<date>; when the app restarts it opens the new one, and only if that fails is the old one put back
//! (apply_pending, called before the engine opens anything).

use super::download::Control;
use super::SetupError;
use crate::regions;
use serde_json::{json, Map, Value};
use std::collections::{BTreeSet, HashSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DATA_NAME: &str = "LastbookData";
const SLACK: f64 = 1.02; // installed_bytes is measured; a zip's members may add a little in directory entries
pub const SPACE_MARGIN: u64 = 2_000_000_000;
const COPY: usize = 4 * 1024 * 1024;

pub const MAP_FIXED: &[&str] = &["map-set.json", "derived/geo/national-status.json"];
const MAP_DIRS: &[&str] = &["", "derived", "derived/geo"];
const SQLITE_SIDES: &[&str] = &["-wal", "-shm", "-journal"]; // SQLite's own files beside a database: they move with it

fn refused(m: impl Into<String>) -> SetupError {
    SetupError::refused("unpack", m)
}

fn activate(m: impl Into<String>) -> SetupError {
    SetupError::refused("activate", m)
}

fn io_error(e: std::io::Error) -> SetupError {
    if super::download::disk_full(&e) {
        SetupError::failed("no_space", "The disk filled up. Free some space, then try again.")
    } else {
        refused(format!("Lastbook Europe could not write a file ({e})."))
    }
}

fn short(name: &str) -> String {
    let s: String = name.chars().take(80).collect();
    format!("{s:?}")
}

pub fn is_library(folder: &Path) -> bool {
    folder.join("library-data").join("catalog.json").is_file()
}

/// Is this path (under LastbookData) one a region map zip may hold? docs/SETUP-API.md, Region maps.
pub fn map_member(rel: &str, is_dir: bool) -> bool {
    let rel = rel.trim_matches('/');
    if is_dir {
        return MAP_DIRS.contains(&rel);
    }
    if MAP_FIXED.contains(&rel) {
        return true;
    }
    match rel.strip_prefix("derived/geo/").and_then(|n| n.strip_suffix(".sqlite")) {
        Some(stem) => !stem.is_empty() && stem.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-')),
        None => false,
    }
}

/// A rule narrower than "under LastbookData": (path under LastbookData, is a folder) -> allowed.
pub type Allowed<'a> = &'a dyn Fn(&str, bool) -> bool;

#[derive(Debug, Clone)]
pub struct Member {
    pub index: usize,
    pub parts: Vec<String>,
    pub is_dir: bool,
    pub size: u64,
    pub mtime: Option<i64>,
}

fn member_parts(name: &str, is_dir: bool, unix_mode: Option<u32>, encrypted: bool, top_optional: bool) -> Result<Vec<String>, SetupError> {
    if name.contains('\\') || name.contains('\0') {
        return Err(refused(format!("The pack holds a file with a name Lastbook Europe does not accept ({}).", short(name))));
    }
    let first = name.split('/').next().unwrap_or("");
    if name.starts_with('/') || first.contains(':') {
        return Err(refused(format!("The pack holds a file with an absolute path ({}).", short(name))));
    }
    let mut parts: Vec<String> = name.split('/').filter(|p| !p.is_empty() && *p != ".").map(str::to_string).collect();
    if parts.iter().any(|p| p == "..") {
        return Err(refused(format!("The pack holds a file that would land outside the library ({}).", short(name))));
    }
    if top_optional && !parts.is_empty() && parts[0] != DATA_NAME {
        parts.insert(0, DATA_NAME.to_string()); // a region map zip may hold its files at the top, without the folder
    }
    if parts.is_empty() || parts[0] != DATA_NAME {
        return Err(refused(format!("The pack holds a file outside its LastbookData folder ({}).", short(name))));
    }
    if let Some(mode) = unix_mode {
        let kind = mode & 0o170000;
        if kind != 0 && kind != 0o100000 && !(is_dir && kind == 0o040000) {
            return Err(refused(format!("The pack holds a link or a special file ({}).", short(name))));
        }
    }
    if encrypted {
        return Err(refused("The pack is encrypted."));
    }
    Ok(parts)
}

/// Every member, checked, and the total unpacked bytes; else Refused.
pub fn check_zip(path: &Path, installed_bytes: Option<u64>, allowed: Option<Allowed>) -> Result<(Vec<Member>, u64), SetupError> {
    let file = File::open(path).map_err(|e| refused(format!("The pack could not be opened as a zip file ({e}).")))?;
    let mut z = zip::ZipArchive::new(file).map_err(|e| refused(format!("The pack could not be opened as a zip file ({e}).")))?;
    let mut members = Vec::new();
    let mut seen = HashSet::new();
    let mut total = 0u64;
    let mut spans = Vec::new();
    for i in 0..z.len() {
        let f = z.by_index_raw(i).map_err(|e| refused(format!("The pack could not be read ({e}).")))?;
        let name = String::from_utf8_lossy(f.name_raw()).into_owned();
        let is_dir = f.is_dir();
        let parts = member_parts(&name, is_dir, f.unix_mode(), f.encrypted(), allowed.is_some())?;
        if f.is_symlink() {
            return Err(refused(format!("The pack holds a link or a special file ({}).", short(&name))));
        }
        if let Some(allowed) = allowed {
            if !allowed(&parts[1..].join("/"), is_dir) {
                return Err(refused(format!("The map holds a file that is not part of a map ({}).", short(&name))));
            }
        }
        let key = parts.join("/").to_lowercase(); // two names differing only in case would overwrite each other on a Mac or Windows
        if !is_dir && !seen.insert(key) {
            return Err(refused(format!("The pack holds the same file twice ({}).", short(&name))));
        }
        match f.compression() {
            zip::CompressionMethod::Stored => {
                if f.compressed_size() != f.size() {
                    return Err(refused(format!("The pack holds a stored file whose sizes disagree ({}).", short(&name))));
                }
            }
            zip::CompressionMethod::Deflated => {}
            _ => return Err(refused(format!("The pack uses a kind of compression Lastbook Europe does not accept ({}).", short(&name)))),
        }
        total += f.size();
        spans.push((f.header_start(), f.compressed_size(), name.clone()));
        let mtime = f.last_modified().and_then(zip_time);
        members.push(Member { index: i, parts, is_dir, size: f.size(), mtime });
    }
    // No two entries may share bytes: each local header (at least 30 bytes) and its data end before the next begins, and the
    // last before the central directory.
    spans.sort();
    let end = z.central_directory_start();
    for (k, (start, size, name)) in spans.iter().enumerate() {
        let limit = spans.get(k + 1).map(|s| s.0).unwrap_or(end);
        if start + 30 + size > limit {
            return Err(refused(format!("The pack holds entries that overlap ({}).", short(name))));
        }
    }
    if let Some(inst) = installed_bytes {
        if total as f64 > inst as f64 * SLACK + 1024.0 * 1024.0 {
            return Err(refused(format!("The pack unpacks to {total} bytes, more than the {inst} it should.")));
        }
    }
    Ok((members, total))
}

fn zip_time(t: zip::DateTime) -> Option<i64> {
    let days = days_from_civil(t.year() as i64, t.month() as i64, t.day() as i64);
    Some(days * 86400 + t.hour() as i64 * 3600 + t.minute() as i64 * 60 + t.second() as i64)
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's algorithm).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// (year, month, day) of a day count since 1970-01-01.
pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { yoe + era * 400 + 1 } else { yoe + era * 400 }, m, d)
}

/// Unpacks into `<into>/LastbookData`. progress(done, total). Resumable. `allowed`: map_member for a region map, which then
/// needs map-set.json instead of a catalog; `verify` replaces both checks.
pub fn unpack(
    zip_path: &Path,
    into: &Path,
    installed_bytes: Option<u64>,
    control: &Control,
    progress: &mut dyn FnMut(u64, u64),
    allowed: Option<Allowed>,
    verify: Option<&dyn Fn(&Path) -> Result<(), SetupError>>,
) -> Result<PathBuf, SetupError> {
    let (members, total) = check_zip(zip_path, installed_bytes, allowed)?;
    fs::create_dir_all(into).map_err(io_error)?;
    let root = into.canonicalize().map_err(io_error)?; // on Windows the \\?\ form, with no 260-character limit
    let mut z = zip::ZipArchive::new(File::open(zip_path).map_err(io_error)?).map_err(|e| refused(e.to_string()))?;
    let mut done = 0u64;
    let mut buf = vec![0u8; COPY];
    for m in &members {
        control.check()?;
        let dest = m.parts.iter().fold(root.clone(), |p, part| p.join(part));
        if !dest.starts_with(&root) {
            return Err(refused("The pack holds a file that would land outside the library."));
        }
        if m.is_dir {
            fs::create_dir_all(&dest).map_err(io_error)?;
            continue;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        if fs::symlink_metadata(&dest).map(|md| md.file_type().is_symlink()).unwrap_or(false) {
            return Err(refused("A link appeared in the unpack folder."));
        }
        if dest.is_file() && fs::metadata(&dest).map(|md| md.len()).ok() == Some(m.size) {
            done += m.size; // whole from an earlier run
            progress(done, total);
            continue;
        }
        let part = PathBuf::from(format!("{}.partial", dest.display()));
        {
            let mut src = z.by_index(m.index).map_err(|e| refused(e.to_string()))?;
            let mut out = File::create(&part).map_err(io_error)?;
            let mut written = 0u64;
            loop {
                control.check()?;
                let n = src.read(&mut buf).map_err(|e| refused(format!("A file in the pack could not be read ({e}).")))?;
                if n == 0 {
                    break;
                }
                written += n as u64;
                if written > m.size {
                    return Err(refused(format!("A file in the pack is larger than it says ({}).", short(&m.parts.join("/")))));
                }
                out.write_all(&buf[..n]).map_err(io_error)?;
                done += n as u64;
                progress(done, total);
            }
            out.flush().map_err(io_error)?;
        }
        if fs::metadata(&part).map(|md| md.len()).ok() != Some(m.size) {
            return Err(refused(format!("A file in the pack came out the wrong size ({}).", short(&m.parts.join("/")))));
        }
        fs::rename(&part, &dest).map_err(io_error)?;
        if let Some(t) = m.mtime {
            let _ = filetime::set_file_mtime(&dest, filetime::FileTime::from_unix_time(t, 0));
        }
    }
    let lib = root.join(DATA_NAME);
    if let Some(verify) = verify {
        verify(&lib)?;
    } else if allowed.is_some() {
        check_map(&lib)?;
    } else if !is_library(&lib) {
        return Err(refused("The unpacked pack has no library catalog in it."));
    }
    Ok(lib)
}

// ------------------------------------------------------------------------------------------------------------- space --
/// Room needed on the target disk: what is still to download, plus the unpacked size, plus SPACE_MARGIN.
pub fn need_bytes(pack: Option<&Value>, model: Option<&Value>, have_files: &HashSet<String>) -> u64 {
    let mut n = SPACE_MARGIN;
    if let Some(pack) = pack {
        for f in pack["files"].as_array().into_iter().flatten() {
            if !have_files.contains(f["name"].as_str().unwrap_or("")) {
                n += f["size"].as_u64().unwrap_or(0);
            }
        }
        n += pack["installed_bytes"].as_u64().unwrap_or(0);
    }
    if let Some(model) = model {
        if !have_files.contains(model["file"].as_str().unwrap_or("")) {
            n += model["size"].as_u64().unwrap_or(0);
        }
    }
    n
}

// --------------------------------------------------------------------------------------------------------- switching --
pub fn write_json(path: &Path, d: &Value) -> std::io::Result<()> {
    let tmp = PathBuf::from(format!("{}.tmp", path.display()));
    fs::write(&tmp, serde_json::to_string_pretty(d).unwrap_or_default())?;
    fs::rename(tmp, path)
}

pub fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

pub fn today() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (y, m, d) = civil_from_days(secs.div_euclid(86400));
    format!("{y:04}{m:02}{d:02}")
}

pub fn now_secs() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

pub fn old_name(target: &Path) -> PathBuf {
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let base = target.with_file_name(format!("{name}.old-{}", today()));
    let mut p = base.clone();
    let mut i = 2;
    while p.exists() {
        p = target.with_file_name(format!("{}-{i}", base.file_name().unwrap().to_string_lossy()));
        i += 1;
    }
    p
}

/// Writes what the switch will do to `pend`, a file in the app's own settings folder (never the work folder), for
/// apply_pending. Nothing is renamed yet.
pub fn stage_switch(pend: &Path, new_lib: &Path, target: &Path, model_file: Option<&str>, kind: Option<&str>, model_sha256: Option<&str>) -> std::io::Result<Value> {
    let mut d = json!({"new": new_lib.to_string_lossy(), "target": target.to_string_lossy(), "model_file": model_file, "written": now_secs()});
    if let Some(kind) = kind {
        d["kind"] = json!(kind);
    }
    if let Some(sha) = model_sha256 {
        d["model_sha256"] = json!(sha); // the digest setup matched, for the local AI after the switch
    }
    if let Some(parent) = pend.parent() {
        fs::create_dir_all(parent)?;
    }
    write_json(pend, &d)?;
    Ok(d)
}

/// What belongs to the person, not the pack, goes with them: the model files, address indexes, and the AI's settings.
pub fn carry_over(old: &Path, new: &Path) -> Vec<PathBuf> {
    let mut moved = Vec::new();
    let src = old.join("models");
    if src.is_dir() {
        let _ = fs::create_dir_all(new.join("models"));
        for f in fs::read_dir(&src).into_iter().flatten().flatten() {
            let dest = new.join("models").join(f.file_name());
            if f.path().is_file() && !dest.exists() && fs::rename(f.path(), &dest).is_ok() {
                moved.push(dest);
            }
        }
    }
    let addresses = old.join("derived").join("geo").join("addresses");
    if addresses.is_dir() {
        let dest_dir = new.join("derived").join("geo").join("addresses");
        let _ = fs::create_dir_all(&dest_dir);
        for f in fs::read_dir(&addresses).into_iter().flatten().flatten() {
            let p = f.path();
            let ok_ext = matches!(p.extension().and_then(|e| e.to_str()), Some("sqlite") | Some("json"));
            let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let dest = dest_dir.join(f.file_name());
            if p.is_file() && ok_ext && regions::is_code(&stem) && !dest.exists() && fs::rename(&p, &dest).is_ok() {
                moved.push(dest);
            }
        }
    }
    let state = old.join("derived").join("llm").join("state.json");
    let new_state = new.join("derived").join("llm").join("state.json");
    if state.is_file() && !new_state.exists() {
        let _ = fs::create_dir_all(new_state.parent().unwrap());
        let _ = fs::copy(&state, &new_state);
    }
    moved
}

/// The renames. Returns the old library's new name, or None when there was none. Refused leaves things as they were.
pub fn switch(new_lib: &Path, target: &Path) -> Result<Option<PathBuf>, SetupError> {
    if !is_library(new_lib) {
        return Err(activate("The new library is not complete."));
    }
    let mut old = None;
    if target.exists() {
        let o = old_name(target);
        if is_library(target) {
            carry_over(target, new_lib);
        }
        fs::rename(target, &o).map_err(|e| activate(format!("Lastbook Europe could not move the old library aside ({e}). Close anything using it, then try again.")))?;
        old = Some(o);
    }
    if let Err(e) = fs::rename(new_lib, target) {
        if let Some(o) = &old {
            let _ = fs::rename(o, target);
        }
        return Err(activate(format!("Lastbook Europe could not put the new library in place ({e}).")));
    }
    Ok(old)
}

// ------------------------------------------------------------------------------------------------------- region maps --
/// The map set's map-set.json ({name, files: {path: bytes}}), or None.
pub fn map_set(lib: &Path) -> Option<Value> {
    read_json(&lib.join("map-set.json")).filter(|d| d["files"].is_object())
}

/// 'base', 'europe' (a pack with every region's detail) or a region code: from map-set.json, else pack.json, else 'base'.
pub fn map_name(lib: &Path) -> String {
    if let Some(name) = map_set(lib).and_then(|d| d["name"].as_str().filter(|n| !n.is_empty()).map(str::to_string)) {
        return name;
    }
    read_json(&lib.join("pack.json")).and_then(|d| d["map"].as_str().filter(|m| !m.is_empty()).map(str::to_string)).unwrap_or_else(|| "base".into())
}

/// Every file map-set.json lists is there at its size, and each is a map file.
pub fn check_map(lib: &Path) -> Result<Value, SetupError> {
    let d = map_set(lib).ok_or_else(|| refused("The map has no map-set.json in it."))?;
    for (rel, size) in d["files"].as_object().unwrap() {
        if !map_member(rel, false) || rel.split('/').any(|p| p == "..") {
            return Err(refused(format!("The map lists a file that is not part of a map ({}).", short(rel))));
        }
        let p = lib.join(rel);
        if !p.is_file() || fs::metadata(&p).map(|m| m.len()).ok() != size.as_u64() {
            return Err(refused(format!("The map’s {rel} is missing or the wrong size.")));
        }
    }
    Ok(d)
}

/// The files of a library's map set as they are on disk, relative to it, with SQLite's side files and map-set.json.
pub fn map_files(lib: &Path) -> Vec<String> {
    let mut rels: BTreeSet<String> = BTreeSet::new();
    if let Some(d) = map_set(lib) {
        rels.extend(d["files"].as_object().unwrap().keys().filter(|r| map_member(r, false) && !r.split('/').any(|p| p == "..")).cloned());
    }
    let geo = lib.join("derived").join("geo");
    for f in fs::read_dir(&geo).into_iter().flatten().flatten() {
        let rel = format!("derived/geo/{}", f.file_name().to_string_lossy());
        if f.path().is_file() && map_member(&rel, false) {
            rels.insert(rel);
        }
    }
    rels.extend(MAP_FIXED.iter().filter(|r| lib.join(r).is_file()).map(|r| r.to_string()));
    let mut out = BTreeSet::new();
    for r in rels {
        let sides: &[&str] = if r.ends_with(".sqlite") { SQLITE_SIDES } else { &[] };
        for side in std::iter::once("").chain(sides.iter().copied()) {
            let rel = format!("{r}{side}");
            if lib.join(&rel).is_file() {
                out.insert(rel);
            }
        }
    }
    out.into_iter().collect()
}

pub fn map_bytes(lib: &Path) -> u64 {
    map_files(lib).iter().filter_map(|r| fs::metadata(lib.join(r)).ok()).map(|m| m.len()).sum()
}

/// Keep an installed map while refreshing the same library's content.
pub fn copy_map(old: &Path, new: &Path) -> std::io::Result<()> {
    for rel in map_files(new) {
        fs::remove_file(new.join(&rel))?;
    }
    for rel in map_files(old) {
        let dest = new.join(&rel);
        fs::create_dir_all(dest.parent().unwrap())?;
        fs::copy(old.join(&rel), dest)?;
    }
    Ok(())
}

/// Puts the map set in `new` into `lib`; the old set's files go to old_dir first. Renames only; on any failure what was
/// moved is put back.
pub fn swap_map(lib: &Path, new: &Path, old_dir: &Path) -> Result<(), SetupError> {
    check_map(new)?;
    let (mut moved_out, mut moved_in) = (Vec::new(), Vec::new());
    let result = (|| -> std::io::Result<()> {
        for rel in map_files(lib) {
            let dest = old_dir.join(&rel);
            fs::create_dir_all(dest.parent().unwrap())?;
            fs::rename(lib.join(&rel), dest)?;
            moved_out.push(rel);
        }
        for rel in map_files(new) {
            let dest = lib.join(&rel);
            fs::create_dir_all(dest.parent().unwrap())?;
            fs::rename(new.join(&rel), dest)?;
            moved_in.push(rel);
        }
        Ok(())
    })();
    if let Err(e) = result {
        for rel in &moved_in {
            let _ = fs::rename(lib.join(rel), new.join(rel));
        }
        for rel in &moved_out {
            let _ = fs::rename(old_dir.join(rel), lib.join(rel));
        }
        return Err(activate(format!("Lastbook Europe could not put the new map in place ({e}).")));
    }
    Ok(())
}

/// A pack being installed with a region map: the map's files replace the pack's own map set inside the staged library.
pub fn replace_map_in(staged: &Path, new: &Path) -> Result<(), SetupError> {
    check_map(new)?;
    for rel in map_files(staged) {
        fs::remove_file(staged.join(rel)).map_err(io_error)?;
    }
    for rel in map_files(new) {
        let dest = staged.join(&rel);
        fs::create_dir_all(dest.parent().unwrap()).map_err(io_error)?;
        fs::rename(new.join(&rel), dest).map_err(io_error)?;
    }
    Ok(())
}

fn real(p: &Path) -> String {
    let r = p.canonicalize().unwrap_or_else(|_| {
        // a path that does not exist (yet): its parent's real path and its own name
        match (p.parent().and_then(|q| q.canonicalize().ok()), p.file_name()) {
            (Some(parent), Some(name)) => parent.join(name),
            _ => p.to_path_buf(),
        }
    });
    let s = r.to_string_lossy().into_owned();
    if cfg!(windows) || cfg!(target_os = "macos") {
        s.to_lowercase()
    } else {
        s
    }
}

pub fn same_path(a: &Path, b: &Path) -> bool {
    real(a) == real(b)
}

fn apply_map(work: &Path, pend: &Path, mut d: Value, target: &Path, mut note: Map<String, Value>) -> (Option<PathBuf>, Option<Map<String, Value>>) {
    let old_dir = work.join("map-old");
    note.insert("map_old".into(), json!(old_dir.to_string_lossy()));
    if !d["switched"].as_bool().unwrap_or(false) {
        if let Err(e) = swap_map(target, Path::new(d["new"].as_str().unwrap_or("")), &old_dir) {
            eprintln!("setup: map switch failed: {}", e.message());
            let _ = fs::remove_file(pend);
            note.insert("error".into(), json!(e.message()));
            return (is_library(target).then(|| target.to_path_buf()), Some(note));
        }
        d["switched"] = json!(true);
        let _ = write_json(pend, &d);
    }
    let ok = check_map(target).is_ok() && is_library(target);
    if !ok && old_dir.is_dir() {
        let broken = work.join("map-failed");
        let _ = fs::remove_dir_all(&broken);
        for rel in map_files(target) {
            let dest = broken.join(&rel);
            let _ = fs::create_dir_all(dest.parent().unwrap());
            let _ = fs::rename(target.join(&rel), dest);
        }
        for f in walk(&old_dir) {
            if let Ok(rel) = f.strip_prefix(&old_dir) {
                let dest = target.join(rel);
                let _ = fs::create_dir_all(dest.parent().unwrap());
                let _ = fs::rename(&f, dest);
            }
        }
        note.insert("error".into(), json!("The new map did not open, so Lastbook Europe put the old one back."));
    }
    (is_library(target).then(|| target.to_path_buf()), Some(note))
}

pub fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(p),
                Ok(t) if t.is_file() => out.push(p),
                _ => {}
            }
        }
    }
    out
}

pub fn tree_size(p: &Path) -> u64 {
    walk(p).iter().filter_map(|f| fs::metadata(f).ok()).map(|m| m.len()).sum()
}

/// Only what setup itself stages: the work folder's own library, its own staged copy, and a LastbookData.old-* beside it.
fn pending_ok(d: &Value, work: &Path) -> bool {
    let (Some(target_s), Some(new_s)) = (d["target"].as_str(), d["new"].as_str()) else { return false };
    let work_name = work.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let Some(lib_name) = work_name.strip_suffix(".setup").filter(|n| !n.is_empty()) else { return false };
    let target = work.with_file_name(lib_name);
    let kind = d["kind"].as_str();
    if !(d["kind"].is_null() || kind == Some("map")) {
        return false;
    }
    let new = work.join(if kind == Some("map") { "unpack-map" } else { "unpack" }).join(DATA_NAME);
    if !same_path(Path::new(target_s), &target) || !same_path(Path::new(new_s), &new) {
        return false;
    }
    if !d["old"].is_null() {
        let Some(old) = d["old"].as_str() else { return false };
        let old = Path::new(old);
        let starts = old.file_name().map(|n| n.to_string_lossy().starts_with(&format!("{lib_name}.old-"))).unwrap_or(false);
        if !(old.parent().is_some_and(|p| same_path(p, target.parent().unwrap_or(Path::new("")))) && starts) {
            return false;
        }
    }
    if !d["model_sha256"].is_null() {
        let ok = d["model_sha256"].as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)));
        if !ok {
            return false;
        }
    }
    if !d["model_file"].is_null() {
        let Some(mf) = d["model_file"].as_str() else { return false };
        if !Path::new(mf).parent().is_some_and(|p| same_path(p, &target.join("models"))) {
            return false;
        }
    }
    true
}

/// Runs at startup, before the engine opens anything: the library to open (or None) and a note. A staged switch is made
/// now; if the new library then does not open, the old one is put back. A file that is not exactly what setup stages for
/// this work folder is removed, not obeyed.
pub fn apply_pending(pend: &Path, work: &Path) -> (Option<PathBuf>, Option<Map<String, Value>>) {
    let Some(mut d) = read_json(pend) else { return (None, None) };
    if !pending_ok(&d, work) {
        eprintln!("setup: a pending switch that does not match its work folder was ignored");
        let _ = fs::remove_file(pend);
        return (None, None);
    }
    let target = PathBuf::from(d["target"].as_str().unwrap());
    let mut note = Map::new();
    note.insert("target".into(), json!(target.to_string_lossy()));
    note.insert("old".into(), Value::Null);
    note.insert("model_file".into(), d["model_file"].clone());
    note.insert("model_sha256".into(), d["model_sha256"].clone());
    if d["kind"].as_str() == Some("map") {
        return apply_map(work, pend, d, &target, note);
    }
    let lib_ok = if d["switched"].as_bool().unwrap_or(false) {
        is_library(&target)
    } else {
        match switch(Path::new(d["new"].as_str().unwrap()), &target) {
            Ok(old) => {
                d["switched"] = json!(true);
                d["old"] = old.map(|o| json!(o.to_string_lossy())).unwrap_or(Value::Null);
                let _ = write_json(pend, &d);
                is_library(&target)
            }
            Err(e) => {
                eprintln!("setup: switch failed: {}", e.message());
                let _ = fs::remove_file(pend);
                note.insert("error".into(), json!(e.message()));
                return (is_library(&target).then(|| target.clone()), Some(note));
            }
        }
    };
    note.insert("old".into(), d["old"].clone());
    if !lib_ok {
        if let Some(old) = d["old"].as_str().map(PathBuf::from).filter(|o| o.exists()) {
            let broken = target.with_file_name(format!("{}.failed", target.file_name().unwrap().to_string_lossy()));
            let _ = fs::rename(&target, &broken);
            let _ = fs::rename(&old, &target);
            carry_over(&broken, &target); // the model file, moved into the new one, goes back too
            note.insert("error".into(), json!("The new library did not open, so Lastbook Europe put the old one back."));
            note.insert("old".into(), Value::Null);
        }
    }
    (is_library(&target).then(|| target.clone()), Some(note))
}

/// The new library opened: the switch is done. The old one stays until the person deletes it from setup.
pub fn confirm(pend: &Path) {
    let _ = fs::remove_file(pend);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use zip::write::SimpleFileOptions;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lb-install-{name}-{}-{}", std::process::id(), now_secs()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn make_zip(path: &Path, files: &[(&str, &[u8])]) -> u64 {
        let mut z = zip::ZipWriter::new(File::create(path).unwrap());
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, data) in files {
            if name.ends_with('/') {
                z.add_directory(*name, opts).unwrap();
            } else {
                z.start_file(*name, opts).unwrap();
                z.write_all(data).unwrap();
            }
        }
        z.finish().unwrap();
        files.iter().map(|(_, d)| d.len() as u64).sum()
    }

    fn pack(dir: &Path) -> PathBuf {
        let p = dir.join("pack.zip");
        make_zip(&p, &[("LastbookData/", b""), ("LastbookData/library-data/catalog.json", b"{}"), ("LastbookData/pack.json", b"{\"name\":\"small\"}"),
                       ("LastbookData/library-data/big.bin", &vec![7u8; 300_000])]);
        p
    }

    #[test]
    fn unpacks_and_resumes() {
        let d = tmp("unpack");
        let p = pack(&d);
        let c = Control::new();
        let mut last = 0;
        let lib = unpack(&p, &d.join("out"), Some(400_000), &c, &mut |done, _| last = done, None, None).unwrap();
        assert!(is_library(&lib));
        assert_eq!(last, 300_000 + 2 + 16);
        let lib2 = unpack(&p, &d.join("out"), Some(400_000), &c, &mut |_, _| {}, None, None).unwrap(); // whole files are kept
        assert_eq!(lib, lib2);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn refuses_unsafe_zips() {
        let d = tmp("refuse");
        let cases: Vec<(&str, Vec<(&str, &[u8])>)> = vec![
            ("outside", vec![("library-data/catalog.json", b"{}")]),
            ("dotdot", vec![("LastbookData/../x", b"x")]),
            ("absolute", vec![("/LastbookData/x", b"x")]),
            ("drive", vec![("C:/LastbookData/x", b"x")]),
            ("backslash", vec![("LastbookData\\x", b"x")]),
            ("twice", vec![("LastbookData/a.txt", b"1"), ("LastbookData/A.TXT", b"2")]),
            ("no catalog", vec![("LastbookData/x.txt", b"x")]),
        ];
        for (what, files) in cases {
            let p = d.join("z.zip");
            make_zip(&p, &files);
            let r = unpack(&p, &d.join("out"), Some(1_000_000), &Control::new(), &mut |_, _| {}, None, None);
            assert!(matches!(r, Err(SetupError::Refused { .. })), "{what}");
        }
        let big = d.join("big.zip");
        let two_mib = vec![1u8; 2 * 1024 * 1024];
        make_zip(&big, &[("LastbookData/library-data/catalog.json", b"{}"), ("LastbookData/big.bin", &two_mib)]);
        assert!(matches!(check_zip(&big, Some(1000), None), Err(SetupError::Refused { .. })), "larger than installed_bytes");
        assert!(check_zip(&big, Some(2 * 1024 * 1024), None).is_ok());
        let p = pack(&d);
        let c = Control::new();
        c.stop("cancelled");
        assert!(matches!(unpack(&p, &d.join("out2"), None, &c, &mut |_, _| {}, None, None), Err(SetupError::Stopped(_))));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn refuses_links() {
        let d = tmp("link");
        let p = d.join("l.zip");
        let mut z = zip::ZipWriter::new(File::create(&p).unwrap());
        z.add_symlink("LastbookData/link", "/etc/passwd", SimpleFileOptions::default()).unwrap();
        z.finish().unwrap();
        assert!(matches!(check_zip(&p, None, None), Err(SetupError::Refused { .. })));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn map_members() {
        for ok in ["map-set.json", "derived/geo/national-status.json", "derived/geo/terrain.sqlite", "derived/geo/places.sqlite"] {
            assert!(map_member(ok, false), "{ok}");
        }
        for bad in ["derived/geo/socal-cells.json", "library-data/catalog.json", concat!("library-data/geography", "/resources/atlas", ".sqlite"), "derived/geo/a/b.sqlite",
                    "derived/geo/.sqlite", "derived/geo/x.sqlite.bak"] {
            assert!(!map_member(bad, false), "{bad}");
        }
        assert!(map_member("derived/geo", true) && !map_member("library-data/documents", true) && !map_member("library-data", true));
    }

    #[test]
    fn switch_and_put_back() {
        let d = tmp("switch");
        let target = d.join(DATA_NAME);
        fs::create_dir_all(target.join("library-data")).unwrap();
        fs::write(target.join("library-data/catalog.json"), b"old").unwrap();
        fs::create_dir_all(target.join("models")).unwrap();
        fs::write(target.join("models/m.gguf"), b"model").unwrap();
        fs::create_dir_all(target.join("derived/geo/addresses")).unwrap();
        fs::write(target.join("derived/geo/addresses/DE-HB.sqlite"), b"idx").unwrap();
        fs::write(target.join("derived/geo/addresses/notes.sqlite"), b"no").unwrap();
        let work = d.join("LastbookData.setup");
        let new = work.join("unpack").join(DATA_NAME);
        fs::create_dir_all(new.join("library-data")).unwrap();
        fs::write(new.join("library-data/catalog.json"), b"new").unwrap();
        let pend = d.join("settings/pending.json");
        stage_switch(&pend, &new, &target, None, None, None).unwrap();
        let (lib, note) = apply_pending(&pend, &work);
        assert_eq!(lib.as_deref(), Some(target.as_path()));
        assert_eq!(fs::read(target.join("library-data/catalog.json")).unwrap(), b"new");
        assert_eq!(fs::read(target.join("models/m.gguf")).unwrap(), b"model");
        assert!(target.join("derived/geo/addresses/DE-HB.sqlite").is_file());
        assert!(!target.join("derived/geo/addresses/notes.sqlite").exists());
        let old = note.unwrap()["old"].as_str().unwrap().to_string();
        assert!(Path::new(&old).file_name().unwrap().to_string_lossy().starts_with("LastbookData.old-"));
        // a pending file naming somewhere else is not obeyed
        fs::write(&pend, serde_json::to_vec(&json!({"new": "/tmp/elsewhere", "target": target})).unwrap()).unwrap();
        assert!(apply_pending(&pend, &work).0.is_none());
        assert!(!pend.exists());
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn dates() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(days_from_civil(2026, 11, 17)), (2026, 11, 17));
        assert_eq!(civil_from_days(days_from_civil(2000, 2, 29)), (2000, 2, 29));
        assert_eq!(today().len(), 8);
        let _ = Cursor::new(Vec::<u8>::new());
    }
}
