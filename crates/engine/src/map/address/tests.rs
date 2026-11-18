//! Address search, against tiny address indexes made here with the index's own schema and keys. The first tests are
//! app/tests/test_address_search.py's, with European addresses; then the keys; then each of the 31 countries, with
//! addresses written as people there write them, a postcode alone and a town alone.

use super::super::tests::{fx, Fx};
use super::*;

type Row<'a> = (&'a str, &'a str, &'a str, &'a str, &'a str, f64, f64); // country, number, street, town, postcode, lat, lon

/// An address index for a region, as a builder makes one: SCHEMA, the rows with their keys, FINISH, meta.
fn index(f: &Fx, region: &str, rows: &[Row]) {
    let c = f.sqlite(&format!("derived/geo/addresses/{region}.sqlite"), SCHEMA);
    let mut countries: Vec<&str> = Vec::new();
    for (country, num, street, city, postcode, lat, lon) in rows {
        Address { num, street, city, state: "", postcode, country, lat: *lat, lon: *lon }.insert(&c).unwrap();
        if !countries.contains(country) {
            countries.push(country);
        }
    }
    c.execute_batch(FINISH).unwrap();
    c.execute("INSERT INTO meta VALUES ('region', ?1), ('countries', ?2), ('publisher', 'Test addresses'), ('schema', '2')", [region, &countries.join(",")]).unwrap();
}

fn names(v: &[Value]) -> Vec<String> {
    v.iter().map(|r| r["name"].as_str().unwrap_or("").to_string()).collect()
}

const LI: &[Row] = &[
    ("LI", "5", "Städtle", "Vaduz", "9490", 47.1395, 9.5215),
    ("LI", "10", "Äulestrasse", "Vaduz", "9490", 47.1372, 9.5190),
    ("LI", "12", "Äulestrasse", "Vaduz", "9490", 47.1371, 9.5192),
    ("LI", "7", "Landstrasse", "Schaan", "9494", 47.1650, 9.5100),
    ("LI", "7", "Landstrasse", "Triesen", "9495", 47.1050, 9.5280),
    ("CH", "3", "Bahnhofstrasse", "Buchs SG", "9470", 47.1680, 9.4780),
    ("AT", "7", "Landstraße", "Feldkirch", "6800", 47.2380, 9.5980),
];

#[test]
fn every_way_of_writing_one_address() {
    let f = fx("addr-forms");
    index(&f, "LI", LI);
    for q in ["Äulestrasse 10 Vaduz", "Äulestraße 10, 9490 Vaduz", "Aeulestr. 10", "aulestrasse 10, vaduz, liechtenstein", "ÄULESTRASSE 10 FL-9490 VADUZ",
              "10 Äulestrasse Vaduz", "Äulestrasse Nr. 10", "Äule 10, Vaduz"] {
        let r = f.map.find_addresses(q, 8, None);
        assert_eq!(names(&r), vec!["Äulestrasse 10"], "{q}");
        assert_eq!((r[0]["kind"].as_str(), r[0]["kind_code"].as_str()), (Some("Street address"), Some("street_address")));
        assert_eq!(r[0]["detail"], "9490 Vaduz, Liechtenstein");
        assert_eq!((r[0]["locality"].as_str(), r[0]["country"].as_str()), (Some("9490 Vaduz"), Some("LI")), "the country apart, for the interface to name");
        assert_eq!(r[0]["lat"], 47.1372);
        assert_eq!(r[0]["source"], "Test addresses");
        assert_eq!(f.map.query_kind(q), Kind::Address, "{q}");
    }
}

#[test]
fn an_unknown_number_does_not_return_a_nearby_house() {
    let f = fx("addr-unknown");
    index(&f, "LI", LI);
    assert!(f.map.find_addresses("Äulestrasse 11 Vaduz", 8, None).is_empty());
    assert!(f.map.find_addresses("Äulestrasse 10a", 8, None).is_empty(), "10a is another house");
    assert!(f.map.places("Äulestrasse 11", 20, None).is_empty());
}

#[test]
fn town_and_postcode_do_not_silently_fall_back() {
    let f = fx("addr-fallback");
    index(&f, "LI", LI);
    assert!(f.map.find_addresses("Äulestrasse 10 Wrongtown", 8, None).is_empty());
    assert!(f.map.find_addresses("Äulestrasse 10, 9494 Vaduz", 8, None).is_empty(), "the wrong postcode");
    assert!(f.map.find_addresses("Äulestrasse 10, 9494", 8, None).is_empty());
}

#[test]
fn the_town_and_the_country_choose_among_streets_of_one_name() {
    let f = fx("addr-country");
    index(&f, "LI", LI);
    let all = f.map.find_addresses("Landstrasse 7", 8, None);
    assert_eq!(all.len(), 3, "Schaan, Triesen, and Feldkirch across the border: {all:?}");
    let schaan = f.map.find_addresses("Landstrasse 7 Schaan", 8, None);
    assert_eq!(schaan.len(), 1);
    assert_eq!(schaan[0]["detail"], "9494 Schaan, Liechtenstein");
    let austria = f.map.find_addresses("Landstraße 7, Österreich", 8, None);
    assert_eq!(names(&austria), vec!["Landstraße 7"]);
    assert_eq!(austria[0]["detail"], "6800 Feldkirch, Austria");
    let near = f.map.find_addresses("Landstrasse 7", 8, Some((47.10, 9.53)));
    assert_eq!(near[0]["detail"], "9495 Triesen, Liechtenstein", "nearest to where the map looks, among equals");
    assert_eq!(f.map.find_addresses("Bahnhofstrasse 3 Buchs", 8, None)[0]["detail"], "9470 Buchs SG, Switzerland");
}

