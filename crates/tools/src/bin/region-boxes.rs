//! Writes crates/engine/src/regions_boxes.rs from Eurostat's NUTS 2024 boundaries.
//!
//!   cargo run -p lastbook-tools --bin region-boxes -- NUTS_RG_03M_2024_4326_LEVL_0.geojson \
//!       NUTS_RG_03M_2024_4326_LEVL_1.geojson NUTS_RG_03M_2024_4326_LEVL_2.geojson [--check]
//!
//! The three files are GISCO's NUTS 2024 regions at 1:3 million in WGS 84, about 9 MB together, from
//! https://gisco-services.ec.europa.eu/distribution/v2/nuts/geojson/ (administrative boundaries: © EuroGeographics).
//! Only each unit's extent is read; no boundary is copied into the app.
//!
//! A region's box is the extent of the NUTS units the registry names for it (crates/engine/src/regions_data.rs), widened
//! by regions::MARGIN and rounded outward to 0.01 degrees. The tool also reports a box whose land comes in parts far
//! apart (more than GAP degrees between them): give such a region one unit per part instead. --check only reports.

use lastbook_engine::regions::{MARGIN, REGIONS};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

const GAP: f64 = 2.0;
type Extent = [f64; 4];

fn extent_of_rings(rings: &Value) -> Option<Extent> {
    let mut e = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for ring in rings.as_array()? {
        for p in ring.as_array()? {
            let (x, y) = (p.get(0)?.as_f64()?, p.get(1)?.as_f64()?);
            e = [e[0].min(x), e[1].min(y), e[2].max(x), e[3].max(y)];
        }
    }
    Some(e)
}

fn read_units(paths: &[PathBuf]) -> HashMap<String, Vec<Extent>> {
    let mut units = HashMap::new();
    for path in paths {
        let data: Value = serde_json::from_slice(&std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))).expect("GeoJSON");
        for f in data["features"].as_array().expect("features") {
            let id = f["properties"]["NUTS_ID"].as_str().expect("NUTS_ID").to_string();
            let g = &f["geometry"];
            let polys: Vec<&Value> = match g["type"].as_str() {
                Some("Polygon") => vec![&g["coordinates"]],
                Some("MultiPolygon") => g["coordinates"].as_array().expect("coordinates").iter().collect(),
                other => panic!("unexpected geometry {other:?}"),
            };
            units.insert(id, polys.into_iter().filter_map(extent_of_rings).collect());
        }
    }
    units
}

/// Groups of extents within GAP degrees of each other: one group is one piece of land.
fn parts_apart(extents: &[Extent]) -> Vec<Extent> {
    let mut groups: Vec<Vec<Extent>> = Vec::new();
    for e in extents {
        let near = |o: &Extent| e[0] - GAP <= o[2] && o[0] - GAP <= e[2] && e[1] - GAP <= o[3] && o[1] - GAP <= e[3];
        let (joined, rest): (Vec<_>, Vec<_>) = groups.into_iter().partition(|g| g.iter().any(near));
        let mut merged = vec![*e];
        merged.extend(joined.into_iter().flatten());
        groups = rest;
        groups.push(merged);
    }
    groups.iter().map(|g| union(g)).collect()
}

fn union(es: &[Extent]) -> Extent {
    es.iter().fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])])
}

fn widen(e: Extent) -> Extent {
    let down = |v: f64| ((v - MARGIN) * 100.0).floor() / 100.0;
    let up = |v: f64| ((v + MARGIN) * 100.0).ceil() / 100.0;
    [down(e[0]), down(e[1]), up(e[2]), up(e[3])]
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let check = args.iter().any(|a| a == "--check");
    let files: Vec<PathBuf> = args.iter().filter(|a| !a.starts_with("--")).map(PathBuf::from).collect();
    if files.len() != 3 {
        eprintln!("give the three NUTS files, levels 0, 1 and 2");
        std::process::exit(2);
    }
    let units = read_units(&files);
    let mut out = String::from(
        "// Written by `cargo run -p lastbook-tools --bin region-boxes` from Eurostat NUTS 2024 (1:3 million, levels 0 to 2): do not edit by hand.\n\
         // Each region: its boxes (west, south, east, north), the NUTS extent widened by MARGIN and rounded outward to 0.01 degrees.\n\
         pub const BOXES: &[(&str, &[[f64; 4]])] = &[\n",
    );
    for r in REGIONS {
        let mut boxes = Vec::new();
        for part in r.nuts {
            let mut extents = Vec::new();
            for id in part.split('+') {
                extents.extend(units.get(id).unwrap_or_else(|| panic!("{}: NUTS unit {id} is not in the files given", r.code)));
            }
            let apart = parts_apart(&extents);
            if apart.len() > 1 {
                eprintln!("note: {} ({part}): {} parts far apart: {:?}", r.code, apart.len(), apart);
            }
            boxes.push(widen(union(&extents)));
        }
        let list: Vec<String> = boxes.iter().map(|b| format!("[{:.2}, {:.2}, {:.2}, {:.2}]", b[0], b[1], b[2], b[3])).collect();
        out.push_str(&format!("    (\"{}\", &[{}]),\n", r.code, list.join(", ")));
    }
    out.push_str("];\n");
    if check {
        return;
    }
    let target = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../engine/src/regions_boxes.rs");
    std::fs::write(&target, out).expect("write regions_boxes.rs");
    println!("{}: {} regions", target.display(), REGIONS.len());
}
