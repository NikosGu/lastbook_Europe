//! The region's administrative areas, from the GeoPackage's `gis_osm_adminareas_a_free`: the outline of its own country
//! (or, in a regional extract, of the region), its municipalities (admin level 8) and its districts (level 6).

use crate::map::geo::{self, Part};
use crate::map::gpkg;
use rusqlite::Connection;

#[derive(Debug, Default, Clone)]
pub struct Admin {
    /// The region's country code and the outline its own addresses lie in.
    pub country: Option<(String, Vec<Part>)>,
    pub towns: Vec<(String, Vec<Part>)>,
    pub districts: Vec<(String, Vec<Part>)>,
}

impl Admin {
    pub fn load(g: &Connection, country: &str) -> Admin {
        let rows = gpkg::layer(g, "gis_osm_adminareas_a_free", &["fclass", "name"]);
        let named = |level: &str| -> Vec<(String, Vec<Part>)> {
            rows.iter().filter(|r| r.col(0) == Some(level)).filter_map(|r| Some((r.col(1)?.to_string(), r.parts.clone()))).collect()
        };
        // a country's extract has its national outline; a region's extract (a German state) its own, at level 4
        let own = rows.iter().filter(|r| r.col(0) == Some("national")).chain(rows.iter().filter(|r| r.col(0) == Some("admin_level4")))
            .max_by(|a, b| geo::ring_area_m2(&a.parts[0]).total_cmp(&geo::ring_area_m2(&b.parts[0])));
        Admin { country: own.map(|r| (country.to_string(), r.parts.clone())), towns: named("admin_level8"), districts: named("admin_level6") }
    }

    pub fn town_at(&self, lon: f64, lat: f64) -> Option<&str> {
        self.towns.iter().find(|(_, rings)| geo::contains(rings, lon, lat)).map(|(n, _)| n.as_str())
    }

    pub fn district_at(&self, lon: f64, lat: f64) -> Option<&str> {
        self.districts.iter().find(|(_, rings)| geo::contains(rings, lon, lat)).map(|(n, _)| n.as_str())
    }

    /// Is the point inside the region's own country (true when its outline is not known)?
    pub fn in_country(&self, lon: f64, lat: f64) -> bool {
        self.country.as_ref().is_none_or(|(_, rings)| geo::contains(rings, lon, lat))
    }
}