#[test]
fn a_flat_or_a_staircase_is_the_same_building() {
    let f = fx("addr-flat");
    index(&f, "AT", &[("AT", "12", "Neubaugasse", "Wien", "1070", 48.2010, 16.3490), ("AT", "14-16", "Neubaugasse", "Wien", "1070", 48.2012, 16.3491)]);
    assert_eq!(names(&f.map.find_addresses("Neubaugasse 12/3/14, Wien", 8, None)), vec!["Neubaugasse 12"]);
    assert_eq!(names(&f.map.find_addresses("Neubaugasse 14-16", 8, None)), vec!["Neubaugasse 14-16"]);
    assert_eq!(names(&f.map.find_addresses("Neubaugasse 12, Top 5, 1070 Wien", 8, None)), vec!["Neubaugasse 12"]);
}

#[test]
fn what_a_query_is() {
    for (q, kind) in [("9490", Kind::Postcode), ("FL-9490", Kind::Postcode), ("9490 Vaduz", Kind::Postcode), ("Vaduz 9490", Kind::Postcode), ("1012 AB", Kind::Postcode),
                      ("1012 AB Amsterdam", Kind::Postcode), ("00-950 Warszawa", Kind::Postcode), ("D02 X285", Kind::Postcode), ("VLT 1117", Kind::Postcode),
                      ("Vaduz", Kind::Place), ("Frankfurt am Main", Kind::Place), ("Lago di Garda", Kind::Place), ("Neumarkt", Kind::Place), ("Zaandijk", Kind::Place),
                      ("Städtle 12 Vaduz", Kind::Address), ("Via Roma 12", Kind::Address), ("12 rue de la Paix", Kind::Address), ("Hauptstraße 5, 9490 Vaduz", Kind::Address),
                      ("Bahnhofstrasse Zürich", Kind::Address), ("Calle Mayor, 5", Kind::Address), ("ul. Marszałkowska 1", Kind::Address)] {
        assert_eq!(parse(q).kind, kind, "{q}");
    }
    let q = parse("Hauptstraße 5, 9490 Vaduz");
    assert_eq!(q.addresses[0].number.as_deref(), Some("5"));
    assert_eq!(q.addresses[0].street, "Hauptstraße");
    assert_eq!(q.addresses[0].town, "Vaduz");
    assert_eq!(q.addresses[0].postcode.as_ref().map(|p| p.key.as_str()), Some("9490"));
    let q = parse("12 bis, rue de la Paix, 75002 Paris, France");
    assert_eq!(q.country, Some("FR"));
    assert_eq!((q.addresses[0].number.as_deref(), q.addresses[0].street.as_str(), q.addresses[0].town.as_str()), (Some("12bis"), "rue de la Paix", "Paris"));
    let q = parse("Corso Italia 5");
    assert!(q.addresses.iter().any(|r| r.street == "Corso Italia"), "a street named after a country is still a street: {:?}", q.addresses);
    let q = parse("Calle Mayor 5, 3º izq, Madrid");
    assert_eq!((q.addresses[0].street.as_str(), q.addresses[0].town.as_str()), ("Calle Mayor", "Madrid"), "the floor and the door are not the town");
    let q = parse("Østergade 12, 2. tv, København");
    assert_eq!(q.addresses[0].town, "København");
}

