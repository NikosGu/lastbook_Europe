//! European address search, in the address indexes installed with region maps (`derived/geo/addresses/<REGION>.sqlite`,
//! docs/ADDRESSES.md). It replaces the original Lastbook's American one (house number first, ZIP, state abbreviations).
//!
//! A query is read every way it can be: the house number before or after the street ("12 rue de la Paix", "Via Roma 12",
//! "Calle Mayor, 5"), a postcode in the shape of any of the 31 countries ("9490", "1012 AB", "00-950", "D02 X285"), the
//! town before or after, commas or none, a country's name at the end. Each reading is looked up in turn, the most precise
//! first, and the first that finds addresses gives the answer. Only exact house numbers are returned: never a guessed
//! house. Every word the person typed stays in the reading, as street or town, so a wrong town finds nothing rather than
//! the same street somewhere else.
//!
//! Streets are compared by [`street_key`], built by the address's own country's rules ([`countries`], [`words`]); the
//! index's builder files every address under the same key.

pub mod countries;
pub mod words;

use self::countries::Order;
use super::text::{fold, loose, name_key, split};
use super::Map;
use crate::regions;
use rusqlite::types::Value as Sql;
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::OnceLock;

/// The tables of an address index. One row per address; an address with two numbers (Czech and Slovak 846/1) may be
/// filed once under each (846/1, 846 and 1).
pub const SCHEMA: &str = "CREATE TABLE addr(num TEXT NOT NULL, street TEXT NOT NULL, norm TEXT NOT NULL, city TEXT NOT NULL DEFAULT '',
        state TEXT NOT NULL DEFAULT '', zip TEXT NOT NULL DEFAULT '', lat REAL NOT NULL, lon REAL NOT NULL, country TEXT NOT NULL,
        postcode TEXT NOT NULL DEFAULT '', numkey TEXT NOT NULL, citykey TEXT NOT NULL DEFAULT '');
    CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT);";

/// Run once every address is in: the indexes the search reads, and `area`, one row per postcode and town with its
/// middle, for a postcode or a town searched alone.
pub const FINISH: &str = "CREATE INDEX addr_num_norm ON addr(numkey, norm);
    CREATE INDEX addr_norm ON addr(norm, numkey);
    CREATE TABLE area(country TEXT NOT NULL, zip TEXT NOT NULL, postcode TEXT NOT NULL, citykey TEXT NOT NULL, city TEXT NOT NULL,
        lat REAL NOT NULL, lon REAL NOT NULL, n INTEGER NOT NULL);
    INSERT INTO area SELECT country, zip, MAX(postcode), citykey, MAX(city), AVG(lat), AVG(lon), COUNT(*) FROM addr GROUP BY country, zip, citykey;
    CREATE INDEX area_zip ON area(zip);
    CREATE INDEX area_city ON area(citykey);";

/// One address as a source gives it, for a builder: [`Address::insert`] works out its keys.
pub struct Address<'a> {
    pub num: &'a str,
    pub street: &'a str,
    pub city: &'a str,
    pub state: &'a str,
    pub postcode: &'a str,
    /// ISO 3166-1 alpha-2.
    pub country: &'a str,
    pub lat: f64,
    pub lon: f64,
}

impl Address<'_> {
    pub fn insert(&self, c: &Connection) -> rusqlite::Result<()> {
        c.execute(
            "INSERT INTO addr VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            rusqlite::params![self.num, self.street, street_key(self.street, self.country), self.city, self.state,
                countries::postcode_key(self.postcode, self.country), self.lat, self.lon, self.country, self.postcode, number_key(self.num), name_key(self.city)],
        )?;
        Ok(())
    }
}

/// A country's street words, folded and loosened, longest suffix first.
struct Table {
    words: HashMap<String, String>,
    first: HashMap<String, String>,
    last: HashMap<String, String>,
    suffixes: Vec<(String, String)>,
    particles: HashSet<String>,
}

fn build(langs: &[&str]) -> Table {
    let mut t = Table { words: HashMap::new(), first: HashMap::new(), last: HashMap::new(), suffixes: Vec::new(), particles: HashSet::new() };
    let key = |s: &str| loose(s).replace('.', "");
    for code in langs {
        let Some(l) = words::LANGS.iter().find(|l| l.code == *code) else { continue };
        for (map, list) in [(&mut t.words, l.words), (&mut t.first, l.first), (&mut t.last, l.last)] {
            for (k, v) in list {
                map.entry(key(k)).or_insert_with(|| loose(v));
            }
        }
        for (k, v) in l.suffixes {
            if !t.suffixes.iter().any(|(s, _)| *s == key(k)) {
                t.suffixes.push((key(k), loose(v)));
            }
        }
        t.particles.extend(l.particles.iter().map(|p| key(p)));
    }
    t.suffixes.sort_by_key(|(s, _)| std::cmp::Reverse(s.len()));
    t
}

