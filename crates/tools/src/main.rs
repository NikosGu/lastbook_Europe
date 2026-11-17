//! Lastbook Europe's build tools, one command with a subcommand for each job.
//!
//!   cargo run -p lastbook-tools -- <subcommand> [options]
//!
//! Each subcommand lives in its own module and prints its own usage with --help.

use lastbook_tools::{library, map};

const USAGE: &str = "lastbook-tools <subcommand> [options]

  build-library   a LastbookData folder from documents and their sources list
  build-pack      a LastbookData folder as Lastbook-<id>.zip, with its manifest entry
  build-map       a region's map files and address index, from OpenStreetMap, Copernicus DEM and Natural Earth
  pack-map        a region's map files as Lastbook-Map-<region>.zip, with its manifest entry
  pack-addresses  a region's address index as Lastbook-Addresses-<region>.zip, with its manifest entry
  build-terrain   a region's elevation tiles alone, from Copernicus DEM

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
        _ => {
            eprintln!("{USAGE}");
            2
        }
    };
    std::process::exit(code);
}