#[test]
fn no_indexes_no_addresses() {
    let f = fx("addr-none");
    std::fs::remove_dir_all(f.file("derived/geo/addresses")).unwrap();
    assert!(f.map.find_addresses("Städtle 5 Vaduz", 8, None).is_empty());
    // an American index of the original Lastbook's day (no country, no keys) is passed over, not an error
    f.sqlite("derived/geo/addresses/LI.sqlite", "CREATE TABLE addr(num TEXT,street TEXT,norm TEXT,city TEXT,state TEXT,zip TEXT,lat REAL,lon REAL);
        INSERT INTO addr VALUES('5','Städtle','stadtle','Vaduz','LI','9490',47.1,9.5);");
    assert!(f.map.find_addresses("Städtle 5 Vaduz", 8, None).is_empty());
    assert!(f.map.places("9490", 20, None).is_empty());
}

#[test]
fn the_route_keeps_the_house_number_and_says_what_it_read() {
    let f = fx("addr-route");
    index(&f, "LI", LI);
    let r = f.get("/api/places?q=St%C3%A4dtle+5&near=47.14,9.52");
    assert_eq!(r.header("X-Query-Kind"), Some("address"));
    let v: Value = serde_json::from_slice(&r.body).unwrap();
    assert_eq!(v[0]["name"], "Städtle 5");
    assert_eq!((v[0]["kind"].as_str(), v[0]["kind_code"].as_str(), v[0]["locality"].as_str(), v[0]["country"].as_str()),
               (Some("Street address"), Some("street_address"), Some("9490 Vaduz"), Some("LI")), "the page reads them from the route");
    let none = f.get("/api/places?q=St%C3%A4dtle+7");
    assert_eq!((none.header("X-Query-Kind"), none.body.as_slice()), (Some("address"), b"[]".as_slice()), "an exact house or nothing");
    assert_eq!(f.get("/api/places?q=9490").header("X-Query-Kind"), Some("postcode"));
    assert_eq!(f.get("/api/places?q=Vaduz").header("X-Query-Kind"), Some("place"));
    assert_eq!(f.get("/api/places").body, b"[]");
    let many: Vec<Row> = (1..=30).map(|i| ("LI", "1", "Landstrasse", "Ort", "9494", 47.0 + i as f64 / 100.0, 9.5)).collect();
    index(&f, "LI2", &many);
    assert!(f.map.places("Landstrasse 1", 20, None).len() <= 8, "at most eight addresses, as before");
}

#[test]
fn streets_without_a_number() {
    let f = fx("addr-street");
    index(&f, "CH", &[("CH", "1", "Bahnhofstrasse", "Zürich", "8001", 47.3667, 8.5400), ("CH", "9", "Bahnhofstrasse", "Zürich", "8001", 47.3700, 8.5390),
                      ("CH", "2", "Bahnhofstrasse", "Bern", "3011", 46.9480, 7.4390)]);
    let r = f.map.places("Bahnhofstrasse Zürich", 20, None);
    assert_eq!(names(&r), vec!["Bahnhofstrasse"]);
    assert_eq!((r[0]["kind"].as_str(), r[0]["kind_code"].as_str()), (Some("Street"), Some("street")));
    assert_eq!(r[0]["detail"], "Zürich, Switzerland");
    assert_eq!((r[0]["locality"].as_str(), r[0]["country"].as_str()), (Some("Zürich"), Some("CH")));
    assert!((r[0]["lat"].as_f64().unwrap() - 47.36835).abs() < 1e-6, "the middle of its addresses");
    assert_eq!(f.map.places("Bahnhofstr.", 20, None).len(), 2, "one per town");
}

#[test]
fn street_keys_meet_across_spellings() {
    for (c, a, b) in [
        ("DE", "Hauptstraße", "Hauptstr."), ("DE", "Hauptstraße", "Haupt-Straße"), ("DE", "Berliner Straße", "Berlinerstrasse"), ("DE", "Straße des 17. Juni", "Str. des 17. Juni"),
        ("DE", "Sankt-Georg-Platz", "St.-Georg-Pl."), ("AT", "Mariahilfer Straße", "Mariahilferstr."), ("CH", "Rue du Rhône", "rue du Rhone"), ("BE", "Wetstraat", "Wetstr."),
        ("BE", "Place Royale", "Pl. Royale"), ("NL", "Kalverstraat", "Kalverstr."), ("FR", "Avenue des Champs-Élysées", "av. des Champs Elysees"),
        ("FR", "Rue du Faubourg Saint-Honoré", "r. du Fbg St-Honoré"), ("FR", "Boulevard Saint-Germain", "Bd St Germain"), ("IT", "Viale Monza", "V.le Monza"),
        ("IT", "Piazza del Duomo", "P.za Duomo"), ("IT", "Corso Vittorio Emanuele II", "C.so Vittorio Emanuele II"), ("IT", "Via della Spiga", "via Spiga"),
        ("ES", "Calle Mayor", "C/ Mayor"), ("ES", "Calle Mayor", "C/Mayor"), ("ES", "Avenida de la Constitución", "Avda. Constitucion"),
        ("ES", "Paseo de la Castellana", "Pº de la Castellana"), ("ES", "Carrer de Balmes", "C/ Balmes"), ("ES", "Plaça de Catalunya", "Pl. Catalunya"),
        ("PT", "Avenida da Liberdade", "Av. Liberdade"), ("PT", "Praça do Comércio", "Pç. Comércio"), ("PL", "Marszałkowska", "ul. Marszalkowska"),
        ("PL", "Aleje Jerozolimskie", "al. Jerozolimskie"), ("PL", "Plac Defilad", "pl. Defilad"), ("CZ", "Václavské náměstí", "Vaclavske nam."),
        ("SK", "Hlavné námestie", "Hlavne nam."), ("HU", "Váci utca", "Váci u."), ("HU", "Váci utca", "Vaci"), ("SI", "Slovenska cesta", "Slovenska c."),
        ("HR", "Trg bana Josipa Jelačića", "Trg bana Josipa Jelacica"), ("RO", "Strada Lipscani", "Str. Lipscani"), ("RO", "Bulevardul Unirii", "Bd. Unirii"),
        ("BG", "улица Граф Игнатиев", "ул. Граф Игнатиев"), ("BG", "булевард Витоша", "бул. Витоша"), ("GR", "Οδός Ερμού", "ΕΡΜΟΥ"),
        ("GR", "Λεωφόρος Βασιλίσσης Σοφίας", "Λεωφ. Βασιλισσης Σοφιας"), ("CY", "Λεωφόρος Μακαρίου", "Λεωφ. Μακαρίου"), ("LV", "Brīvības iela", "Brivibas"),
        ("LT", "Gedimino prospektas", "Gedimino pr."), ("LT", "Pilies gatvė", "Pilies g."), ("EE", "Pärnu maantee", "Parnu mnt"), ("FI", "Mannerheimintie", "Mannerheimin tie"),
        ("FI", "Aleksanterinkatu", "Aleksanterin katu"), ("SE", "Sveavägen", "Sveavagen"), ("SE", "Drottninggatan", "Drottning gatan"), ("NO", "Karl Johans gate", "Karl Johans gt."),
        ("NO", "Storgata", "Storgaten"), ("DK", "Østergade", "Oestergade"), ("DK", "H.C. Andersens Boulevard", "H.C. Andersens Blvd."), ("IS", "Skólavörðustígur", "Skolavordustigur"),
        ("IE", "Main Street", "Main St"), ("IE", "Main Street", "Main St."), ("IE", "St Stephen's Green", "Saint Stephen's Green"), ("MT", "Triq il-Kbira", "Triq Il Kbira"),
        ("LU", "Avenue de la Gare", "av. de la Gare"), ("LI", "Äulestrasse", "Aeulestr."), ("IT", "Bahnhofstraße", "Bahnhofstr."),
    ] {
        assert_eq!(street_key(a, c), street_key(b, c), "{c}: {a} / {b}");
    }
    assert_ne!(street_key("Hauptstraße", "DE"), street_key("Hauptweg", "DE"));
    assert_ne!(street_key("Via Roma", "IT"), street_key("Piazza Roma", "IT"));
    assert_ne!(street_key("Rue de la Gare", "FR"), street_key("Avenue de la Gare", "FR"));
    assert_ne!(street_key("Váci utca", "HU"), street_key("Váci út", "HU"));
    assert_ne!(street_key("Main Street", "IE"), street_key("Main Road", "IE"));
    assert_eq!(street_key("Straße", "DE"), "strasse", "a name that is only the street word keeps it");
    assert_eq!(street_key("Bahnhofstraße", "DE"), "bahnhof");
}

#[test]
fn house_numbers() {
    for (a, b) in [("12 bis", "12bis"), ("12 A", "12a"), ("12/A", "12a"), ("12-a", "12a"), ("10.", "10"), ("12/3", "12/3"), ("12-14", "12-14"), ("846/1", "846/1"),
                   ("бл. 3", "3"), ("бл.12", "12"), ("Бл. 3А", "3а"), ("Nr. 5", "5"), ("No 7", "7"), ("n12", "n12"), ("бл", "бл")] {
        assert_eq!(number_key(a), b, "{a}");
    }
    assert_eq!(number_forms("12/3/14"), vec!["12/3/14", "12/3", "12"]);
    assert_eq!(number_forms("12-14"), vec!["12-14", "12"]);
    assert_eq!(number_forms("12a"), vec!["12a"]);
    for n in ["12", "12a", "12bis", "12/3", "12-14", "12/3/14", "12/a", "10.", "5th"] {
        assert!(is_number(&fold(n)), "{n}");
    }
    for n in ["a12", "123456", "12abc", "", "x"] {
        assert!(!is_number(&fold(n)), "{n}");
    }
}

#[test]
fn shown_as_people_there_write_it() {
    assert_eq!(shown("12", "Via Roma", "IT"), "Via Roma 12");
    assert_eq!(shown("12", "rue de la Paix", "FR"), "12 rue de la Paix");
    assert_eq!(shown("5", "Calle Mayor", "ES"), "Calle Mayor, 5");
    assert_eq!(shown("16", "Rue de la Loi", "BE"), "16 Rue de la Loi");
    assert_eq!(shown("16", "Wetstraat", "BE"), "Wetstraat 16");
    assert_eq!(shown("12", "Rue du Rhône", "CH"), "Rue du Rhône 12");
    assert_eq!(shown("12", "Main Street", "IE"), "12 Main Street");
    assert_eq!(detail("Dublin 2", "D02 X285", "IE"), "Dublin 2, D02 X285, Ireland");
    assert_eq!(detail("Valletta", "VLT 1117", "MT"), "Valletta VLT 1117, Malta");
    assert_eq!(detail("Rīga", "1050", "LV"), "Rīga, LV-1050, Latvia");
    assert_eq!(detail("Luxembourg", "1611", "LU"), "L-1611 Luxembourg, Luxembourg");
    assert_eq!(detail("Vaduz", "", "LI"), "Vaduz, Liechtenstein");
    // the same without the country, which the interface names in the person's language
    assert_eq!(locality("Dublin 2", "D02 X285", "IE"), "Dublin 2, D02 X285");
    assert_eq!(locality("Valletta", "VLT 1117", "MT"), "Valletta VLT 1117");
    assert_eq!(locality("Luxembourg", "1611", "LU"), "L-1611 Luxembourg");
    assert_eq!(locality("", "1050", "LV"), "LV-1050");
    assert_eq!(with_country("", "LV"), "Latvia");
}

/// Each kind of place found is a code beside its English kind, with its town and postcode and its country apart.
#[test]
fn each_kind_is_a_code_beside_its_english() {
    let f = fx("addr-kinds");
    index(&f, "LI", LI);
    let mut said = Vec::new();
    for (q, kind, code, name, locality, detail) in [
        ("Städtle 5 Vaduz", "Street address", "street_address", "Städtle 5", "9490 Vaduz", "9490 Vaduz, Liechtenstein"),
        ("Landstrasse Schaan", "Street", "street", "Landstrasse", "Schaan", "Schaan, Liechtenstein"),
        ("9490", "Postcode", "postcode", "9490", "Vaduz", "Vaduz, Liechtenstein"),
        ("Schaan", "Town", "town", "Schaan", "9494", "9494, Liechtenstein"),
    ] {
        let r = f.map.places(q, 20, None);
        let p = r.iter().find(|p| p["kind"] == kind).unwrap_or_else(|| panic!("{q}: {r:?}"));
        assert_eq!((p["kind_code"].as_str(), p["name"].as_str(), p["locality"].as_str(), p["country"].as_str(), p["detail"].as_str()),
                   (Some(code), Some(name), Some(locality), Some("LI"), Some(detail)), "{q}");
        said.push(code);
    }
    assert_eq!(said, KIND_CODES, "every kind code is said");
}

#[test]
fn every_code_has_words_in_the_interface() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/locales/en.json");
    let words: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for code in KIND_CODES {
        assert!(words[format!("map.kind.{code}")].is_string(), "web/locales/en.json has no map.kind.{code}");
    }
}

