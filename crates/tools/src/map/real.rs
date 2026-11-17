//! The regions built from real data, asked what people there type, when their files are here: the data folder of
//! each region set up for the app check (`derived/app-check/<REGION>/LastbookData`, docs/MAPS.md). Nothing is
//! downloaded for this; without the folders the test says so and passes.
//!
//! Each query names the first answer it must give (and its kind, for a place); `~` before a name asks only that it be
//! among the first five.

use lastbook_engine::map::Map;
use lastbook_engine::paths::Paths;
use std::path::PathBuf;

fn data(region: &str) -> Option<PathBuf> {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../derived/app-check").join(region).join("LastbookData");
    d.join("derived/geo/national.sqlite").is_file().then_some(d)
}

const QUERIES: &[(&str, &[(&str, &str, &str)])] = &[
    ("MT", &[
        ("74 Triq ir-Repubblika, Valletta", "74 Triq ir-Repubblika", ""),
        ("Triq ir-Repubblika 74, Il-Belt Valletta", "74 Triq ir-Repubblika", ""),
        ("74 Republic Street, Valletta", "74 Republic Street", ""),
        ("115 Old Theatre Street, Valletta VLT 1426", "115 Old Theatre Street", ""),
        ("VLT 1117", "VLT 1117", ""),
        ("VLT1117", "VLT 1117", ""),
        ("ZTN 3000", "ZTN 3000", ""),
        ("Valletta", "Valletta", "national capital"),
        ("St Julian's", "San Ġiljan", "town"),
        ("Floriana", "~Il-Furjana", "town"),
        ("Comino", "Kemmuna", "island"),
        ("Gozo", "~Għawdex", "island"),
    ]),
    ("LU", &[
        ("2, place Guillaume II, L-1648 Luxembourg", "2 Place Guillaume II", ""),
        ("Place Guillaume II 2, Luxemburg", "2 Place Guillaume II", ""),
        ("45 John F. Kennedy Avenue, Luxembourg", "John F. Kennedy Avenue 45", ""),
        ("5, rue Alphonse Weicker, L-2721 Luxembourg", "5 Rue Alphonse Weicker", ""),
        ("Reisduerferstrooss 27, Ermsdorf", "Reisduerferstrooss 27", ""),
        ("L-1648", "L-1648", ""),
        ("1648 Luxembourg", "L-1648", ""),
        ("Luxemburg", "Luxembourg", "national capital"),
        ("Lëtzebuerg", "Luxembourg", "national capital"),
        ("Esch-Uelzecht", "Esch-sur-Alzette", "town"),
        ("Ettelbréck", "Ettelbruck", "town"),
    ]),
    ("DE-HB", &[
        ("Am Markt 21, 28195 Bremen", "Am Markt 21", ""),
        ("Obernstrasse 1, Bremen", "Obernstraße 1", ""),
        ("Bürgermeister-Smidt-Straße 10, Bremerhaven", "Bürgermeister-Smidt-Straße 10", ""),
        ("Buergermeister-Smidt-Str. 10, 27568 Bremerhaven", "Bürgermeister-Smidt-Straße 10", ""),
        ("28195", "28195", ""),
        ("27568", "27568", ""),
        ("Bremerhaven", "Bremerhaven", "city"),
        ("Vegesack", "Vegesack", "suburb"),
    ]),
];

#[test]
fn real_regions_answer_their_countries_forms() {
    let mut wrong = Vec::new();
    for (region, queries) in QUERIES {
        let Some(lib) = data(region) else {
            eprintln!("{region}: no check folder here, nothing asked");
            continue;
        };
        let home = std::env::temp_dir().join(format!("lb-tools-real-{region}-{}", std::process::id()));
        let map = Map::new(Paths::new(home.clone(), Some(lib)));
        for (q, want, kind) in *queries {
            let got = map.places(q, 5, None);
            let names: Vec<&str> = got.iter().map(|v| v["name"].as_str().unwrap_or("")).collect();
            let ok = match want.strip_prefix('~') {
                Some(w) => got.iter().any(|v| v["name"] == w && (kind.is_empty() || v["kind"] == *kind)),
                None => got.first().is_some_and(|v| v["name"] == *want && (kind.is_empty() || v["kind"] == *kind)),
            };
            if !ok {
                wrong.push(format!("{region} {q:?}: {want} {kind} wanted, got {names:?}"));
            }
        }
        // a postcode offers only its own town, not a stray one (Bremerhaven's 27568 written Bremen)
        if *region == "DE-HB" {
            let towns: Vec<String> = map.places("27568", 5, None).iter().filter(|v| v["name"] == "27568").map(|v| v["detail"].to_string()).collect();
            if towns.len() != 1 {
                wrong.push(format!("DE-HB 27568 offered with {towns:?}"));
            }
        }
        lastbook_engine::db::close_all();
        let _ = std::fs::remove_dir_all(&home);
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