fn table(country: &str) -> &'static Table {
    static TABLES: OnceLock<HashMap<&'static str, Table>> = OnceLock::new();
    static NONE: OnceLock<Table> = OnceLock::new();
    TABLES.get_or_init(|| countries::COUNTRIES.iter().map(|c| (c.code, build(c.langs))).collect()).get(country).unwrap_or_else(|| NONE.get_or_init(|| build(&[])))
}

fn titles() -> &'static HashMap<String, String> {
    static T: OnceLock<HashMap<String, String>> = OnceLock::new();
    T.get_or_init(|| words::TITLES.iter().map(|(k, v)| (loose(k), loose(v))).collect())
}

/// Every country's street words together, to tell whether a query names a street at all: `true` for the word every
/// street has (Straße, rue, gatan), `false` for the others.
fn any_street_word() -> &'static (HashMap<String, bool>, Vec<String>) {
    static T: OnceLock<(HashMap<String, bool>, Vec<String>)> = OnceLock::new();
    T.get_or_init(|| {
        let mut words = HashMap::new();
        let mut generic_suffixes = Vec::new();
        for l in words::LANGS {
            for (k, v) in l.words.iter().chain(l.first.iter().filter(|(k, _)| k.chars().count() > 1)).chain(l.last.iter().filter(|(k, _)| k.chars().count() > 1)) {
                let e = words.entry(loose(k).replace('.', "")).or_insert(false);
                *e |= v.is_empty();
                if !v.is_empty() && !v.contains(' ') {
                    words.entry(loose(v)).or_insert(false);
                }
            }
            for (k, v) in l.suffixes {
                words.entry(loose(k)).or_insert(false);
                if v.is_empty() {
                    generic_suffixes.push(loose(k));
                }
            }
        }
        (words, generic_suffixes)
    })
}

/// Does this word name a street: a street word, or a compound ending in the word every street has (Bahnhofstraße)?
fn street_word(f: &str) -> bool {
    let (words, generic) = any_street_word();
    let l = loose(f).replace('.', "");
    words.contains_key(&l) || generic.iter().any(|s| l.len() > s.len() + 1 && l.ends_with(s.as_str()))
}

/// The key a street is compared by, by its country's rules: [`loose`] words, abbreviations written out (Str. Straße,
/// v.le viale, C/ calle, ul. ulica), the word every street has dropped, the small words of Romance names dropped,
/// compounds split (Bahnhofstraße and Bahnhofstr. "bahnhof", Sveavägen "svea vag"), titles as one (Sankt and St).
pub fn street_key(street: &str, country: &str) -> String {
    let t = table(country);
    let mut subs: Vec<String> = Vec::new();
    for raw in raw_words(street) {
        let l = loose(&raw);
        let compact: String = l.chars().filter(|c| !matches!(c, '.' | '/' | '\'' | '’')).collect();
        if compact != l && !compact.is_empty() && (t.words.contains_key(&compact) || t.first.contains_key(&compact) || t.last.contains_key(&compact) || titles().contains_key(&compact)) {
            subs.push(compact);
        } else {
            subs.extend(split(&l));
        }
    }
    let n = subs.len();
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: &str| {
        if !s.is_empty() {
            out.push(s.to_string());
        }
    };
    for (i, s) in subs.iter().enumerate() {
        if let Some(v) = (i == 0).then(|| t.first.get(s)).flatten().or_else(|| (i + 1 == n).then(|| t.last.get(s)).flatten()) {
            push(v);
        } else if let Some(v) = t.words.get(s).or_else(|| titles().get(s)) {
            push(v);
        } else if n > 1 && t.particles.contains(s) {
            continue;
        } else if let Some((suf, v)) = t.suffixes.iter().find(|(suf, _)| s.len() > suf.len() + 1 && s.ends_with(suf.as_str())) {
            push(&s[..s.len() - suf.len()]);
            push(v);
        } else {
            push(s);
        }
    }
    if out.is_empty() {
        subs.join(" ")
    } else {
        out.join(" ")
    }
}

/// A house number as the index files it: lower case, no spaces, no final dot, a letter after a slash or dash joined
/// ("12 bis" 12bis, "12 A" 12a, "12/A" 12a, "10." 10; "12/3" and "12-14" stay).
pub fn number_key(num: &str) -> String {
    let mut s: String = fold(num).chars().filter(|c| !c.is_whitespace()).collect();
    while s.ends_with('.') {
        s.pop();
    }
    if let Some(i) = s.rfind(['/', '-']) {
        let (a, b) = (&s[..i], &s[i + 1..]);
        if !a.is_empty() && a.chars().all(|c| c.is_ascii_digit()) && b.chars().count() == 1 && b.chars().all(|c| c.is_alphabetic()) {
            return format!("{a}{b}");
        }
    }
    s
}

/// The numbers a house number may be filed under, the exact one first: a flat or a staircase after a slash, or the
/// end of a range, is the same building (12/3/14, 12/3, 12; 12-14, 12). A letter is another house: 12a is never 12.
fn number_forms(num: &str) -> Vec<String> {
    let k = number_key(num);
    let mut out = vec![k.clone()];
    let mut s = k.clone();
    while let Some(i) = s.rfind('/') {
        s.truncate(i);
        if !s.is_empty() && !out.contains(&s) {
            out.push(s.clone());
        }
    }
    if let Some((a, b)) = k.split_once('-') {
        if !a.is_empty() && a.chars().all(|c| c.is_ascii_digit()) && b.chars().next().is_some_and(|c| c.is_ascii_digit()) && !out.contains(&a.to_string()) {
            out.push(a.to_string());
        }
    }
    out
}