/// One country: its addresses, queries that must find each (in the local way of writing them), a postcode alone and a
/// town alone.
fn country(region: &str, rows: &[Row], queries: &[(&str, &str)], postcode: (&str, &str), town: (&str, &str)) {
    let f = fx(&format!("addr-{region}"));
    index(&f, region, rows);
    for (q, want) in queries {
        let r = f.map.places(q, 20, None);
        assert_eq!(f.map.query_kind(q), Kind::Address, "{region} {q}");
        assert_eq!(r.first().and_then(|r| r["name"].as_str()), Some(*want), "{region} {q}: {r:?}");
        assert_eq!((r[0]["kind"].as_str(), r[0]["kind_code"].as_str()), (Some("Street address"), Some("street_address")), "{region} {q}");
        assert_eq!(r[0]["country"].as_str(), Some(regions::country(region)), "{region} {q}");
    }
    let r = f.map.places(postcode.0, 20, None);
    assert_eq!(f.map.query_kind(postcode.0), Kind::Postcode, "{region} {}", postcode.0);
    assert_eq!((r.first().and_then(|r| r["kind"].as_str()), r.first().and_then(|r| r["kind_code"].as_str()), r.first().and_then(|r| r["name"].as_str())),
               (Some("Postcode"), Some("postcode"), Some(postcode.1)), "{region} {}: {r:?}", postcode.0);
    let r = f.map.places(town.0, 20, None);
    assert_eq!(f.map.query_kind(town.0), Kind::Place, "{region} {}", town.0);
    assert!(r.iter().any(|p| p["kind"] == "Town" && p["kind_code"] == "town" && p["name"] == town.1), "{region} {}: {r:?}", town.0);
}

