//! Where a detailed map can be: the 31 countries of the European Union and EFTA, one region each, or, for the five
//! largest, one region per first-level ISO 3166-2 subdivision.
//!
//! A region's code is the ISO code itself: `LU`, `MT`, `AT` for a whole country (ISO 3166-1), `DE-BY`, `FR-IDF`, `IT-25`,
//! `ES-CT`, `PL-14` for a subdivision (ISO 3166-2). The same code names the region's map in the manifest, in
//! `map-set.json`, in `Lastbook-Map-<code>.zip` and in its address index, `derived/geo/addresses/<code>.sqlite`.
//!
//! Each region has one box or more: west, south, east, north in degrees (WGS 84), from Eurostat's NUTS 2024 boundaries
//! at 1:3 million, widened by [`MARGIN`] and rounded outward to 0.01 degrees ([`BOXES`], written by the `region-boxes`
//! tool). A detailed map holds the base map plus every row inside its region's boxes, so it also shows a strip of each
//! neighbour: Berlin lies inside Brandenburg's box and Bremen inside Lower Saxony's. Portugal, with the Azores and
//! Madeira, has three boxes.
//!
//! Not here: France's overseas regions, Svalbard and Jan Mayen. Spain's Canary Islands, Ceuta and Melilla, and
//! Portugal's Azores and Madeira are.

/// Degrees added around the boundary.
pub const MARGIN: f64 = 0.15;

/// Countries split into their first-level subdivisions: each is estimated at more than about 4 GB as one detailed map.
/// To be checked against measured sizes once maps are built.
pub const SPLIT: &[&str] = &["DE", "ES", "FR", "IT", "PL"];

#[derive(Debug, Clone, Copy)]
pub struct Region {
    pub code: &'static str,
    /// The name people there use: Bayern, Lombardia, Île-de-France.
    pub name: &'static str,
    pub name_en: &'static str,
    /// The NUTS 2024 units its boxes come from: one string per box, units joined by `+` make one box together.
    pub nuts: &'static [&'static str],
}

include!("regions_data.rs");
include!("regions_boxes.rs");

/// Has the shape of a region code (`LU`, `DE-BY`, `FR-20R`). Says nothing about whether the region is known.
pub fn is_code(code: &str) -> bool {
    let b = code.as_bytes();
    if b.len() < 2 || !b[0].is_ascii_uppercase() || !b[1].is_ascii_uppercase() {
        return false;
    }
    match b.len() {
        2 => true,
        4..=6 => b[2] == b'-' && b[3..].iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()),
        _ => false,
    }
}

pub fn get(code: &str) -> Option<&'static Region> {
    REGIONS.iter().find(|r| r.code == code)
}

pub fn known(code: &str) -> bool {
    get(code).is_some()
}

/// `DE` for `DE-BY`, `LU` for `LU`.
pub fn country(code: &str) -> &str {
    code.split('-').next().unwrap_or(code)
}

/// For a whole country, its English name; for a subdivision, the local name.
pub fn name(code: &str) -> String {
    if let Some((_, _, en)) = COUNTRIES.iter().find(|(c, _, _)| *c == code) {
        return en.to_string();
    }
    get(code).map(|r| r.name.to_string()).unwrap_or_else(|| code.to_string())
}

pub fn name_en(code: &str) -> String {
    get(code).map(|r| r.name_en.to_string()).unwrap_or_else(|| code.to_string())
}

pub fn boxes(code: &str) -> Option<&'static [[f64; 4]]> {
    BOXES.iter().find(|(c, _)| *c == code).map(|(_, b)| *b)
}

/// One box around all of a region's boxes.
pub fn bounds(code: &str) -> Option<[f64; 4]> {
    let bs = boxes(code)?;
    Some(bs.iter().fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]))
}

pub fn contains(code: &str, lat: f64, lon: f64) -> bool {
    boxes(code).is_some_and(|bs| bs.iter().any(|b| b[0] <= lon && lon <= b[2] && b[1] <= lat && lat <= b[3]))
}

/// The regions of a country, by name, as a person looks for theirs.
pub fn in_country(country_code: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = REGIONS.iter().filter(|r| country(r.code) == country_code).map(|r| r.code).collect();
    out.sort_by_key(|c| name(c).to_lowercase());
    out
}

