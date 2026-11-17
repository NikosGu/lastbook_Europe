//! The region's administrative areas, from the GeoPackage's `gis_osm_adminareas_a_free`: the outline of its own country
//! (or, in a regional extract, of the region), its municipalities (admin level 8) and its districts (level 6).

use crate::map::geo::{self, Part};
use crate::map::gpkg;
use rusqlite::Connection;

/// The columns of `gis_osm_adminareas_a_free` the areas are read with.
pub const ADMIN_COLUMNS: &[&str] = &["fclass", "name"];

#[derive(Debug, Default, Clone)]
pub struct Admin {
    /// The region's country code and the outline its own addresses lie in.
    pub country: Option<(String, Vec<Part>)>,
    pub towns: Vec<(String, Vec<Part>)>,
    pub districts: Vec<(String, Vec<Part>)>,
    /// Districts that are towns themselves, holding no level-8 municipality.
    pub cities: Vec<(String, Vec<Part>)>,
}

impl Admin {
    pub fn load(g: &Connection, country: &str) -> Admin {
        Admin::from_rows(&gpkg::layer(g, "gis_osm_adminareas_a_free", ADMIN_COLUMNS), country)
    }

    /// From `gis_osm_adminareas_a_free`'s rows, read with [`ADMIN_COLUMNS`] (of every extract of the region's own).
    pub fn from_rows(rows: &[gpkg::Row], country: &str) -> Admin {
        let named = |level: &str| -> Vec<(String, Vec<Part>)> {
            rows.iter().filter(|r| r.col(0) == Some(level)).filter_map(|r| Some((r.col(1)?.to_string(), r.parts.clone()))).collect()
        };
        // a country's extract has its national outline; a region's extract (a German state) its own, at level 4. An outline
        // may be in parts (Malta's islands, Bremen and Bremerhaven): all of them count.
        let area = |parts: &[Part]| parts.iter().map(geo::ring_area_m2).sum::<f64>();
        let own = rows.iter().filter(|r| r.col(0) == Some("national")).chain(rows.iter().filter(|r| r.col(0) == Some("admin_level4")))
            .max_by(|a, b| area(&a.parts).total_cmp(&area(&b.parts)));
        let (towns, districts) = (named("admin_level8"), named("admin_level6"));
        // a district with no municipality of its own inside is a town itself: a German city that is its own district
        let cities = districts.iter().filter(|(_, d)| !towns.iter().any(|(_, t)| geo::centroid(t).is_some_and(|(x, y)| geo::contains(d, x, y))))
            .cloned().collect();
        Admin { country: own.map(|r| (country.to_string(), r.parts.clone())), towns, districts, cities }
    }

    /// The municipality a point lies in: level 8, or a city that is its own district (Bremen, Bremerhaven).
    pub fn town_at(&self, lon: f64, lat: f64) -> Option<&str> {
        self.towns.iter().chain(&self.cities).find(|(_, rings)| geo::contains(rings, lon, lat)).map(|(n, _)| n.as_str())
    }

    pub fn district_at(&self, lon: f64, lat: f64) -> Option<&str> {
        self.districts.iter().find(|(_, rings)| geo::contains(rings, lon, lat)).map(|(n, _)| n.as_str())
    }

    /// Is the point inside the region's own country (true when its outline is not known)?
    pub fn in_country(&self, lon: f64, lat: f64) -> bool {
        self.country.as_ref().is_none_or(|(_, rings)| geo::contains(rings, lon, lat))
    }
}