#[test]
fn austria() {
    country("AT", &[("AT", "2", "Ballhausplatz", "Wien", "1010", 48.2087, 16.3636), ("AT", "120", "Mariahilfer Straße", "Wien", "1070", 48.1965, 16.3405),
                    ("AT", "12", "Neubaugasse", "Wien", "1070", 48.2010, 16.3490)],
            &[("Ballhausplatz 2", "Ballhausplatz 2"), ("Mariahilfer Str. 120, 1070 Wien", "Mariahilfer Straße 120"), ("Mariahilferstrasse 120", "Mariahilfer Straße 120"),
              ("Neubaugasse 12/3/14, Wien", "Neubaugasse 12")],
            ("1070", "1070"), ("Wien", "Wien"));
}

#[test]
fn belgium() {
    country("BE", &[("BE", "16", "Rue de la Loi", "Bruxelles", "1000", 50.8462, 4.3667), ("BE", "16", "Wetstraat", "Brussel", "1000", 50.8462, 4.3667),
                    ("BE", "1", "Grote Markt", "Antwerpen", "2000", 51.2213, 4.3997)],
            &[("16 rue de la Loi, Bruxelles", "16 Rue de la Loi"), ("Rue de la Loi 16, 1000 Bruxelles", "16 Rue de la Loi"), ("Wetstraat 16 Brussel", "Wetstraat 16"),
              ("Wetstr. 16", "Wetstraat 16"), ("Grote Markt 1, 2000 Antwerpen", "Grote Markt 1")],
            ("2000", "2000"), ("Antwerpen", "Antwerpen"));
}

#[test]
fn bulgaria() {
    // a housing estate's blocks are numbered, not its houses: the first address of Bulgaria's index, which its check typed
    country("BG", &[("BG", "15", "бул. Витоша", "София", "1000", 42.6939, 23.3216), ("BG", "2", "ул. Граф Игнатиев", "София", "1000", 42.6925, 23.3260),
                    ("BG", "бл. 3", "ж.к. Дружба 1", "София", "1592", 42.6517, 23.3961)],
            &[("бул. Витоша 15", "бул. Витоша 15"), ("булевард Витоша 15, София", "бул. Витоша 15"), ("Граф Игнатиев 2", "ул. Граф Игнатиев 2"),
              ("ж.к. Дружба 1 бл. 3, София", "ж.к. Дружба 1 бл. 3"), ("Дружба 1, бл. 3", "ж.к. Дружба 1 бл. 3"), ("жк Дружба 1 блок 3", "ж.к. Дружба 1 бл. 3"),
              ("ж.к. Дружба 1, бл. 3, вх. А, ет. 2, София", "ж.к. Дружба 1 бл. 3")],
            ("1000", "1000"), ("София", "София"));
}

#[test]
fn switzerland() {
    country("CH", &[("CH", "1", "Bahnhofstrasse", "Zürich", "8001", 47.3667, 8.5400), ("CH", "12", "Rue du Rhône", "Genève", "1204", 46.2033, 6.1475),
                    ("CH", "5", "Via Nassa", "Lugano", "6900", 46.0037, 8.9511)],
            &[("Bahnhofstr. 1 Zürich", "Bahnhofstrasse 1"), ("Bahnhofstraße 1, 8001 Zuerich", "Bahnhofstrasse 1"), ("Rue du Rhône 12, Genève", "Rue du Rhône 12"),
              ("12 rue du Rhone, 1204 Geneve", "Rue du Rhône 12"), ("Via Nassa 5, Lugano", "Via Nassa 5")],
            ("CH-8001", "8001"), ("Zurich", "Zürich"));
}

#[test]
fn cyprus() {
    country("CY", &[("CY", "50", "Λήδρας", "Λευκωσία", "1011", 35.1725, 33.3617), ("CY", "25", "Λεωφόρος Μακαρίου", "Λευκωσία", "1065", 35.1631, 33.3600)],
            &[("Λήδρας 50", "Λήδρας 50"), ("ΛΗΔΡΑΣ 50 ΛΕΥΚΩΣΙΑ", "Λήδρας 50"), ("Λεωφ. Μακαρίου 25, 1065 Λευκωσία", "Λεωφόρος Μακαρίου 25")],
            ("1065", "1065"), ("Λευκωσία", "Λευκωσία"));
}

#[test]
fn czechia() {
    country("CZ", &[("CZ", "846/1", "Václavské náměstí", "Praha 1", "110 00", 50.0814, 14.4261), ("CZ", "1", "Karlova", "Praha 1", "110 00", 50.0860, 14.4170)],
            &[("Václavské náměstí 846/1, Praha", "Václavské náměstí 846/1"), ("Vaclavske nam. 846/1, 110 00 Praha 1", "Václavské náměstí 846/1"), ("Karlova 1, Praha", "Karlova 1")],
            ("110 00", "110 00"), ("Praha", "Praha 1"));
}

