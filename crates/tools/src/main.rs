//! Lastbook Europe's build tools, one command with a subcommand for each job.
//!
//!   cargo run -p lastbook-tools -- <subcommand> [options]
//!
//! Each subcommand lives in its own module and prints its own usage with --help.

use lastbook_tools::{library, map, release};

const USAGE: &str = "lastbook-tools <subcommand> [options]

  build-library   a LastbookData folder from documents and their sources list
  build-pack      a LastbookData folder as Lastbook-<id>.zip, with its manifest entry
  build-map       a region's map files and address index, from OpenStreetMap, Copernicus DEM and Natural Earth
  pack-map        a region's map files as Lastbook-Map-<region>.zip, with its manifest entry
  pack-addresses  a region's address index as Lastbook-Addresses-<region>.zip, with its manifest entry
  build-terrain   a region's elevation tiles alone, from Copernicus DEM
  region-sources  what to download for a region: its extracts, its neighbours', the DEM tiles, Natural Earth
  check-map       a region's zips checked as setup takes them and as a person uses them
  build-manifest  the data release's manifest-v1.json: the model catalogue, every region's entries and the starter pack's
  check-roots     every host setup downloads from verified through the app's own roots, and a site outside them refused
  scan-package    a built package looked through for what must not ship

Run a subcommand with --help for its options.";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = args.get(1..).unwrap_or_default();
    let code = match args.first().map(String::as_str) {
        Some("build-library") => library::build::main(rest),
        Some("build-pack") => library::pack::main(rest),
        Some("build-map") => map::cli::build_map(rest),
        Some("pack-map") => map::cli::pack_map(rest),
        Some("pack-addresses") => map::cli::pack_addresses(rest),
        Some("build-terrain") => map::cli::build_terrain(rest),
        Some("region-sources") => map::cli::region_sources(rest),
        Some("check-map") => map::cli::check_map(rest),
        Some("build-manifest") => map::cli::build_manifest(rest),
        Some("check-roots") => release::check_roots(rest),
        Some("scan-package") => release::scan_package(rest),
        _ => {
            eprintln!("{USAGE}");
            2
        }
    };
    std::process::exit(code);
}
