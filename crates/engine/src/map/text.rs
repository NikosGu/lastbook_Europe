//! The keys place and address search compare text by. What a person types and what a map file holds go through the same
//! functions, so both spellings of a name meet: Straße and Strasse, München, Munchen and Muenchen, Łódź and Lodz. The
//! builders of the map files call these too. A key is never shown: display text is never changed.

use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

/// The key two texts are compared by, as `fold()` in web/words.js: lower case, decomposed (NFKD), accent marks on Latin
/// letters removed, composed again, final ς as σ. Two additions for the map: accents on Greek letters go too (Greek
/// capitals are written without them), and the letters that have no accent to remove are written out: ß ss, æ ae, œ oe,
/// ø o, ł l, đ and ð d, þ th, ħ h, ı i, ŧ t, ŋ n.
pub fn fold(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut strip = false; // the last letter was Latin or Greek: its marks go
    for c in lower.nfkd() {
        if is_combining_mark(c) {
            if !strip {
                out.push(c);
            }
            continue;
        }
        strip = is_latin(c) || is_greek(c);
        match c {
            'ß' => out.push_str("ss"),
            'æ' => out.push_str("ae"),
            'œ' => out.push_str("oe"),
            'þ' => out.push_str("th"),
            'ø' => out.push('o'),
            'ł' => out.push('l'),
            'đ' | 'ð' => out.push('d'),
            'ħ' => out.push('h'),
            'ı' => out.push('i'),
            'ŧ' => out.push('t'),
            'ŋ' => out.push('n'),
            'ς' => out.push('σ'),
            c => out.push(c),
        }
    }
    out.nfc().collect()
}

fn is_latin(c: char) -> bool {
    c.is_ascii_alphabetic()
        || matches!(c, '\u{aa}' | '\u{ba}' | '\u{c0}'..='\u{d6}' | '\u{d8}'..='\u{f6}' | '\u{f8}'..='\u{24f}' | '\u{250}'..='\u{2af}'
            | '\u{1d00}'..='\u{1dbf}' | '\u{1e00}'..='\u{1eff}' | '\u{2c60}'..='\u{2c7f}' | '\u{a720}'..='\u{a7ff}' | '\u{ff21}'..='\u{ff3a}' | '\u{ff41}'..='\u{ff5a}')
}

fn is_greek(c: char) -> bool {
    matches!(c, '\u{370}'..='\u{3ff}' | '\u{1f00}'..='\u{1fff}')
}

/// [`fold`], then the second way of writing a letter brought back to the first, so that both spellings give one key:
/// ae, oe and ue as a, o and u (Müller and Mueller, Tromsø and Tromsoe), aa as a (Ålborg and Aalborg), dj as d (Đakovo
/// and Djakovo). Left to right, one pair at a time, so the key of the start of a word is the start of the word's key.
pub fn loose(s: &str) -> String {
    let f = fold(s);
    let c: Vec<char> = f.chars().collect();
    let mut out = String::with_capacity(f.len());
    let mut i = 0;
    while i < c.len() {
        out.push(c[i]);
        if i + 1 < c.len() && matches!((c[i], c[i + 1]), ('a' | 'o' | 'u', 'e') | ('a', 'a') | ('d', 'j')) {
            i += 2;
        } else {
            i += 1;
        }
    }
    out
}

/// The words of a text, each [`loose`]: anything that is not a letter, mark or digit separates words, as in web/words.js.
pub fn words(s: &str) -> Vec<String> {
    split(&loose(s))
}

/// Splits a text already folded into its words.
pub fn split(s: &str) -> Vec<String> {
    s.split(|c: char| !(c.is_alphanumeric() || is_combining_mark(c))).filter(|w| !w.is_empty()).map(str::to_string).collect()
}

/// Titles written out or shortened, one key for both: Saint, Sankt and St; Sainte and Ste.
fn title(w: &str) -> &str {
    match w {
        "saint" | "sankt" | "sint" | "st" => "st",
        "sainte" | "ste" => "ste",
        _ => w,
    }
}

/// The key of a place's name: its words, [`loose`], with Saint, Sankt and St as one. Ranking compares these.
pub fn name_key(s: &str) -> String {
    words(s).iter().map(|w| title(w)).collect::<Vec<_>>().join(" ")
}

/// The search key a builder writes for a place (the `key` column of places.sqlite and water.sqlite): every word of every
/// name it has, once, as [`name_key`] makes them. A postcode's names are its own and its postcode key ("1012 AB" and
/// "1012AB").
pub fn place_key(names: &[&str]) -> String {
    let mut out: Vec<String> = Vec::new();
    for n in names {
        for w in words(n) {
            let w = title(&w).to_string();
            if !out.contains(&w) {
                out.push(w);
            }
        }
    }
    out.join(" ")
}