#[test]
fn germany() {
    country("DE-BE", &[("DE", "77", "Unter den Linden", "Berlin", "10117", 52.5163, 13.3806), ("DE", "135", "Straße des 17. Juni", "Berlin", "10623", 52.5126, 13.3279),
                       ("DE", "5", "Hauptstraße", "Berlin", "10827", 52.4870, 13.3550)],
            &[("Unter den Linden 77", "Unter den Linden 77"), ("Straße des 17. Juni 135, 10623 Berlin", "Straße des 17. Juni 135"), ("Str. des 17. Juni 135", "Straße des 17. Juni 135"),
              ("Hauptstr. 5 Berlin", "Hauptstraße 5"), ("Hauptstrasse 5, D-10827 Berlin", "Hauptstraße 5")],
            ("10117", "10117"), ("Berlin", "Berlin"));
}

#[test]
fn denmark() {
    country("DK", &[("DK", "12", "Østergade", "København K", "1100", 55.6795, 12.5800), ("DK", "27", "H.C. Andersens Boulevard", "København V", "1553", 55.6750, 12.5690)],
            &[("Østergade 12, 2. tv, København", "Østergade 12"), ("Oestergade 12", "Østergade 12"), ("H.C. Andersens Blvd. 27", "H.C. Andersens Boulevard 27")],
            ("1100", "1100"), ("København K", "København K"));
}

#[test]
fn estonia() {
    country("EE", &[("EE", "10", "Pärnu mnt", "Tallinn", "10148", 59.4342, 24.7480), ("EE", "2", "Viru tänav", "Tallinn", "10140", 59.4365, 24.7470)],
            &[("Pärnu maantee 10, Tallinn", "Pärnu mnt 10"), ("Parnu mnt 10", "Pärnu mnt 10"), ("Viru 2, 10140 Tallinn", "Viru tänav 2")],
            ("10148", "10148"), ("Tallinn", "Tallinn"));
}

#[test]
fn spain() {
    country("ES-MD", &[("ES", "5", "Calle Mayor", "Madrid", "28013", 40.4155, -3.7074), ("ES", "100", "Paseo de la Castellana", "Madrid", "28046", 40.4410, -3.6910)],
            &[("C/ Mayor, 5, 28013 Madrid", "Calle Mayor, 5"), ("Calle Mayor 5", "Calle Mayor, 5"), ("Mayor 5, Madrid", "Calle Mayor, 5"),
              ("Pº de la Castellana 100", "Paseo de la Castellana, 100"), ("Calle Mayor 5, 3º izq, Madrid", "Calle Mayor, 5")],
            ("28013", "28013"), ("Madrid", "Madrid"));
    country("ES-CT", &[("ES", "10", "Carrer de Balmes", "Barcelona", "08007", 41.3880, 2.1620)],
            &[("C/ Balmes 10, Barcelona", "Carrer de Balmes, 10"), ("Carrer Balmes, 10", "Carrer de Balmes, 10")],
            ("08007", "08007"), ("Barcelona", "Barcelona"));
}

#[test]
fn finland() {
    country("FI", &[("FI", "13", "Mannerheimintie", "Helsinki", "00100", 60.1699, 24.9384), ("FI", "1", "Aleksanterinkatu", "Helsinki", "00170", 60.1690, 24.9500)],
            &[("Mannerheimintie 13, 00100 Helsinki", "Mannerheimintie 13"), ("Aleksanterinkatu 1 Helsinki", "Aleksanterinkatu 1")],
            ("00100", "00100"), ("Helsinki", "Helsinki"));
}

#[test]
fn france() {
    country("FR-IDF", &[("FR", "12", "Rue de la Paix", "Paris", "75002", 48.8686, 2.3310), ("FR", "55", "Rue du Faubourg Saint-Honoré", "Paris", "75008", 48.8704, 2.3165),
                        ("FR", "1", "Avenue des Champs-Élysées", "Paris", "75008", 48.8656, 2.3212), ("FR", "12bis", "Rue de la Paix", "Paris", "75002", 48.8687, 2.3311)],
            &[("12 rue de la Paix, 75002 Paris", "12 Rue de la Paix"), ("12 r. de la Paix Paris", "12 Rue de la Paix"), ("12 RUE DE LA PAIX", "12 Rue de la Paix"),
              ("12 bis rue de la Paix", "12bis Rue de la Paix"), ("55 rue du Fbg St-Honoré", "55 Rue du Faubourg Saint-Honoré"),
              ("1 av. des Champs-Elysées, Paris", "1 Avenue des Champs-Élysées")],
            ("75008", "75008"), ("Paris", "Paris"));
}

#[test]
fn greece() {
    country("GR", &[("GR", "10", "Ερμού", "Αθήνα", "105 63", 37.9764, 23.7302), ("GR", "1", "Λεωφόρος Βασιλίσσης Σοφίας", "Αθήνα", "106 74", 37.9750, 23.7380)],
            &[("Ερμού 10, 105 63 Αθήνα", "Ερμού 10"), ("ΕΡΜΟΥ 10 ΑΘΗΝΑ", "Ερμού 10"), ("Οδός Ερμού 10", "Ερμού 10"), ("Λεωφ. Βασιλίσσης Σοφίας 1", "Λεωφόρος Βασιλίσσης Σοφίας 1")],
            ("105 63", "105 63"), ("Αθηνα", "Αθήνα"));
}

#[test]
fn croatia() {
    country("HR", &[("HR", "10", "Ilica", "Zagreb", "10000", 45.8131, 15.9700), ("HR", "1", "Trg bana Josipa Jelačića", "Zagreb", "10000", 45.8130, 15.9775)],
            &[("Ilica 10, 10000 Zagreb", "Ilica 10"), ("Trg bana Josipa Jelacica 1", "Trg bana Josipa Jelačića 1")],
            ("10000", "10000"), ("Zagreb", "Zagreb"));
}

