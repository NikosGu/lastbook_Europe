//! Lastbook Europe's build tools. They run on a build machine, read public data and write what the app ships;
//! they are never part of the app.
//!
//! The map's files (docs/MAPS.md) and the address index (docs/ADDRESSES.md) are built by the `map-data` command
//! (src/bin/map-data.rs) from Geofabrik's GeoPackage and OSM PBF of a region, Copernicus DEM GLO-30 and Natural Earth.

pub mod addresses;
pub mod admin;
pub mod buildings;
pub mod detail;
pub mod geo;
pub mod gpkg;
pub mod library;
pub mod osm;
pub mod places;
pub mod terrain;
pub mod water;
