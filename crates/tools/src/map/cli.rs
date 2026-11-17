//! The map's subcommands of lastbook-tools. Each prints what it made as JSON: build-map the counts, the packers the
//! entry for the manifest (manifest-v1.json, under `maps` or `address_indexes`).
//!
//!   lastbook-tools build-map --region LI --gpkg derived/sources/LI/liechtenstein-free.gpkg.zip \
//!       --pbf derived/sources/LI/liechtenstein.osm.pbf \
//!       --dem derived/sources/dem/Copernicus_DSM_COG_10_N47_00_E009_00_DEM.tif \
//!       --dem derived/sources/dem/Copernicus_DSM_COG_10_N46_00_E009_00_DEM.tif --ne derived/sources/ne --out derived/maps/LI
//!   lastbook-tools pack-map --map derived/maps/LI/map --out derived/packs
//!   lastbook-tools pack-addresses --index derived/maps/LI/addresses/LI.sqlite --out derived/packs
//!   lastbook-tools build-terrain --region LI --dem FILE [--dem FILE]... --out terrain.sqlite

use super::{check, mapset, pack, sources, terrain};
use lastbook_engine::{dist, regions};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const BUILD_MAP: &str = "lastbook-tools build-map --region REGION --gpkg FILE... --pbf FILE... [--neighbour-gpkg FILE]...
          [--neighbour-pbf FILE]... [--dem FILE]... [--ne FOLDER] --out FOLDER

  --region          the region's code (crates/engine/src/regions.rs), as LI or DE-HB
  --gpkg            Geofabrik's free GeoPackage of the region (<region>-latest-free.gpkg.zip); several for a region
                    Geofabrik splits (Grand Est: Alsace, Champagne-Ardenne, Lorraine)
  --pbf             Geofabrik's PBF of the region (<region>-latest.osm.pbf), likewise
  --neighbour-gpkg  a neighbour's GeoPackage, read only inside the region's boxes (Switzerland's for Liechtenstein's
                    bank of the Rhine)
  --neighbour-pbf   a neighbour's PBF, read the same way
  --dem             a Copernicus DEM GLO-30 GeoTIFF; give each one the region's boxes touch (none: no elevation)
  --ne              the folder holding Natural Earth's six zips (none: no zoomed-out base)
  --out             the folder to write map/ (the map set) and addresses/<REGION>.sqlite into
Prints what it made.";

const PACK_MAP: &str = "lastbook-tools pack-map --map FOLDER --out FOLDER [--base URL]

  --map   a map set made by build-map (its map/ folder, which holds map-set.json)
  --out   the folder to write Lastbook-Map-<REGION>.zip into
  --base  the download site's address; the zip is published in its maps/ folder (default: the download site's)
Prints the map's entry for the manifest (manifest-v1.json, \"maps\").";

const PACK_ADDRESSES: &str = "lastbook-tools pack-addresses --index FILE --out FOLDER [--base URL]

  --index  an address index made by build-map (addresses/<REGION>.sqlite)
  --out    the folder to write Lastbook-Addresses-<REGION>.zip into
  --base   the download site's address; the zip is published in its addresses/ folder (default: the download site's)
Prints the index's entry for the manifest (manifest-v1.json, \"address_indexes\").";

const BUILD_TERRAIN: &str = "lastbook-tools build-terrain --region REGION --dem FILE [--dem FILE]... --out FILE

  --region  the region's code
  --dem     a Copernicus DEM GLO-30 GeoTIFF; give each one the region's boxes touch
  --out     the terrain.sqlite to write
Prints the number of tiles written, and of those left out because the DEM does not cover them whole.";

const REGION_SOURCES: &str = "lastbook-tools region-sources --region REGION [--neighbours yes]

  --region      the region's code
  --neighbours  yes: also the extracts of the regions next to it, which build-map reads only inside its boxes
Prints what to download for the region (crates/tools/data/region-sources.json): its Geofabrik extracts, its
neighbours', the Copernicus DEM tiles and Natural Earth's zips, with the town and the address count its check expects.";