#[test]
fn hungary() {
    country("HU", &[("HU", "10", "Váci utca", "Budapest", "1052", 47.4936, 19.0526), ("HU", "2", "Andrássy út", "Budapest", "1061", 47.5020, 19.0570)],
            &[("Váci u. 10., Budapest", "Váci utca 10"), ("Vaci utca 10, 1052 Budapest", "Váci utca 10"), ("Andrássy út 2", "Andrássy út 2")],
            ("1052", "1052"), ("Budapest", "Budapest"));
}

#[test]
fn ireland() {
    country("IE", &[("IE", "10", "O'Connell Street Upper", "Dublin 1", "D01 F5P2", 53.3498, -6.2603), ("IE", "12", "Main Street", "Galway", "H91 A0C3", 53.2740, -9.0490)],
            &[("10 O'Connell Street Upper, Dublin 1", "10 O'Connell Street Upper"), ("12 Main St, Galway", "12 Main Street"), ("12 Main Street H91 A0C3", "12 Main Street")],
            ("D01 F5P2", "D01 F5P2"), ("Galway", "Galway"));
    let f = fx("addr-IE-key");
    index(&f, "IE", &[("IE", "12", "Main Street", "Galway", "H91 A0C3", 53.2740, -9.0490)]);
    assert_eq!(f.map.places("H91", 20, None)[0]["name"], "H91 A0C3", "an Eircode's routing key names its area");
    assert_eq!(f.map.places("12 Main Street, Galway", 20, None)[0]["detail"], "Galway, H91 A0C3, Ireland");
}

#[test]
fn iceland() {
    country("IS", &[("IS", "10", "Laugavegur", "Reykjavík", "101", 64.1447, -21.9300), ("IS", "5", "Skólavörðustígur", "Reykjavík", "101", 64.1440, -21.9280)],
            &[("Laugavegur 10, 101 Reykjavík", "Laugavegur 10"), ("Skolavordustigur 5 Reykjavik", "Skólavörðustígur 5")],
            ("101", "101"), ("Reykjavik", "Reykjavík"));
}

#[test]
fn italy() {
    country("IT-62", &[("IT", "12", "Via del Corso", "Roma", "00186", 41.9009, 12.4797), ("IT", "1", "Piazza Navona", "Roma", "00186", 41.8992, 12.4731),
                       ("IT", "21", "Viale di Trastevere", "Roma", "00153", 41.8840, 12.4700)],
            &[("Via del Corso 12, 00186 Roma", "Via del Corso 12"), ("via Corso 12", "Via del Corso 12"), ("Corso 12, Roma", "Via del Corso 12"),
              ("P.za Navona 1", "Piazza Navona 1"), ("Piazza Navona, 1 Roma", "Piazza Navona 1"), ("V.le Trastevere 21", "Viale di Trastevere 21")],
            ("00186", "00186"), ("Roma", "Roma"));
    country("IT-32", &[("IT", "8", "Laubengasse", "Bozen", "39100", 46.4990, 11.3540), ("IT", "8", "Via dei Portici", "Bolzano", "39100", 46.4990, 11.3540)],
            &[("Laubengasse 8 Bozen", "Laubengasse 8"), ("Via Portici 8, Bolzano", "Via dei Portici 8")],
            ("39100", "39100"), ("Bolzano", "Bolzano"));
}

#[test]
fn liechtenstein() {
    country("LI", LI, &[("Städtle 5 Vaduz", "Städtle 5"), ("Staedtle 5", "Städtle 5"), ("Äulestrasse 10, 9490 Vaduz", "Äulestrasse 10"), ("Aeulestr. 10", "Äulestrasse 10"),
                        ("Landstraße 7, 9494 Schaan", "Landstrasse 7")],
            ("FL-9494", "9494"), ("Schaan", "Schaan"));
}

#[test]
fn lithuania() {
    country("LT", &[("LT", "10", "Gedimino pr.", "Vilnius", "01103", 54.6872, 25.2797), ("LT", "5", "Pilies g.", "Vilnius", "01123", 54.6840, 25.2890)],
            &[("Gedimino prospektas 10, LT-01103 Vilnius", "Gedimino pr. 10"), ("Gedimino pr. 10", "Gedimino pr. 10"), ("Pilies gatvė 5 Vilnius", "Pilies g. 5")],
            ("LT-01103", "LT-01103"), ("Vilnius", "Vilnius"));
}

#[test]
fn luxembourg() {
    country("LU", &[("LU", "12", "Avenue de la Gare", "Luxembourg", "1611", 49.6050, 6.1300), ("LU", "3", "Grand-Rue", "Luxembourg", "1661", 49.6110, 6.1290)],
            &[("12, avenue de la Gare, L-1611 Luxembourg", "12 Avenue de la Gare"), ("12 av. de la Gare Luxembourg", "12 Avenue de la Gare"), ("Grand-Rue 3", "Grand-Rue 3")],
            ("L-1611", "L-1611"), ("Luxembourg", "Luxembourg"));
}

#[test]
fn latvia() {
    country("LV", &[("LV", "10", "Brīvības iela", "Rīga", "LV-1050", 56.9496, 24.1052), ("LV", "2", "Kaļķu iela", "Rīga", "LV-1050", 56.9490, 24.1100)],
            &[("Brīvības iela 10, Rīga, LV-1050", "Brīvības iela 10"), ("Brivibas iela 10 Riga", "Brīvības iela 10"), ("Kalku iela 2", "Kaļķu iela 2")],
            ("LV-1050", "LV-1050"), ("Riga", "Rīga"));
}

#[test]
fn malta() {
    country("MT", &[("MT", "10", "Triq ir-Repubblika", "Il-Belt Valletta", "VLT 1117", 35.8989, 14.5146), ("MT", "5", "Triq il-Kbira", "Ħal Qormi", "QRM 2450", 35.8760, 14.4720)],
            &[("10, Triq ir-Repubblika, Valletta VLT 1117", "10 Triq ir-Repubblika"), ("Triq ir-Repubblika 10", "10 Triq ir-Repubblika"), ("5 Triq il-Kbira, Hal Qormi", "5 Triq il-Kbira")],
            ("VLT 1117", "VLT 1117"), ("Valletta", "Il-Belt Valletta"));
}

