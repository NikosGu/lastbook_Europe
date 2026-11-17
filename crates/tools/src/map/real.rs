//! The regions built from real data, asked what people there type, when their files are here: the data folder of
//! each region set up for the app check (`derived/app-check/<REGION>/LastbookData`, docs/MAPS.md). Nothing is
//! downloaded for this; without the folders the test says so and passes.

use lastbook_engine::map::Map;
use lastbook_engine::paths::Paths;
use serde_json::Value;
use std::path::PathBuf;

fn data(region: &str) -> Option<PathBuf> {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../derived/app-check").join(region).join("LastbookData");
    d.join("derived/geo/national.sqlite").is_file().then_some(d)
}

fn line(v: &Value) -> String {
    format!("{} | {} | {}", v["name"].as_str().unwrap_or(""), v["kind"].as_str().unwrap_or(""), v["detail"].as_str().unwrap_or(""))
}

fn show(map: &Map, q: &str) -> Vec<Value> {
    let got = map.places(q, 5, None);
    eprintln!("{q:?}");
    for v in &got {
        eprintln!("    {}", line(v));
    }
    got
}

#[test]
fn real_regions_answer_their_countries_forms() {
    let queries: &[(&str, &[&str])] = &[
        ("MT", &["74 Triq ir-Repubblika, Valletta", "Triq ir-Repubblika 74, Il-Belt Valletta", "74 Republic Street, Valletta",
                 "115 Old Theatre Street, Valletta VLT 1426", "VLT 1117", "VLT1117", "vlt 1117", "ZTN 3000", "Valletta", "Floriana",
                 "St Julian's", "Sliema", "Mellieha", "Gozo", "Għawdex", "Comino"]),
        ("LU", &["2, place Guillaume II, L-1648 Luxembourg", "Place Guillaume II 2, Luxemburg", "2 place Guillaume II Lëtzebuerg",
                 "5, rue Alphonse Weicker, L-2721 Luxembourg", "Reisduerferstrooss 27, Ermsdorf", "L-1648", "1648", "1648 Luxembourg",
                 "Luxemburg", "Lëtzebuerg", "Esch-Uelzecht", "Esch an der Alzette", "Kirchberg", "Ettelbréck"]),
        ("DE-HB", &["Am Markt 21, 28195 Bremen", "Am Markt 21 Bremen", "Obernstrasse 1, Bremen", "Bürgermeister-Smidt-Straße 10, Bremerhaven",
                    "Buergermeister-Smidt-Str. 10, 27568 Bremerhaven", "28195", "27568", "Bremerhaven", "Bremen", "Vegesack"]),
    ];
    for (region, qs) in queries {
        let Some(lib) = data(region) else {
            eprintln!("{region}: no check folder here, nothing asked");
            continue;
        };
        let home = std::env::temp_dir().join(format!("lb-tools-real-{region}-{}", std::process::id()));
        let map = Map::new(Paths::new(home.clone(), Some(lib)));
        for q in *qs {
            show(&map, q);
        }
        lastbook_engine::db::close_all();
        let _ = std::fs::remove_dir_all(&home);
    }
}
