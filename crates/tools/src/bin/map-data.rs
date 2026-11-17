//! Builds a region's map files and address index, and the zips setup installs, from public data.
//!
//!   cargo run --release -p lastbook-tools --bin map-data -- region LI \
//!       --gpkg derived/sources/LI/liechtenstein-free.gpkg.zip --pbf derived/sources/LI/liechtenstein.osm.pbf \
//!       --dem derived/sources/dem/Copernicus_DSM_COG_10_N47_00_E009_00_DEM.tif --dem ...N46... \
//!       --ne derived/sources/ne --out derived/maps/LI
//!   cargo run ... -- pack-map derived/maps/LI/map --out derived/packs [--url-base URL]
//!   cargo run ... -- pack-addresses derived/maps/LI/addresses/LI.sqlite --out derived/packs [--url-base URL]
//!   cargo run ... -- terrain LI --dem FILE... --out terrain.sqlite
//!
//! Sources (crates/tools/src/lib.rs names what reads each):
//!   - Geofabrik's free GeoPackage and OSM PBF of the region: https://download.geofabrik.de/europe/ (ODbL 1.0)
//!   - Copernicus DEM GLO-30, one GeoTIFF a degree: https://copernicus-dem-30m.s3.amazonaws.com/
//!     Copernicus_DSM_COG_10_N47_00_E009_00_DEM/Copernicus_DSM_COG_10_N47_00_E009_00_DEM.tif and its neighbours
//!   - Natural Earth 1:10m (public domain): https://naciscdn.org/naturalearth/10m/ (base.rs lists the six zips)
//! The tool reads only local files; fetch the sources first. `region` prints what it made, the packers print the
//! manifest entry (under `maps` or `address_indexes` in manifest-v1.json).

use lastbook_tools::{mapset, pack, terrain};
use std::path::PathBuf;

fn usage() -> ! {
    eprintln!("usage: map-data region REGION --gpkg FILE --pbf FILE [--dem FILE]... [--ne DIR] --out DIR\n       \
               map-data pack-map MAP-DIR --out DIR [--url-base URL]\n       \
               map-data pack-addresses INDEX --out DIR [--url-base URL]\n       \
               map-data terrain REGION --dem FILE... --out FILE");
    std::process::exit(2)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(cmd), Some(what)) = (args.first(), args.get(1)) else { usage() };
    let mut opts: Vec<(String, String)> = Vec::new();
    let mut i = 2;
    while i < args.len() {
        let (Some(k), Some(v)) = (args[i].strip_prefix("--"), args.get(i + 1)) else { usage() };
        opts.push((k.to_string(), v.clone()));
        i += 2;
    }
    let one = |k: &str| opts.iter().find(|o| o.0 == k).map(|o| PathBuf::from(&o.1));
    let all = |k: &str| opts.iter().filter(|o| o.0 == k).map(|o| PathBuf::from(&o.1)).collect::<Vec<_>>();
    let need = |k: &str| one(k).unwrap_or_else(|| usage());
    let url_base = opts.iter().find(|o| o.0 == "url-base").map(|o| o.1.trim_end_matches('/').to_string()).unwrap_or_else(lastbook_engine::dist::download_base);
    let result = match cmd.as_str() {
        "region" => mapset::build(what, &mapset::Sources { gpkg: need("gpkg"), pbf: need("pbf"), dem: all("dem"), ne: one("ne") }, &need("out")),
        "pack-map" => pack::map(&PathBuf::from(what), &need("out"), &url_base),
        "pack-addresses" => pack::addresses(&PathBuf::from(what), &need("out"), &url_base),
        "terrain" => (|| {
            let boxes = lastbook_engine::regions::boxes(what).ok_or("not a region")?;
            let dems: Vec<terrain::Dem> = all("dem").iter().map(|p| terrain::Dem::read(p)).collect::<Result<_, _>>()?;
            let n = terrain::write(&need("out"), &dems, boxes, terrain::ZOOMS, mapset::TERRAIN_SOURCE, mapset::TERRAIN_LICENCE).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({"tiles": n.tiles, "not_covered": n.skipped}))
        })(),
        _ => usage(),
    };
    match result {
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap()),
        Err(e) => {
            eprintln!("map-data: {e}");
            std::process::exit(1);
        }
    }
}