/// What a query asks for, as the engine reads it: an address (a street, with or without a house number), a postcode
/// (with or without its town), or a place by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Address,
    Postcode,
    Place,
}

impl Kind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Kind::Address => "address",
            Kind::Postcode => "postcode",
            Kind::Place => "place",
        }
    }
}

/// A postcode as typed, with every country whose shape it fits.
#[derive(Debug, Clone, PartialEq)]
pub struct Postcode {
    pub key: String,
    pub countries: Vec<&'static str>,
    /// A part of a postcode (NL 1012, PT 1000, IE D02): matched as the start of the key.
    pub partial: bool,
    start: usize,
    len: usize,
}

/// One way of reading a query.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub number: Option<String>,
    pub street: String,
    pub town: String,
    pub postcode: Option<Postcode>,
    /// A country named at the end of the query, read as such.
    pub country: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct Query {
    pub kind: Kind,
    /// With a house number, the most precise first.
    pub addresses: Vec<Reading>,
    /// A street without a number ("Bahnhofstrasse Zürich").
    pub streets: Vec<Reading>,
    /// A postcode or a town alone (no street).
    pub areas: Vec<Reading>,
    /// A country named at the end ("…, Liechtenstein", "…, LI"). After a comma it is only the country; without one, the
    /// query is also read as if it were part of a name (Corso Italia).
    pub country: Option<&'static str>,
}

#[derive(Debug, Clone)]
struct Tok {
    raw: String,
    f: String,
    seg: usize,
    num: bool,
    word: bool,
}

/// The words of a street name as written: split at spaces, and "C/Mayor" as "C/" and "Mayor".
fn raw_words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    for w in s.split_whitespace() {
        let w = w.trim_matches(|c: char| "()[]\"«»„“”".contains(c));
        match w.split_once('/') {
            Some((a, b)) if a.chars().count() == 1 && a.chars().all(char::is_alphabetic) && !b.is_empty() && !b.chars().next().is_some_and(|c| c.is_ascii_digit()) => {
                out.push(format!("{a}/"));
                out.push(b.to_string());
            }
            _ if !w.is_empty() => out.push(w.to_string()),
            _ => {}
        }
    }
    out
}

/// Looks like a house number: 12, 12a, 12bis, 12/3, 12-14, 12/3/14, 12/A, 10.
fn is_number(f: &str) -> bool {
    let digits = f.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 || digits > 5 {
        return false;
    }
    let rest = &f[digits..];
    if rest.is_empty() || rest == "." || matches!(rest, "bis" | "ter" | "quater") {
        return true;
    }
    if rest.chars().count() <= 2 && rest.chars().all(|c| c.is_alphabetic()) {
        return true;
    }
    if let Some(t) = rest.strip_prefix(['/', '-']) {
        if t.chars().count() == 1 && t.chars().all(char::is_alphabetic) {
            return true;
        }
        return t.split(['/', '-']).all(|p| {
            let d = p.chars().take_while(|c| c.is_ascii_digit()).count();
            (1..=5).contains(&d) && p[d..].chars().count() <= 1 && p[d..].chars().all(char::is_alphabetic)
        });
    }
    false
}

fn compact(f: &str) -> String {
    f.chars().filter(|c| !matches!(c, '.' | '°' | 'º')).collect()
}

/// A floor written after a house number: 3º, 2ª, 1.°
fn ordinal(raw: &str) -> bool {
    raw.chars().next().is_some_and(|c| c.is_ascii_digit()) && raw.ends_with(['º', 'ª', '°'])
}

fn tokens(q: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    for (seg, part) in q.split([',', ';']).enumerate() {
        for raw in raw_words(part) {
            let f = fold(&raw);
            let num = is_number(&f) && !ordinal(&raw);
            let word = !num && f.chars().any(char::is_alphabetic);
            out.push(Tok { raw, f, seg, num, word });
        }
    }
    // "Nr. 5", "n° 5": the mark is not part of the street
    let marks = |t: &Tok| words::NUMBER_MARKS.contains(&compact(&t.f).as_str());
    let mut kept: Vec<Tok> = Vec::new();
    for i in 0..out.len() {
        if marks(&out[i]) && out.get(i + 1).is_some_and(|n| n.num) && i > 0 {
            continue;
        }
        kept.push(out[i].clone());
    }
    kept.truncate(16);
    kept
}

