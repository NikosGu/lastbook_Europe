//! How addresses are written in each of the 31 countries: the languages of their street names, where the house number
//! goes, the shape of a postcode, and how the postcode and town are written after the street.
//!
//! Postcode shapes: `9` a digit, `A` a letter, `X` a letter or a digit, a space where one may be written or left out,
//! anything else as itself. A country's prefixes may stand before its postcode ("L-1611", "FL-9490", "D-80331").

/// Where the house number goes in the way addresses are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    /// Via Roma 12, Hauptstraße 5, Calle Mayor, 5.
    StreetFirst,
    /// 12 rue de la Paix, 12 Main Street.
    NumberFirst,
    /// By the street's language: French street names number first (Belgium, Luxembourg), the others after.
    ByLanguage,
}

pub struct Country {
    pub code: &'static str,
    /// Languages of its street names, the most common first: on a word two of them read differently, the first wins.
    pub langs: &'static [&'static str],
    pub order: Order,
    /// "Calle Mayor, 5": a comma between street and number.
    pub comma: bool,
    pub postcode: &'static [&'static str],
    /// Shapes of a part of a postcode that names an area of its own: NL 1012, PT 1000, the Eircode routing key D02.
    pub partial: &'static [&'static str],
    pub prefixes: &'static [&'static str],
    /// Written before the postcode's digits when shown: L-1611, LV-1050, LT-01100.
    pub shown_prefix: &'static str,
    /// The town before the postcode: Dublin 2, D02 X285; Valletta VLT 1117; Rīga, LV-1050.
    pub town_first: bool,
}