/// Every region: countries by English name, the regions of a split country by their own name.
pub fn ordered() -> Vec<&'static str> {
    let mut countries: Vec<_> = COUNTRIES.to_vec();
    countries.sort_by_key(|c| c.2);
    countries.iter().flat_map(|c| in_country(c.0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// National capitals, and some regional ones: each in its region's boxes.
    const CAPITALS: &[(&str, f64, f64)] = &[
        ("AT", 48.2082, 16.3738), ("BE", 50.8503, 4.3517), ("BG", 42.6977, 23.3219), ("CH", 46.9480, 7.4474), ("CY", 35.1856, 33.3823),
        ("CZ", 50.0755, 14.4378), ("DE-BE", 52.5200, 13.4050), ("DK", 55.6761, 12.5683), ("EE", 59.4370, 24.7536), ("ES-MD", 40.4168, -3.7038),
        ("FI", 60.1699, 24.9384), ("FR-IDF", 48.8566, 2.3522), ("GR", 37.9838, 23.7275), ("HR", 45.8150, 15.9819), ("HU", 47.4979, 19.0402),
        ("IE", 53.3498, -6.2603), ("IS", 64.1466, -21.9426), ("IT-62", 41.9028, 12.4964), ("LI", 47.1410, 9.5209), ("LT", 54.6872, 25.2797),
        ("LU", 49.6116, 6.1319), ("LV", 56.9496, 24.1052), ("MT", 35.8989, 14.5146), ("NL", 52.3676, 4.9041), ("NO", 59.9139, 10.7522),
        ("PL-14", 52.2297, 21.0122), ("PT", 38.7223, -9.1393), ("RO", 44.4268, 26.1025), ("SE", 59.3293, 18.0686), ("SI", 46.0569, 14.5058),
        ("SK", 48.1486, 17.1077), ("DE-BY", 48.1351, 11.5820), ("DE-HB", 53.0793, 8.8017), ("IT-25", 45.4642, 9.1900), ("IT-82", 38.1157, 13.3615),
        ("ES-CN", 28.1235, -15.4363), ("ES-CE", 35.8894, -5.3213), ("FR-20R", 41.9192, 8.7386), ("PL-12", 50.0647, 19.9450),
        ("PT", 37.7412, -25.6756), ("PT", 32.6669, -16.9241),
    ];
    const OUTSIDE: &[(&str, f64, f64)] = &[
        ("Longyearbyen", 78.2232, 15.6267), ("Cayenne", 4.9224, -52.3135), ("Saint-Denis, Réunion", -20.8821, 55.4507),
        ("Tórshavn", 62.0079, -6.79), ("London", 51.5074, -0.1278), ("Ankara", 39.9334, 32.8597), ("Moscow", 55.7558, 37.6173),
    ];

    #[test]
    fn codes_and_countries() {
        assert_eq!(REGIONS.len(), 110);
        assert_eq!(COUNTRIES.len(), 31);
        for r in REGIONS {
            assert!(is_code(r.code), "{}", r.code);
            assert!(COUNTRIES.iter().any(|c| c.0 == country(r.code)), "{}", r.code);
        }
        for bad in ["de-by", "DEU", "DE_BY", "DE-", "-BY", "../DE-HB", "DE-BAYERN", "", "DE-BY.zip", "D"] {
            assert!(!is_code(bad), "{bad}");
        }
        for good in ["LU", "DE-BY", "FR-20R", "IT-25", "PL-14"] {
            assert!(is_code(good), "{good}");
        }
        for (c, _, _) in COUNTRIES {
            let inside = in_country(c);
            if SPLIT.contains(c) {
                assert!(!known(c) && inside.len() > 1 && inside.iter().all(|r| r.starts_with(&format!("{c}-"))), "{c}");
            } else {
                assert_eq!(inside, vec![*c]);
            }
        }
        assert_eq!(in_country("DE").len(), 16);
        assert_eq!(in_country("FR").len(), 13);
        assert_eq!(in_country("IT").len(), 20);
        assert_eq!(in_country("ES").len(), 19);
        assert_eq!(in_country("PL").len(), 16);
        assert_eq!(ordered().len(), 110);
    }

    #[test]
    fn boxes_and_places() {
        assert_eq!(BOXES.len(), REGIONS.len());
        for r in REGIONS {
            let bs = boxes(r.code).unwrap();
            assert_eq!(bs.len(), if r.code == "PT" { 3 } else { 1 }, "{}", r.code);
            for b in bs {
                assert!(-32.0 < b[0] && b[0] < b[2] && b[2] < 35.0 && 27.0 < b[1] && b[1] < b[3] && b[3] < 72.0, "{} {b:?}", r.code);
            }
        }
        for (code, lat, lon) in CAPITALS {
            assert!(contains(code, *lat, *lon), "{code}");
        }
        for (place, lat, lon) in OUTSIDE {
            assert!(REGIONS.iter().all(|r| !contains(r.code, *lat, *lon)), "{place}");
        }
    }

    #[test]
    fn names() {
        assert_eq!(name("DE-BY"), "Bayern");
        assert_eq!(name_en("DE-BY"), "Bavaria");
        assert_eq!(name("LU"), "Luxembourg");
        assert_eq!(name("IT-25"), "Lombardia");
        assert_eq!(name("unknown"), "unknown");
        assert_eq!(country("FR-20R"), "FR");
        assert!(!known("CA"));
        assert_eq!(in_country("DE")[0], "DE-BW");
        assert_eq!(&ordered()[..3], &["AT", "BE", "BG"]);
    }
}