const CHECK_MAP: &str = "lastbook-tools check-map --region REGION --packs FOLDER [--town NAME] [--min-addresses N] [--work FOLDER]

  --region         the region's code
  --packs          the folder holding Lastbook-Map-<REGION>.zip and Lastbook-Addresses-<REGION>.zip, with their entries
                   saved as map-entry-<REGION>.json and addresses-entry-<REGION>.json
  --town           the town the place search must find inside the region's boxes (default: the sources' table's)
  --min-addresses  the least the address index may hold (default: the table's, else 100)
  --work           where to unpack them (default: <packs>/check, removed after)
Prints every check, passed or not; fails if one does.";

/// A subcommand's options: `--name value` pairs, a name given once or, as --dem, more than once.
struct Opts(Vec<(String, String)>);

impl Opts {
    fn parse(args: &[String]) -> Result<Opts, String> {
        let mut out = Vec::new();
        let mut it = args.iter();
        while let Some(a) = it.next() {
            let Some(name) = a.strip_prefix("--") else { return Err(format!("unexpected argument {a:?}")) };
            let Some(v) = it.next() else { return Err(format!("--{name} needs a value")) };
            out.push((name.to_string(), v.clone()));
        }
        Ok(Opts(out))
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.0.iter().find(|o| o.0 == name).map(|o| o.1.as_str())
    }

    fn need(&self, name: &str) -> Result<&str, String> {
        self.get(name).ok_or_else(|| format!("--{name} is needed"))
    }

    fn all(&self, name: &str) -> Vec<PathBuf> {
        self.0.iter().filter(|o| o.0 == name).map(|o| PathBuf::from(&o.1)).collect()
    }

    fn base(&self) -> String {
        self.get("base").map(|b| b.trim_end_matches('/').to_string()).unwrap_or_else(dist::download_base)
    }
}

/// Runs a subcommand: its help on --help, else what it made as JSON, or the reason it could not and its help.
fn run(name: &str, help: &str, args: &[String], job: impl FnOnce(&Opts) -> Result<Value, String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{help}");
        return 0;
    }
    match Opts::parse(args).and_then(|o| job(&o)) {
        Ok(v) => {
            println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            0
        }
        Err(e) => {
            eprintln!("{name}: {e}\n\n{help}");
            1
        }
    }
}

pub fn build_map(args: &[String]) -> i32 {
    run("build-map", BUILD_MAP, args, |o| {
        o.need("gpkg")?;
        o.need("pbf")?;
        let sources = mapset::Sources { gpkg: o.all("gpkg"), pbf: o.all("pbf"), neighbour_gpkg: o.all("neighbour-gpkg"), neighbour_pbf: o.all("neighbour-pbf"),
                                        dem: o.all("dem"), ne: o.get("ne").map(PathBuf::from) };
        mapset::build(o.need("region")?, &sources, Path::new(o.need("out")?))
    })
}

pub fn pack_map(args: &[String]) -> i32 {
    run("pack-map", PACK_MAP, args, |o| pack::map(Path::new(o.need("map")?), Path::new(o.need("out")?), &o.base()))
}

pub fn pack_addresses(args: &[String]) -> i32 {
    run("pack-addresses", PACK_ADDRESSES, args, |o| pack::addresses(Path::new(o.need("index")?), Path::new(o.need("out")?), &o.base()))
}

pub fn build_terrain(args: &[String]) -> i32 {
    run("build-terrain", BUILD_TERRAIN, args, |o| {
        let region = o.need("region")?;
        let boxes = regions::boxes(region).ok_or_else(|| format!("{region} is not a region"))?;
        let dems = o.all("dem").iter().map(|p| terrain::Dem::read(p)).collect::<Result<Vec<_>, _>>()?;
        if dems.is_empty() {
            return Err("--dem is needed".into());
        }
        let n = terrain::write(Path::new(o.need("out")?), &dems, boxes, terrain::ZOOMS, mapset::TERRAIN_SOURCE, mapset::TERRAIN_LICENCE)
            .map_err(|e| e.to_string())?;
        Ok(json!({"tiles": n.tiles, "not_covered": n.skipped}))
    })
}

pub fn region_sources(args: &[String]) -> i32 {
    run("region-sources", REGION_SOURCES, args, |o| sources::sources(o.need("region")?, o.get("neighbours") == Some("yes")))
}

pub fn check_map(args: &[String]) -> i32 {
    run("check-map", CHECK_MAP, args, |o| {
        let region = o.need("region")?;
        let entry = sources::table().remove(region);
        let town = o.get("town").map(str::to_string).or_else(|| entry.as_ref().map(|e| e.capital.clone())).ok_or("--town is needed for a region not in the table")?;
        let least = match o.get("min-addresses") {
            Some(n) => n.parse().map_err(|_| format!("--min-addresses {n}: not a number"))?,
            None => entry.map(|e| e.min_addresses).unwrap_or(sources::MIN_ADDRESSES),
        };
        let packs = Path::new(o.need("packs")?);
        let work = o.get("work").map(PathBuf::from).unwrap_or_else(|| packs.join("check"));
        let report = check::check(region, packs, &work, &town, least);
        let _ = std::fs::remove_dir_all(&work);
        let report = report?;
        if report["ok"] != true {
            println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
            return Err("a check failed".into());
        }
        Ok(report)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn options_given_twice_kept_and_mistakes_refused() {
        let o = Opts::parse(&args(&["--dem", "n47.tif", "--dem", "n46.tif", "--region", "LI"])).unwrap();
        assert_eq!(o.all("dem"), vec![PathBuf::from("n47.tif"), PathBuf::from("n46.tif")]);
        assert_eq!(o.need("region"), Ok("LI"));
        assert_eq!(o.need("out"), Err("--out is needed".to_string()));
        assert_eq!(o.base(), dist::download_base());
        assert!(Opts::parse(&args(&["LI"])).is_err(), "no bare arguments");
        assert!(Opts::parse(&args(&["--out"])).is_err(), "an option without its value");
        assert_eq!(build_terrain(&args(&["--region", "XX", "--out", "t.sqlite"])), 1, "not a region");
        assert_eq!(pack_map(&args(&["--help"])), 0);
    }
}