const fn c(code: &'static str, langs: &'static [&'static str], order: Order, postcode: &'static [&'static str], prefixes: &'static [&'static str]) -> Country {
    Country { code, langs, order, comma: false, postcode, partial: &[], prefixes, shown_prefix: "", town_first: false }
}

use Order::*;

pub const COUNTRIES: &[Country] = &[
    c("AT", &["de"], StreetFirst, &["9999"], &["A-", "AT-"]),
    c("BE", &["nl", "fr", "de"], ByLanguage, &["9999"], &["B-", "BE-"]),
    c("BG", &["bg"], StreetFirst, &["9999"], &["BG-"]),
    c("CH", &["de", "fr", "it", "rm"], StreetFirst, &["9999"], &["CH-"]),
    c("CY", &["el", "en", "tr"], StreetFirst, &["9999"], &["CY-"]),
    c("CZ", &["cs"], StreetFirst, &["999 99"], &["CZ-"]),
    c("DE", &["de"], StreetFirst, &["99999"], &["D-", "DE-"]),
    c("DK", &["da"], StreetFirst, &["9999"], &["DK-"]),
    c("EE", &["et"], StreetFirst, &["99999"], &["EE-"]),
    Country { comma: true, ..c("ES", &["es", "ca", "gl", "eu"], StreetFirst, &["99999"], &["E-", "ES-"]) },
    c("FI", &["fi", "sv"], StreetFirst, &["99999"], &["FI-", "FIN-"]),
    c("FR", &["fr"], NumberFirst, &["99999"], &["F-", "FR-"]),
    c("GR", &["el"], StreetFirst, &["999 99"], &["GR-"]),
    c("HR", &["hr"], StreetFirst, &["99999"], &["HR-"]),
    c("HU", &["hu"], StreetFirst, &["9999"], &["H-", "HU-"]),
    Country { partial: &["A99", "D6W"], town_first: true, ..c("IE", &["en", "ga"], NumberFirst, &["A99 XXXX", "D6W XXXX"], &[]) },
    c("IS", &["is"], StreetFirst, &["999"], &["IS-"]),
    c("IT", &["it", "de", "fr"], StreetFirst, &["99999"], &["I-", "IT-"]),
    c("LI", &["de"], StreetFirst, &["9999"], &["FL-", "LI-"]),
    Country { shown_prefix: "LT-", ..c("LT", &["lt"], StreetFirst, &["99999"], &["LT-"]) },
    Country { shown_prefix: "L-", ..c("LU", &["fr", "de", "lb"], ByLanguage, &["9999"], &["L-", "LU-"]) },
    Country { shown_prefix: "LV-", town_first: true, ..c("LV", &["lv"], StreetFirst, &["9999"], &["LV-"]) },
    Country { town_first: true, ..c("MT", &["mt", "en"], NumberFirst, &["AAA 9999"], &[]) },
    Country { partial: &["9999"], ..c("NL", &["nl"], StreetFirst, &["9999 AA"], &["NL-"]) },
    c("NO", &["no"], StreetFirst, &["9999"], &["N-", "NO-"]),
    c("PL", &["pl"], StreetFirst, &["99-999"], &["PL-"]),
    Country { comma: true, partial: &["9999"], ..c("PT", &["pt"], StreetFirst, &["9999-999"], &["P-", "PT-"]) },
    c("RO", &["ro"], StreetFirst, &["999999"], &["RO-"]),
    c("SE", &["sv"], StreetFirst, &["999 99"], &["S-", "SE-"]),
    c("SI", &["sl"], StreetFirst, &["9999"], &["SI-"]),
    c("SK", &["sk"], StreetFirst, &["999 99"], &["SK-"]),
];

pub fn get(code: &str) -> Option<&'static Country> {
    COUNTRIES.iter().find(|c| c.code == code)
}

/// Does `s` (upper case) have this shape?
pub fn shape(s: &str, pat: &str) -> bool {
    let s: Vec<char> = s.chars().collect();
    let mut i = 0;
    for p in pat.chars() {
        if p == ' ' {
            if s.get(i) == Some(&' ') {
                i += 1;
            }
            continue;
        }
        let Some(&c) = s.get(i) else { return false };
        let ok = match p {
            '9' => c.is_ascii_digit(),
            'A' => c.is_ascii_uppercase(),
            'X' => c.is_ascii_uppercase() || c.is_ascii_digit(),
            p => c == p,
        };
        if !ok {
            return false;
        }
        i += 1;
    }
    i == s.len()
}

/// A postcode as the key the index files it under: upper case, the country prefix, spaces and dashes taken out
/// ("L-1611" 1611, "1012 AB" 1012AB, "00-950" 00950, "D02 X285" D02X285).
pub fn postcode_key(pc: &str, country: &str) -> String {
    let mut u = pc.trim().to_uppercase();
    if let Some(c) = get(country) {
        if let Some(p) = c.prefixes.iter().find(|p| u.starts_with(*p) && u.len() > p.len()) {
            u = u[p.len()..].to_string();
        }
    }
    u.chars().filter(|c| !c.is_whitespace() && *c != '-').collect()
}

/// A postcode as people of its country write it, from the source's spelling: LU, LV and LT with their prefix.
pub fn postcode_shown(pc: &str, country: &str) -> String {
    match get(country) {
        Some(c) if !c.shown_prefix.is_empty() && pc.chars().next().is_some_and(|ch| ch.is_ascii_digit()) => format!("{}{pc}", c.shown_prefix),
        _ => pc.to_string(),
    }
}

/// Where a postcode as typed could be from, with its key. A country prefix names the country; otherwise every country
/// whose shape fits. `partial` marks a part of a postcode (prefix match).
pub fn postcode_candidates(typed: &str) -> Vec<(&'static str, String, bool)> {
    let u = typed.trim().to_uppercase();
    let mut out = Vec::new();
    for c in COUNTRIES {
        let (rest, prefixed) = match c.prefixes.iter().find(|p| u.starts_with(*p) && u.len() > p.len()) {
            Some(p) => (&u[p.len()..], true),
            None => (u.as_str(), false),
        };
        if !prefixed && u.chars().take(3).any(|ch| ch == '-') && !c.postcode.iter().any(|s| s.contains('-')) {
            continue; // "L-1611" is Luxembourg's, not a shape with a dash elsewhere
        }
        let key: String = rest.chars().filter(|ch| !ch.is_whitespace() && *ch != '-').collect();
        if c.code == "LI" && !key.starts_with("94") {
            continue; // Liechtenstein's postcodes are 9485 to 9498
        }
        if c.postcode.iter().any(|s| shape(rest, s)) {
            out.push((c.code, key, false));
        } else if c.partial.iter().any(|s| shape(rest, s)) {
            out.push((c.code, key, true));
        }
    }
    // a prefix names the country: drop the shapes that only fit without it
    if COUNTRIES.iter().any(|c| c.prefixes.iter().any(|p| u.starts_with(*p) && u.len() > p.len() && out.iter().any(|o| o.0 == c.code))) {
        out.retain(|o| get(o.0).is_some_and(|c| c.prefixes.iter().any(|p| u.starts_with(*p))));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_country_once() {
        assert_eq!(COUNTRIES.len(), 31);
        for (code, _, _) in crate::regions::COUNTRIES {
            assert!(get(code).is_some(), "{code}");
        }
        for c in COUNTRIES {
            for l in c.langs {
                assert!(super::super::words::LANGS.iter().any(|x| x.code == *l), "{} {l}", c.code);
            }
        }
    }

    #[test]
    fn postcode_shapes() {
        let has = |typed: &str, c: &str| postcode_candidates(typed).iter().any(|o| o.0 == c);
        for (typed, country, key) in [("9490", "LI", "9490"), ("FL-9490", "LI", "9490"), ("1010", "AT", "1010"), ("80331", "DE", "80331"),
            ("D-80331", "DE", "80331"), ("75002", "FR", "75002"), ("00184", "IT", "00184"), ("28013", "ES", "28013"), ("1012 AB", "NL", "1012AB"),
            ("1012AB", "NL", "1012AB"), ("00-950", "PL", "00950"), ("1000-001", "PT", "1000001"), ("110 00", "CZ", "11000"), ("11000", "CZ", "11000"),
            ("811 01", "SK", "81101"), ("114 55", "SE", "11455"), ("105 57", "GR", "10557"), ("VLT 1117", "MT", "VLT1117"), ("VLT1117", "MT", "VLT1117"),
            ("D02 X285", "IE", "D02X285"), ("A65 F4E2", "IE", "A65F4E2"), ("L-1611", "LU", "1611"), ("1611", "LU", "1611"), ("LV-1050", "LV", "1050"),
            ("LT-01100", "LT", "01100"), ("010011", "RO", "010011"), ("101", "IS", "101"), ("1000", "BG", "1000"), ("1050", "DK", "1050"),
            ("10111", "EE", "10111"), ("00100", "FI", "00100"), ("10000", "HR", "10000"), ("1052", "HU", "1052"), ("0150", "NO", "0150"),
            ("1000", "SI", "1000"), ("1010", "CY", "1010"), ("8001", "CH", "8001"), ("1000", "BE", "1000")] {
            let found = postcode_candidates(typed);
            assert!(found.iter().any(|o| o.0 == country && o.1 == key), "{typed} {country}: {found:?}");
            assert_eq!(postcode_key(typed, country), key, "{typed}");
        }
        assert!(has("9490", "AT") && has("9490", "LU"), "a bare 9490 may be any country with four digits");
        assert!(!has("1234", "LI"), "not a Liechtenstein postcode");
        assert!(!has("L-1611", "AT") && !has("FL-9490", "AT"), "a prefix names the country");
        assert!(!has("12", "DE") && !has("Vaduz", "LI"));
        assert!(has("1012", "NL") && postcode_candidates("1012").iter().any(|o| o.0 == "NL" && o.2), "a part of a Dutch postcode");
        assert!(postcode_candidates("D02").iter().any(|o| o.0 == "IE" && o.2), "an Eircode routing key");
        assert_eq!(postcode_shown("1611", "LU"), "L-1611");
        assert_eq!(postcode_shown("LV-1050", "LV"), "LV-1050");
        assert_eq!(postcode_shown("9490", "LI"), "9490");
    }
}