/// Ways an older index, tokenized by SQLite's unicode61 alone (accents gone, ß and ł kept), may hold a word: as typed, as
/// folded, as loosened, with ä ö ü written ae oe ue, and with ss as ß.
pub fn variants(w: &str) -> Vec<String> {
    let lower = w.to_lowercase();
    let mut out = vec![lower.clone(), fold(w), loose(w)];
    out.push(fold(&lower.replace('ä', "ae").replace('ö', "oe").replace('ü', "ue").replace('ø', "oe").replace('å', "aa")));
    if lower.contains("ss") {
        out.push(lower.replace("ss", "ß"));
    }
    let mut seen = Vec::new();
    for v in out {
        if !v.is_empty() && !seen.contains(&v) {
            seen.push(v);
        }
    }
    seen
}

/// An FTS5 prefix term for one word: quoted, so nothing typed is read as query syntax.
pub fn fts_term(w: &str) -> String {
    format!("\"{}\"*", w.replace('"', ""))
}

/// `1,234`: thousands separated as the original Lastbook wrote them.
pub fn thousands(n: i64) -> String {
    let s = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 {
        format!("-{out}")
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_as_words_js() {
        assert_eq!(fold("Éire"), "eire");
        assert_eq!(fold("İstanbul"), "istanbul");
        assert_eq!(fold("ＷＡＴＥＲ"), "water");
        assert_eq!(fold("Straße"), "strasse");
        assert_eq!(fold("STRASSE"), "strasse");
        assert_eq!(fold("Łódź"), "lodz");
        assert_eq!(fold("Tromsø"), "tromso");
        assert_eq!(fold("Næstved"), "naestved");
        assert_eq!(fold("Þingvellir"), "thingvellir");
        assert_eq!(fold("Đakovo"), "dakovo");
        assert_eq!(fold("Ħal Qormi"), "hal qormi");
        assert_eq!(fold("Ploiești"), "ploiesti");
        assert_eq!(fold("Αθήνα"), "αθηνα");
        assert_eq!(fold("ΑΘΗΝΑ"), "αθηνα");
        assert_eq!(fold("Πειραιάς"), "πειραιασ");
        assert_eq!(fold("Пловдив"), "пловдив");
        assert_eq!(fold("Бургас й"), "бургас й", "Cyrillic keeps its marks, as in words.js");
    }

    #[test]
    fn both_spellings_meet() {
        for (a, b) in [("München", "Muenchen"), ("München", "Munchen"), ("Müller", "Mueller"), ("Straße", "Strasse"), ("Tromsø", "Tromsoe"),
                       ("Ålborg", "Aalborg"), ("Göteborg", "Goeteborg"), ("Næstved", "Nastved"), ("Đakovo", "Djakovo"), ("Wrocław", "Wroclaw"),
                       ("Zürich", "Zuerich"), ("Cœur", "Coeur"), ("Saint-Étienne", "SAINT ETIENNE")] {
            assert_eq!(words(a), words(b), "{a} {b}");
        }
        assert_eq!(loose("Muester"), "muster", "the price of meeting both spellings: ue is u everywhere");
        assert_ne!(loose("Bern"), loose("Born"));
        // the key of the start of a word starts the word's key
        for (part, whole) in [("Mue", "Mueller"), ("Mü", "Müller"), ("Aa", "Aalborg"), ("Gö", "Goethe"), ("Stra", "Straße")] {
            assert!(loose(whole).starts_with(&loose(part)), "{part} {whole}");
        }
        assert_eq!(name_key("St. Gallen"), name_key("Sankt Gallen"));
        assert_eq!(name_key("Saint-Étienne"), "st etienne");
        assert_eq!(place_key(&["1012 AB", "1012AB"]), "1012 ab 1012ab");
        assert_eq!(words("l'Église, 12/3"), vec!["l", "eglise", "12", "3"]);
    }

    #[test]
    fn variants_for_older_indexes() {
        let v = variants("Strasse");
        assert!(v.contains(&"straße".to_string()) && v.contains(&"strasse".to_string()));
        assert!(variants("Müller").contains(&"mueller".to_string()));
        assert!(variants("Muenchen").contains(&"munchen".to_string()));
        assert_eq!(fts_term("a\"b"), "\"ab\"*");
        assert_eq!(thousands(1234567), "1,234,567");
        assert_eq!(thousands(12), "12");
    }
}
