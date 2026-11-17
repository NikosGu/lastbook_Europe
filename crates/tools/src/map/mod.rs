//! The map's data tools: `build-map` makes a region's map files and address index from public data, `pack-map` and
//! `pack-addresses` turn them into the zips setup downloads (cli.rs). What they write is what the engine reads
//! (crates/engine/src/map/; docs/MAPS.md, docs/ADDRESSES.md) and what setup checks and unpacks
//! (crates/engine/src/setup/install.rs). They do the work of the original Lastbook's map builders.
//!
//! Sources, read from local files (fetch them first):
//!   - Geofabrik's free GeoPackage and OSM PBF of the region: https://download.geofabrik.de/europe/ (ODbL 1.0)
//!   - Copernicus DEM GLO-30, one GeoTIFF a degree: https://copernicus-dem-30m.s3.amazonaws.com/
//!     Copernicus_DSM_COG_10_N47_00_E009_00_DEM/Copernicus_DSM_COG_10_N47_00_E009_00_DEM.tif and its neighbours
//!   - Natural Earth 1:10m (public domain): https://naciscdn.org/naturalearth/10m/ (base.rs lists the six zips)

pub mod addresses;
pub mod admin;
pub mod base;
pub mod buildings;
pub mod check;
pub mod cli;
pub mod coast;
pub mod detail;
pub mod geo;
pub mod gpkg;
pub mod mapset;
pub mod osm;
pub mod pack;
pub mod places;
#[cfg(test)]
mod real;
pub mod shp;
pub mod sources;
pub mod terrain;
pub mod water;