/// Country names (local and English) and codes, folded, for a country named at the end of a query.
fn country_names() -> &'static Vec<(Vec<String>, &'static str)> {
    static N: OnceLock<Vec<(Vec<String>, &'static str)>> = OnceLock::new();
    N.get_or_init(|| {
        let mut out = Vec::new();
        for (code, local, en) in regions::COUNTRIES {
            for name in local.split(" / ").chain([*en]) {
                out.push((split(&loose(name)), *code));
            }
        }
        out.sort_by_key(|(w, _)| std::cmp::Reverse(w.len()));
        out
    })
}

/// A country named by the last words: how many words, which country, and whether they stand alone after a comma.
fn country_at_end(toks: &[Tok]) -> Option<(usize, &'static str, bool)> {
    let last = toks.last()?;
    let tail: Vec<&Tok> = toks.iter().filter(|t| t.seg == last.seg).collect();
    let code = (last.raw.len() == 2 && last.raw.chars().all(|c| c.is_ascii_uppercase())).then(|| countries::get(&last.raw).map(|c| c.code)).flatten();
    let (cut, c) = match code {
        Some(c) => (1, c),
        None => {
            let (w, c) = country_names().iter().find(|(w, _)| w.len() <= tail.len() && tail[tail.len() - w.len()..].iter().zip(w).all(|(t, n)| loose(&t.raw) == *n))?;
            (w.len(), *c)
        }
    };
    (cut < toks.len()).then_some((cut, c, cut == tail.len() && last.seg > 0))
}

/// Reads a query every way it can be read (see the module's notes).
pub fn parse(q: &str) -> Query {
    let toks = tokens(q);
    let Some((cut, code, alone)) = country_at_end(&toks) else { return read(toks, None) };
    let named = read(toks[..toks.len() - cut].to_vec(), Some(code));
    if alone {
        return named;
    }
    let mut q = read(toks, None);
    q.addresses.extend(named.addresses);
    q.streets.extend(named.streets);
    q.areas.extend(named.areas);
    if q.kind == Kind::Place {
        q.kind = named.kind;
    }
    q.country = Some(code);
    q
}

fn read(toks: Vec<Tok>, country: Option<&'static str>) -> Query {
    let n = toks.len();
    // postcodes: one word or two ("1012 AB", "VLT 1117", "D02 X285")
    let mut pcs: Vec<Postcode> = Vec::new();
    for i in 0..n {
        for len in [2, 1] {
            if i + len > n || toks[i..i + len].iter().any(|t| t.seg != toks[i].seg) {
                continue;
            }
            let typed = toks[i..i + len].iter().map(|t| t.raw.as_str()).collect::<Vec<_>>().join(" ");
            let mut by: Vec<(String, bool, Vec<&'static str>)> = Vec::new();
            for (c, key, partial) in countries::postcode_candidates(&typed) {
                if country.is_some_and(|k| k != c) {
                    continue;
                }
                match by.iter_mut().find(|b| b.0 == key && b.1 == partial) {
                    Some(b) => b.2.push(c),
                    None => by.push((key, partial, vec![c])),
                }
            }
            for (key, partial, countries) in by {
                pcs.push(Postcode { key, countries, partial, start: i, len });
            }
        }
    }
    // house numbers, and a letter or bis written apart: "12 A", "12 bis"
    let mut nums: Vec<(usize, usize, String)> = Vec::new();
    for i in 0..n {
        if !toks[i].num {
            continue;
        }
        if let Some(next) = toks.get(i + 1).filter(|t| t.seg == toks[i].seg && toks[i].f.chars().all(|c| c.is_ascii_digit())) {
            if matches!(next.f.as_str(), "bis" | "ter" | "quater") || (next.f.chars().count() == 1 && next.f.chars().all(char::is_alphabetic)) {
                nums.push((i, 2, format!("{}{}", toks[i].raw, next.raw)));
            }
        }
        nums.push((i, 1, toks[i].raw.clone()));
    }
    let in_pc = |i: usize| pcs.iter().any(|p| (p.start..p.start + p.len).contains(&i));
    nums.sort_by_key(|(i, _, _)| in_pc(*i)); // a number that cannot be a postcode is the house number first
    let seg_range = |s: usize| {
        let a = toks.iter().position(|t| t.seg == s).unwrap_or(0);
        let b = toks.iter().rposition(|t| t.seg == s).map(|b| b + 1).unwrap_or(0);
        (a, b)
    };
    let mut addresses = Vec::new();
    for (h, hl, text) in &nums {
        let (h, hl) = (*h, *hl);
        let pc_options: Vec<Option<&Postcode>> = pcs.iter().filter(|p| p.start + p.len <= h || p.start >= h + hl).map(Some).chain([None]).collect();
        for pc in pc_options {
            let mut used = vec![false; n];
            used[h..h + hl].iter_mut().for_each(|u| *u = true);
            if let Some(p) = pc {
                used[p.start..p.start + p.len].iter_mut().for_each(|u| *u = true);
            }
            let add = |s0: usize, s1: usize, addresses: &mut Vec<Reading>| {
                if (s0..s1).any(|i| used[i]) || !toks[s0..s1].iter().any(|t| t.word) {
                    return;
                }
                let mut u = used.clone();
                u[s0..s1].iter_mut().for_each(|x| *x = true);
                if let Some(town) = town_of(&toks, &u, Some(h)) {
                    let r = Reading { number: Some(text.clone()), street: join(&toks[s0..s1]), town, postcode: pc.cloned(), country };
                    if !addresses.contains(&r) {
                        addresses.push(r);
                    }
                }
            };
            // the street before the number: Via Roma 12, Calle Mayor, 5
            if h > 0 {
                let (a, _) = seg_range(toks[h - 1].seg);
                for s0 in a..h {
                    add(s0, h, &mut addresses);
                }
            }
            // the number before the street: 12 rue de la Paix, 12, rue de la Gare
            let first_in_seg = h == 0 || toks[h - 1].seg != toks[h].seg;
            if first_in_seg && h + hl < n {
                let a = h + hl;
                let (_, b) = seg_range(toks[a].seg);
                for s1 in (a + 1..=b).rev() {
                    add(a, s1, &mut addresses);
                }
            }
        }
    }
    addresses.truncate(48);
    // a street without a number, when the query names one
    let mut streets = Vec::new();
    if toks.iter().any(|t| t.word && street_word(&t.f)) {
        for pc in pcs.iter().map(Some).chain([None]) {
            let mut used = vec![false; n];
            if let Some(p) = pc {
                used[p.start..p.start + p.len].iter_mut().for_each(|u| *u = true);
            }
            for s in 0..=toks.last().map(|t| t.seg).unwrap_or(0) {
                let (a, b) = seg_range(s);
                for s0 in a..b {
                    for s1 in (s0 + 1..=b).rev() {
                        if (s0..s1).any(|i| used[i]) || !toks[s0..s1].iter().any(|t| t.word && street_word(&t.f)) {
                            continue;
                        }
                        let mut u = used.clone();
                        u[s0..s1].iter_mut().for_each(|x| *x = true);
                        if let Some(town) = town_of(&toks, &u, None) {
                            let r = Reading { number: None, street: join(&toks[s0..s1]), town, postcode: pc.cloned(), country };
                            if !streets.contains(&r) {
                                streets.push(r);
                            }
                        }
                    }
                }
            }
        }
        streets.truncate(24);
    }
    // a postcode or a town alone
    let mut areas = Vec::new();
    for pc in pcs.iter().map(Some).chain([None]) {
        let mut used = vec![false; n];
        if let Some(p) = pc {
            used[p.start..p.start + p.len].iter_mut().for_each(|u| *u = true);
        }
        if let Some(town) = town_of(&toks, &used, None) {
            if pc.is_some() || !town.is_empty() {
                areas.push(Reading { number: None, street: String::new(), town, postcode: pc.cloned(), country });
            }
        }
    }
    // a single letter with a dot or a slash opening the query is a street's abbreviation: R. Augusta, C/ Mayor, V. Roma
    let opening = toks.first().is_some_and(|t| {
        let mut c = t.f.chars();
        matches!((c.next(), c.next(), c.next()), (Some(l), Some('.' | '/'), None) if words::LANGS.iter().any(|g| g.first.iter().any(|(k, _)| k.starts_with(l))))
    });
    let named_street = opening || toks.iter().any(|t| t.word && street_word(&t.f));
    // a number that cannot be a postcode, beside a street: a house number
    let house = !addresses.is_empty() && nums.iter().any(|(i, _, _)| !in_pc(*i));
    let kind = if house || (named_street && (!addresses.is_empty() || !streets.is_empty())) {
        Kind::Address
    } else if areas.iter().any(|a| a.postcode.is_some()) {
        Kind::Postcode
    } else if !addresses.is_empty() {
        Kind::Address
    } else {
        Kind::Place
    };
    Query { kind, addresses, streets, areas, country }
}

fn join(toks: &[Tok]) -> String {
    toks.iter().map(|t| t.raw.as_str()).collect::<Vec<_>>().join(" ")
}

/// The words left over, as the town; None if something is left that cannot be part of one (a number, or a street word
/// when the street is already read). A flat or floor after the house number (2. tv, 3º izq, Top 5) is skipped, and a
/// district number after a town's name (Praha 1, Dublin 2) is not part of the town's key.
fn town_of(toks: &[Tok], used: &[bool], house: Option<usize>) -> Option<String> {
    let mut out: Vec<&str> = Vec::new();
    let mut skip_short = false;
    let mut last_word: Option<usize> = None;
    let free: Vec<usize> = (0..toks.len()).filter(|i| !used[*i]).collect();
    let mut k = 0;
    while k < free.len() {
        let i = free[k];
        let t = &toks[i];
        k += 1;
        let after_house = house.is_some_and(|h| i > h);
        if skip_short {
            skip_short = false;
            if t.num || t.f.chars().count() <= 3 {
                continue;
            }
        }
        if after_house && words::UNITS.contains(&compact(&t.f).as_str()) {
            skip_short = true;
            continue;
        }
        if ordinal(&t.raw) {
            continue;
        }
        if t.word {
            out.push(&t.raw);
            last_word = Some(i);
            continue;
        }
        if t.num {
            // a floor before its side: "2. tv"
            if after_house && free.get(k).is_some_and(|&j| words::UNITS.contains(&compact(&toks[j].f).as_str())) {
                k += 1;
                skip_short = false;
                continue;
            }
            if last_word.is_some_and(|w| w + 1 == i) && t.f.len() <= 2 && t.f.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            return None;
        }
    }
    Some(out.join(" "))
}

/// The kinds of place the address indexes find, each a code (`kind_code`) the interface words in the person's language
/// (web/locales: map.kind.<code>), beside its English `kind`: an address with its house number ("Street address"), a
/// street without one, the middle of its addresses in a town ("Street"), a postcode alone ("Postcode"), a town alone
/// ("Town").
pub const KIND_CODES: [&str; 4] = ["street_address", "street", "postcode", "town"];

/// The address files to search, with the countries their addresses are in.
struct Index {
    conn: Rc<Connection>,
    countries: Vec<String>,
    publisher: String,
}

fn meta(c: &Connection, key: &str) -> Option<String> {
    c.query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get::<_, String>(0)).ok().filter(|v| !v.is_empty())
}

fn shown_country(code: &str) -> String {
    regions::COUNTRIES.iter().find(|c| c.0 == code).map(|c| c.2.to_string()).unwrap_or_else(|| code.to_string())
}

/// The town and the postcode as the country writes them: "9490 Vaduz", "Dublin 2, D02 X285", "Rīga, LV-1050". A result
/// sends it as `locality`, with `country`, so the interface can name the country in the person's language.
fn locality(city: &str, postcode: &str, country: &str) -> String {
    let c = countries::get(country);
    let pc = countries::postcode_shown(postcode, country);
    match (city.is_empty(), pc.is_empty()) {
        (true, _) => pc,
        (false, true) => city.to_string(),
        (false, false) if c.is_some_and(|c| c.town_first) => {
            if country == "MT" {
                format!("{city} {pc}")
            } else {
                format!("{city}, {pc}")
            }
        }
        _ => format!("{pc} {city}"),
    }
}

/// A locality with its country's English name after it: the English `detail`.
fn with_country(locality: &str, country: &str) -> String {
    [locality.to_string(), shown_country(country)].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ")
}

/// "9490 Vaduz, Liechtenstein", "Dublin 2, D02 X285, Ireland", "Rīga, LV-1050, Latvia".
fn detail(city: &str, postcode: &str, country: &str) -> String {
    with_country(&locality(city, postcode, country), country)
}

/// An address as people of its country write it: "Via Roma 12", "12 rue de la Paix", "Calle Mayor, 5".
pub fn shown(num: &str, street: &str, country: &str) -> String {
    let Some(c) = countries::get(country) else { return format!("{street} {num}") };
    let number_first = match c.order {
        Order::NumberFirst => true,
        Order::StreetFirst => false,
        Order::ByLanguage => street.split_whitespace().next().is_some_and(|w| words::FRENCH_FIRST.contains(&fold(w).trim_end_matches('.'))),
    };
    if number_first {
        format!("{num} {street}")
    } else if c.comma {
        format!("{street}, {num}")
    } else {
        format!("{street} {num}")
    }
}

fn upper(key: &str) -> String {
    format!("{key}\u{10ffff}")
}

fn dist2(near: Option<(f64, f64)>, lat: f64, lon: f64) -> f64 {
    near.map(|(a, b)| (lat - a).powi(2) + ((lon - b) * a.to_radians().cos()).powi(2)).unwrap_or(0.0)
}

impl Map {
    fn address_indexes(&self) -> Vec<Index> {
        self.address_files()
            .iter()
            .filter_map(|p| {
                let conn = self.db_at(p)?;
                let stem = p.file_stem()?.to_str()?.to_string();
                let countries = match meta(&conn, "countries") {
                    Some(list) => list.split(',').map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect(),
                    None if regions::is_code(&stem) => vec![regions::country(&stem).to_string()],
                    None => countries::COUNTRIES.iter().map(|c| c.code.to_string()).collect(),
                };
                let publisher = meta(&conn, "publisher").unwrap_or_else(|| "Overture Maps".into());
                Some(Index { conn, countries, publisher })
            })
            .collect()
    }

    /// Street addresses matching a query, exact house numbers only, nearest to `near` first among equals.
    pub fn find_addresses(&self, q: &str, limit: usize, near: Option<(f64, f64)>) -> Vec<Value> {
        self.addresses(&parse(q), limit, near)
    }

    /// The countries of an index a reading may be in.
    fn countries_for<'a>(ix: &'a Index, r: &Reading) -> Vec<&'a str> {
        ix.countries
            .iter()
            .map(String::as_str)
            .filter(|c| r.country.is_none_or(|k| k == *c) && r.postcode.as_ref().is_none_or(|p| p.countries.contains(c)))
            .collect()
    }

    pub(crate) fn addresses(&self, q: &Query, limit: usize, near: Option<(f64, f64)>) -> Vec<Value> {
        let indexes = self.address_indexes();
        if indexes.is_empty() {
            return Vec::new();
        }
        for r in &q.addresses {
            let Some(num) = &r.number else { continue };
            for form in number_forms(num) {
                type Found = (String, String, String, String, f64, f64, String, String, String); // num, street, city, postcode, lat, lon, norm, country, source
                let mut rows: Vec<Found> = Vec::new();
                for ix in &indexes {
                    for c in Self::countries_for(ix, r) {
                        let key = street_key(&r.street, c);
                        if key.is_empty() {
                            continue;
                        }
                        let mut sql = String::from("SELECT num, street, city, postcode, lat, lon, norm, country FROM addr WHERE numkey = ? AND norm >= ? AND norm < ? AND country = ?");
                        let mut params: Vec<Sql> = vec![Sql::Text(form.clone()), Sql::Text(key.clone()), Sql::Text(upper(&key)), Sql::Text(c.to_string())];
                        filters(&mut sql, &mut params, r, c);
                        sql.push_str(" ORDER BY length(norm), norm LIMIT ?");
                        params.push(Sql::Integer(limit as i64 * 3));
                        let Ok(mut st) = ix.conn.prepare_cached(&sql) else { continue };
                        let found = st.query_map(rusqlite::params_from_iter(params.iter()), |row| {
                            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?))
                        });
                        if let Ok(found) = found {
                            rows.extend(found.flatten().map(|(a, b, c, d, e, f, g, h)| (a, b, c, d, e, f, g, h, ix.publisher.clone())));
                        }
                    }
                }
                if rows.is_empty() {
                    continue;
                }
                rows.sort_by(|a, b| {
                    (a.6.chars().count(), dist2(near, a.4, a.5), &a.7, &a.2, &a.1)
                        .partial_cmp(&(b.6.chars().count(), dist2(near, b.4, b.5), &b.7, &b.2, &b.1))
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                let mut seen = HashSet::new();
                let mut out = Vec::new();
                for (num, street, city, postcode, lat, lon, norm, country, source) in rows {
                    if !seen.insert((num.clone(), norm, lat.to_bits(), lon.to_bits())) {
                        continue;
                    }
                    out.push(json!({"name": shown(&num, &street, &country), "kind": "Street address", "kind_code": "street_address", "detail": detail(&city, &postcode, &country),
                                    "locality": locality(&city, &postcode, &country), "country": country, "lat": lat, "lon": lon, "source": source, "onMap": true}));
                    if out.len() == limit {
                        break;
                    }
                }
                return out;
            }
        }
        Vec::new()
    }

    /// Streets matching a query that names one without a house number: the middle of each street in each town.
    pub(crate) fn streets(&self, q: &Query, limit: usize, near: Option<(f64, f64)>) -> Vec<Value> {
        let indexes = self.address_indexes();
        for r in &q.streets {
            type Street = (String, String, String, f64, f64, i64, String, String); // street, city, country, lat, lon, addresses, norm, source
            let mut rows: Vec<Street> = Vec::new();
            for ix in &indexes {
                for c in Self::countries_for(ix, r) {
                    let key = street_key(&r.street, c);
                    if key.is_empty() {
                        continue;
                    }
                    let mut sql = String::from("SELECT street, city, country, AVG(lat), AVG(lon), COUNT(*), norm FROM addr WHERE norm >= ? AND norm < ? AND country = ?");
                    let mut params: Vec<Sql> = vec![Sql::Text(key.clone()), Sql::Text(upper(&key)), Sql::Text(c.to_string())];
                    filters(&mut sql, &mut params, r, c);
                    sql.push_str(" GROUP BY norm, city, country ORDER BY length(norm), COUNT(*) DESC LIMIT ?");
                    params.push(Sql::Integer(limit as i64 * 3));
                    let Ok(mut st) = ix.conn.prepare_cached(&sql) else { continue };
                    let found = st.query_map(rusqlite::params_from_iter(params.iter()), |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)));
                    if let Ok(found) = found {
                        rows.extend(found.flatten().map(|(a, b, c, d, e, f, g)| (a, b, c, d, e, f, g, ix.publisher.clone())));
                    }
                }
            }
            if rows.is_empty() {
                continue;
            }
            rows.sort_by(|a, b| (a.6.chars().count(), dist2(near, a.3, a.4)).partial_cmp(&(b.6.chars().count(), dist2(near, b.3, b.4))).unwrap_or(std::cmp::Ordering::Equal));
            let mut seen = HashSet::new();
            return rows
                .into_iter()
                .filter(|r| seen.insert((r.6.clone(), r.1.clone(), r.2.clone())))
                .take(limit)
                .map(|(street, city, country, lat, lon, _, _, source)| {
                    json!({"name": street, "kind": "Street", "kind_code": "street", "detail": detail(&city, "", &country), "locality": locality(&city, "", &country),
                           "country": country, "lat": lat, "lon": lon, "source": source, "onMap": true})
                })
                .collect();
        }
        Vec::new()
    }

    /// A postcode or a town searched alone, from the indexes' `area` tables: the middle of its addresses.
    pub(crate) fn areas(&self, q: &Query, limit: usize, near: Option<(f64, f64)>) -> Vec<Value> {
        let indexes = self.address_indexes();
        let mut out: Vec<(f64, Value)> = Vec::new();
        for r in &q.areas {
            if let Some(pc) = &r.postcode {
                for ix in &indexes {
                    let (mut sql, mut params) = if pc.partial {
                        ("SELECT country, postcode, city, lat, lon, n FROM area WHERE zip >= ? AND zip < ?".to_string(), vec![Sql::Text(pc.key.clone()), Sql::Text(upper(&pc.key))])
                    } else {
                        ("SELECT country, postcode, city, lat, lon, n FROM area WHERE zip = ?".to_string(), vec![Sql::Text(pc.key.clone())])
                    };
                    if !r.town.is_empty() {
                        sql.push_str(" AND (' ' || citykey) LIKE ?");
                        params.push(Sql::Text(format!("% {}%", name_key(&r.town))));
                    }
                    sql.push_str(" ORDER BY n DESC LIMIT 60");
                    let Ok(mut st) = ix.conn.prepare_cached(&sql) else { continue };
                    let Ok(found) = st.query_map(rusqlite::params_from_iter(params.iter()), |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, f64>(3)?, row.get::<_, f64>(4)?, row.get::<_, i64>(5)?))
                    }) else { continue };
                    for (country, postcode, city, lat, lon, n) in found.flatten() {
                        if !pc.countries.contains(&country.as_str()) || r.country.is_some_and(|k| k != country) {
                            continue;
                        }
                        let rank = dist2(near, lat, lon) - n as f64 * 1e-9;
                        out.push((rank, json!({"name": countries::postcode_shown(&postcode, &country), "kind": "Postcode", "kind_code": "postcode", "detail": detail(&city, "", &country),
                                                "locality": locality(&city, "", &country), "country": country, "lat": lat, "lon": lon, "source": ix.publisher, "onMap": true})));
                    }
                }
                if !out.is_empty() {
                    break;
                }
            } else if !r.town.is_empty() {
                let key = name_key(&r.town);
                // the rows of one town, by postcode: those near each other are one place
                type Town = (String, String, f64, f64, i64, Vec<String>, String); // country, city, lat, lon, addresses, postcodes, source
                let mut towns: Vec<Town> = Vec::new();
                for ix in &indexes {
                    // any word of the town's name may start it: "Valletta" is "Il-Belt Valletta"
                    let Ok(mut st) = ix.conn.prepare_cached("SELECT country, city, postcode, lat, lon, n FROM area WHERE (' ' || citykey) LIKE ? ORDER BY n DESC LIMIT 400") else { continue };
                    let Ok(found) = st.query_map([format!("% {key}%")], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, f64>(3)?, row.get::<_, f64>(4)?, row.get::<_, i64>(5)?))
                    }) else { continue };
                    for (country, city, postcode, lat, lon, n) in found.flatten() {
                        if r.country.is_some_and(|k| k != country) {
                            continue;
                        }
                        let near_one = towns.iter_mut().find(|t| t.0 == country && name_key(&t.1) == name_key(&city) && super::haversine(t.2, t.3, lat, lon).0 < 15.0);
                        match near_one {
                            Some(t) => {
                                let w = t.4 as f64 / (t.4 + n) as f64;
                                t.2 = t.2 * w + lat * (1.0 - w);
                                t.3 = t.3 * w + lon * (1.0 - w);
                                t.4 += n;
                                if !postcode.is_empty() && !t.5.contains(&postcode) {
                                    t.5.push(postcode);
                                }
                            }
                            None => towns.push((country, city, lat, lon, n, if postcode.is_empty() { vec![] } else { vec![postcode] }, ix.publisher.clone())),
                        }
                    }
                }
                for (country, city, lat, lon, n, pcs, source) in towns {
                    let mut shown_pcs: Vec<String> = pcs.iter().take(3).map(|p| countries::postcode_shown(p, &country)).collect();
                    if pcs.len() > 3 {
                        shown_pcs.push("…".into());
                    }
                    let local = shown_pcs.join(" ");
                    out.push((dist2(near, lat, lon) - n as f64 * 1e-9, json!({"name": city, "kind": "Town", "kind_code": "town", "detail": with_country(&local, &country),
                                                                              "locality": local, "country": country, "lat": lat, "lon": lon, "source": source, "onMap": true})));
                }
                if !out.is_empty() {
                    break;
                }
            }
        }
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        out.into_iter().take(limit).map(|(_, v)| v).collect()
    }
}

/// The postcode and town a reading names, as conditions on `addr`.
fn filters(sql: &mut String, params: &mut Vec<Sql>, r: &Reading, _country: &str) {
    if let Some(pc) = &r.postcode {
        if pc.partial {
            sql.push_str(" AND zip >= ? AND zip < ?");
            params.push(Sql::Text(pc.key.clone()));
            params.push(Sql::Text(upper(&pc.key)));
        } else {
            sql.push_str(" AND zip = ?");
            params.push(Sql::Text(pc.key.clone()));
        }
    }
    let town = name_key(&r.town);
    if !town.is_empty() {
        sql.push_str(" AND (' ' || citykey) LIKE ?");
        params.push(Sql::Text(format!("% {town}%")));
    }
}

#[cfg(test)]
mod tests;