#[test]
fn netherlands() {
    country("NL", &[("NL", "1", "Dam", "Amsterdam", "1012 JS", 52.3731, 4.8926), ("NL", "174", "Kalverstraat", "Amsterdam", "1012 XA", 52.3680, 4.8910),
                    ("NL", "22", "Laan van Meerdervoort", "Den Haag", "2517 AJ", 52.0790, 4.2950)],
            &[("Dam 1, 1012 JS Amsterdam", "Dam 1"), ("Kalverstr. 174 Amsterdam", "Kalverstraat 174"), ("Kalverstraat 174, 1012XA", "Kalverstraat 174"),
              ("Laan van Meerdervoort 22, Den Haag", "Laan van Meerdervoort 22")],
            ("1012 JS", "1012 JS"), ("Amsterdam", "Amsterdam"));
    let f = fx("addr-NL-part");
    index(&f, "NL", &[("NL", "1", "Dam", "Amsterdam", "1012 JS", 52.3731, 4.8926)]);
    assert_eq!(f.map.places("1012", 20, None)[0]["name"], "1012 JS", "the four digits of a Dutch postcode name its area");
}

#[test]
fn norway() {
    country("NO", &[("NO", "22", "Karl Johans gate", "Oslo", "0159", 59.9133, 10.7389), ("NO", "1", "Storgata", "Oslo", "0155", 59.9140, 10.7500)],
            &[("Karl Johans gt. 22, 0159 Oslo", "Karl Johans gate 22"), ("Karl Johans gate 22", "Karl Johans gate 22"), ("Storgata 1 Oslo", "Storgata 1")],
            ("0159", "0159"), ("Oslo", "Oslo"));
}

#[test]
fn poland() {
    country("PL-14", &[("PL", "1", "Marszałkowska", "Warszawa", "00-624", 52.2297, 21.0122), ("PL", "20", "Aleje Jerozolimskie", "Warszawa", "00-024", 52.2310, 21.0170),
                       ("PL", "5", "Plac Defilad", "Warszawa", "00-901", 52.2318, 21.0060)],
            &[("ul. Marszałkowska 1, 00-624 Warszawa", "Marszałkowska 1"), ("Marszalkowska 1 Warszawa", "Marszałkowska 1"), ("Marszałkowska 1/3", "Marszałkowska 1"),
              ("al. Jerozolimskie 20", "Aleje Jerozolimskie 20"), ("pl. Defilad 5", "Plac Defilad 5")],
            ("00-624", "00-624"), ("Warszawa", "Warszawa"));
}

#[test]
fn portugal() {
    country("PT", &[("PT", "100", "Rua Augusta", "Lisboa", "1100-053", 38.7100, -9.1370), ("PT", "1", "Praça do Comércio", "Lisboa", "1100-148", 38.7075, -9.1364),
                    ("PT", "50", "Avenida da Liberdade", "Lisboa", "1250-145", 38.7190, -9.1440)],
            &[("Rua Augusta, 100, 1100-053 Lisboa", "Rua Augusta, 100"), ("R. Augusta 100", "Rua Augusta, 100"), ("Pç. do Comércio 1", "Praça do Comércio, 1"),
              ("Av. da Liberdade 50 Lisboa", "Avenida da Liberdade, 50")],
            ("1100-053", "1100-053"), ("Lisboa", "Lisboa"));
}

#[test]
fn romania() {
    country("RO", &[("RO", "5", "Strada Lipscani", "București", "030031", 44.4310, 26.1000), ("RO", "10", "Bulevardul Unirii", "București", "030167", 44.4270, 26.1100)],
            &[("Strada Lipscani nr. 5, București", "Strada Lipscani 5"), ("Str. Lipscani 5 Bucuresti", "Strada Lipscani 5"), ("Bd. Unirii 10", "Bulevardul Unirii 10")],
            ("030031", "030031"), ("Bucuresti", "București"));
}

#[test]
fn sweden() {
    country("SE", &[("SE", "5", "Drottninggatan", "Stockholm", "111 51", 59.3326, 18.0649), ("SE", "20", "Sveavägen", "Stockholm", "111 57", 59.3360, 18.0620)],
            &[("Drottninggatan 5, 111 51 Stockholm", "Drottninggatan 5"), ("Sveavagen 20", "Sveavägen 20"), ("Sveavägen 20 lgh 1201 Stockholm", "Sveavägen 20")],
            ("111 51", "111 51"), ("Stockholm", "Stockholm"));
}

#[test]
fn slovenia() {
    country("SI", &[("SI", "10", "Slovenska cesta", "Ljubljana", "1000", 46.0511, 14.5051), ("SI", "1", "Prešernov trg", "Ljubljana", "1000", 46.0514, 14.5060)],
            &[("Slovenska c. 10, 1000 Ljubljana", "Slovenska cesta 10"), ("Presernov trg 1", "Prešernov trg 1")],
            ("1000", "1000"), ("Ljubljana", "Ljubljana"));
}

#[test]
fn slovakia() {
    country("SK", &[("SK", "1", "Hlavné námestie", "Bratislava", "811 01", 48.1435, 17.1090), ("SK", "10", "Obchodná", "Bratislava", "811 06", 48.1470, 17.1070)],
            &[("Hlavné nám. 1, 811 01 Bratislava", "Hlavné námestie 1"), ("Obchodna 10 Bratislava", "Obchodná 10"), ("ul. Obchodná 10", "Obchodná 10")],
            ("811 01", "811 01"), ("Bratislava", "Bratislava"));
}
