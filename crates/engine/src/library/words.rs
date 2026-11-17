//! Words for search. Folding, splitting and matching as web/words.js does them, so the engine and the interface agree on
//! what a word is; and each language's stop words, synonyms and phrases from `content/search/<lang>.json`, read once at
//! start. Ported from the original Lastbook's server.py (STOP, SYN, PHRASES, query_groups, fts_expr) and web/words.js.

use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::LazyLock;
use unicode_normalization::UnicodeNormalization;

/// English as the original Lastbook had it, for when the program's own `content/search/en.json` cannot be read.
const BUILTIN_EN: &str = include_str!("../../../../content/search/en.json");

static LATIN_MARKS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\p{Script=Latin})\p{M}+").unwrap());
static NOT_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^\p{L}\p{M}\p{N}]+").unwrap());
static NO_SPACES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}\p{Script=Thai}\p{Script=Lao}\p{Script=Khmer}\p{Script=Myanmar}]").unwrap()
});
static QUERY_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\w'-]+").unwrap());

/// The key two texts are compared by, as web/words.js fold(): lower case, decomposed (NFKD), accent marks on Latin
/// letters taken off, composed again, and final ς as σ. ß, ł and œ stay, as in the library's FTS5 index.
pub fn fold(s: &str) -> String {
    let decomposed: String = s.to_lowercase().nfkd().collect();
    let bare = LATIN_MARKS.replace_all(&decomposed, "$1");
    bare.nfc().collect::<String>().replace('ς', "σ")
}

/// Whether a word is in a script written without spaces (Chinese, Japanese, Korean, Thai...).
pub fn no_spaces(w: &str) -> bool {
    NO_SPACES.is_match(w)
}

/// A text's words, folded, as words.js splitWords does without Intl.Segmenter: split wherever there is no letter, mark
/// or digit. A run of Chinese or Thai stays one word.
pub fn split_words(text: &str) -> Vec<String> {
    NOT_WORD.split(text).map(fold).filter(|w| !w.is_empty()).collect()
}

/// Words worth searching for (words.js keep): in a script without spaces at any length, two letters in a script without
/// case, otherwise three characters or more.
fn keep(w: &str) -> bool {
    let n = w.chars().count();
    no_spaces(w) || n > 2 || (n == 2 && w.chars().any(char::is_alphabetic) && !w.chars().any(char::is_lowercase))
}

/// The words of a query, without stop words (words.js queryWords).
pub fn query_words(q: &str, stop: &HashSet<String>) -> Vec<String> {
    split_words(q).into_iter().filter(|w| keep(w) && !stop.contains(w)).collect()
}

/// A test for one piece of text: does a query word start one of its words, or, in a script without spaces, appear
/// anywhere in it (words.js matcher)?
pub struct Matcher {
    words: Vec<String>,
    whole: String,
}

impl Matcher {
    pub fn new(text: &str) -> Matcher {
        Matcher { words: split_words(text), whole: fold(text) }
    }

    pub fn has(&self, w: &str) -> bool {
        if no_spaces(w) {
            self.whole.contains(w)
        } else {
            self.words.iter().any(|h| h.starts_with(w))
        }
    }
}

/// Guides for a query, scored as web/words.js rankTopics: a word in a guide's title, short line or keywords counts 1, a
/// word in one of its choices 1.5; guides within 60% of the best, top 3. Gives each guide's id and its best choice's id.
pub fn rank_topics(q: &str, topics: &[Value], stop: &HashSet<String>) -> Vec<(String, Option<String>)> {
    let words = query_words(q, stop);
    if words.is_empty() {
        return Vec::new();
    }
    let s = |v: &Value| v.as_str().unwrap_or("").to_string();
    let mut out: Vec<(f64, String, Option<String>)> = Vec::new();
    for t in topics {
        let mut hay = vec![s(&t["title"]), s(&t["short"])];
        hay.extend(t["keywords"].as_array().into_iter().flatten().map(s));
        let hay = Matcher::new(&hay.join(" "));
        let score = words.iter().filter(|w| hay.has(w)).count() as f64;
        let (mut best, mut bs) = (None, 0);
        for c in t["choices"].as_array().into_iter().flatten() {
            if truthy(&c["goto"]) {
                continue;
            }
            let has = Matcher::new(&format!("{} {}", s(&c["label"]), s(&c["hint"])));
            let n = words.iter().filter(|w| has.has(w)).count();
            if n > bs {
                bs = n;
                best = Some(s(&c["id"]));
            }
        }
        if score > 0.0 || bs > 0 {
            out.push((score + bs as f64 * 1.5, s(&t["id"]), best));
        }
    }
    out.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let top = out.first().map(|o| o.0).unwrap_or(0.0);
    out.into_iter().filter(|o| o.0 >= top * 0.6).take(3).map(|(_, t, c)| (t, c)).collect()
}

/// Truth as the original Lastbook judged it: null, false, 0, "" and empty lists and objects are false.
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// One language's search words, from `content/search/<lang>.json`.
pub struct Lang {
    pub stop: HashSet<String>,
    elisions: Vec<String>,
    synonyms: HashMap<String, Vec<String>>,
    phrases: Vec<(Regex, Vec<Vec<String>>)>,
    plural: Option<(Vec<String>, usize)>,
}

impl Lang {
    /// From the file's JSON. Words are folded as they are read, so a list may be written with its accents. A phrase that
    /// does not compile is left out.
    pub fn from_json(v: &Value) -> Lang {
        let strings = |v: &Value| v.as_array().into_iter().flatten().filter_map(|x| x.as_str()).map(str::to_string).collect::<Vec<_>>();
        let synonyms = v["synonyms"].as_object().into_iter().flatten().map(|(k, vs)| (fold(k), strings(vs))).collect();
        let phrases = v["phrases"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| {
                let rx = Regex::new(p["pattern"].as_str()?).ok()?;
                let groups = p["groups"].as_array()?.iter().map(strings).filter(|g| !g.is_empty()).collect();
                Some((rx, groups))
            })
            .collect();
        let plural = v["plural"].as_object().map(|p| (strings(&p["suffixes"]), p["longer_than"].as_u64().unwrap_or(4) as usize));
        Lang {
            stop: strings(&v["stop"]).iter().map(|w| fold(w)).collect(),
            elisions: strings(&v["elisions"]).iter().map(|w| fold(w)).collect(),
            synonyms,
            phrases,
            plural,
        }
    }
}

/// Every language's search words. English is always there: the others are searched with their own lists first, then
/// English's, because most of the library is in English.
pub struct Words {
    langs: HashMap<String, Lang>,
}

impl Words {
    /// Reads every `<lang>.json` in the folder (the program's `content/search`).
    pub fn load(dir: &Path) -> Words {
        let mut langs = HashMap::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let p = e.path();
                let Some(code) = p.file_stem().and_then(|s| s.to_str()).filter(|_| p.extension().is_some_and(|x| x == "json")) else { continue };
                if let Some(v) = std::fs::read(&p).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
                    langs.insert(code.to_ascii_lowercase(), Lang::from_json(&v));
                }
            }
        }
        if !langs.contains_key("en") {
            langs.insert("en".into(), Lang::from_json(&serde_json::from_str(BUILTIN_EN).unwrap_or(Value::Null)));
        }
        Words { langs }
    }

    /// The languages with lists of their own.
    pub fn languages(&self) -> Vec<String> {
        let mut v: Vec<String> = self.langs.keys().cloned().collect();
        v.sort();
        v
    }

    /// A language code as a request may give it (`it`, `it-IT`, `IT`), as one this has lists for, else `en`.
    pub fn code(&self, lang: &str) -> String {
        let primary = lang.split(['-', '_']).next().unwrap_or("").to_ascii_lowercase();
        if self.langs.contains_key(&primary) {
            primary
        } else {
            "en".into()
        }
    }

    /// The lists a search in this language uses, in order: its own, then English's.
    fn chain(&self, lang: &str) -> Vec<&Lang> {
        let code = self.code(lang);
        let en = &self.langs["en"];
        if code == "en" {
            vec![en]
        } else {
            vec![&self.langs[&code], en]
        }
    }

    /// Whether a folded word is left out of a search in this language.
    pub fn is_stop(&self, w: &str, lang: &str) -> bool {
        self.chain(lang).iter().any(|l| l.stop.contains(w))
    }

    /// The stop words of a search in this language.
    pub fn stop_words(&self, lang: &str) -> HashSet<String> {
        self.chain(lang).iter().flat_map(|l| l.stop.iter().cloned()).collect()
    }

    /// A query as ideas to look for, as server.py query_groups: the content words, and for each idea the words that
    /// stand for it (OR-ed within a group, groups AND-ed). Everyday phrases first, then single words with their
    /// synonyms and, where the language has the rule, the word without its plural ending.
    pub fn query_groups(&self, q: &str, lang: &str) -> (Vec<String>, Vec<Vec<String>>) {
        let chain = self.chain(lang);
        let mut q = fold(q).replace('\u{2019}', "'");
        let mut phrase_groups: Vec<Vec<String>> = Vec::new();
        for l in &chain {
            for (rx, groups) in &l.phrases {
                if rx.is_match(&q) {
                    phrase_groups.extend(groups.iter().cloned());
                    q = rx.replace_all(&q, " ").into_owned();
                }
            }
        }
        let words: Vec<String> = QUERY_WORD
            .find_iter(&q)
            .map(|m| m.as_str().trim_matches('\'').to_string())
            .filter(|w| !w.is_empty())
            .map(|w| match w.split_once('\'') {
                // l'acqua, d'eau: the word after an elided article
                Some((head, rest)) if !rest.is_empty() && chain.iter().any(|l| l.elisions.iter().any(|e| e == head)) => rest.trim_matches('\'').to_string(),
                _ => w,
            })
            .filter(|w| !w.is_empty())
            .collect();
        let mut content: Vec<String> = words.iter().filter(|w| !chain.iter().any(|l| l.stop.contains(*w))).cloned().collect();
        if content.is_empty() && phrase_groups.is_empty() {
            content = words;
        }
        let plural = chain.iter().find_map(|l| l.plural.as_ref());
        let mut groups = phrase_groups.clone();
        for w in &content {
            let mut g = vec![w.clone()];
            if let Some(syn) = chain.iter().find_map(|l| l.synonyms.get(w)) {
                g.extend(syn.iter().cloned());
            }
            if let Some((suffixes, longer_than)) = plural {
                for s in suffixes {
                    if w.ends_with(s.as_str()) && w.chars().count() > *longer_than {
                        let stem = w[..w.len() - s.len()].to_string();
                        if !stem.is_empty() && !g.contains(&stem) {
                            g.push(stem);
                        }
                    }
                }
            }
            groups.push(g);
        }
        let mut all_content: Vec<String> = phrase_groups.iter().map(|g| g[0].clone()).collect();
        all_content.extend(content);
        (all_content, groups)
    }
}

/// The FTS5 expression for query groups, as server.py fts_expr: each word a quoted phrase, OR within a group, AND
/// between groups.
pub fn fts_expr(groups: &[Vec<String>]) -> String {
    let q = |t: &String| format!("\"{}\"", t.replace('"', "\"\""));
    groups.iter().map(|g| format!("({})", g.iter().map(q).collect::<Vec<_>>().join(" OR "))).collect::<Vec<_>>().join(" AND ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn repo() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn words() -> Words {
        Words::load(&repo().join("content/search"))
    }

    fn groups(q: &str, lang: &str) -> Vec<Vec<String>> {
        words().query_groups(q, lang).1
    }

    #[test]
    fn folds_as_words_js() {
        // app/tests/test_topic_search.py FOLD
        for (s, want) in [
            ("İstanbul", "istanbul"), ("ISTANBUL", "istanbul"), ("ılık", "ılık"), ("Straße", "straße"), ("ΣΟΦΟΣ", "σοφοσ"),
            ("ｗａｔｅｒ", "water"), ("niño", "nino"), ("nin\u{303}o", "nino"), ("Jalapeño", "jalapeno"), ("καφές", "καφέσ"),
            ("crème brûlée", "creme brulee"),
        ] {
            assert_eq!(fold(s), want, "{s}");
        }
    }

    #[test]
    fn query_words_as_words_js_without_a_segmenter() {
        let app = std::fs::read_to_string(repo().join("web/app.js")).unwrap();
        let qstop = Regex::new(r"const QSTOP = new Set\('([^']*)'\.split").unwrap().captures(&app).unwrap()[1].to_string();
        let stop: HashSet<String> = qstop.split(' ').map(str::to_string).collect();
        // app/tests/test_topic_search.py WORDS_BOTH, and the runs without spaces as they come out with no segmenter
        let cases: Vec<(&str, Vec<&str>)> = vec![
            ("¿Cómo purifico el agua para el niño?", vec!["como", "purifico", "agua", "para", "nino"]),
            ("niño", vec!["nino"]),
            ("nin\u{303}o", vec!["nino"]),
            ("crème brûlée", vec!["creme", "brulee"]),
            ("How do I purify water's taste?", vec!["purify", "water", "taste"]),
            ("don\u{2019}t have water", vec!["water"]),
            ("ｗａｔｅｒ", vec!["water"]),
            ("water.boil first_aid e.coli boil:water", vec!["water", "boil", "first", "aid", "coli", "boil", "water"]),
            ("water\u{ad}proof", vec!["water", "proof"]),
            ("می\u{200c}خواهم", vec!["می", "خواهم"]),
            ("आग אש دم", vec!["आग", "אש", "دم"]),
            ("पानी उबालें", vec!["पानी", "उबालें"]),
            ("ab 42 112 go", vec!["112"]),
            ("\u{10428}\u{1042f}", vec![]),
            ("c++ (water", vec!["water"]),
            ("[a", vec![]), ("(", vec![]), ("\\", vec![]), ("*?+{", vec![]), ("", vec![]),
            ("İstanbul", vec!["istanbul"]),
            ("éclair", vec!["eclair"]),
            ("怎么净化水", vec!["怎么净化水"]),
        ];
        for (q, want) in cases {
            assert_eq!(query_words(q, &stop), want, "{q:?}");
        }
    }

    #[test]
    fn matches_as_words_js() {
        for (text, w, want) in [
            ("chocolate éclair", "eclair", true), ("Chocolate Éclair", "eclair", true), ("煮沸 水", "水", true), ("怎么净化水", "净化", true),
            ("drink water", "rink", false), ("drink water", "drin", true), ("Rainwater", "water", false), ("Straße", "strasse", false),
            ("ΣΟΦΟΣ", "σοφοσ", true), ("İstanbul", "istanbul", true), ("72-hour kit", "hour", true), ("Water: purify, boil", "purif", true),
        ] {
            assert_eq!(Matcher::new(text).has(w), want, "{text} {w}");
        }
    }

    #[test]
    fn english_groups_as_the_python_engine() {
        let w = words();
        let (content, g) = w.query_groups("How do I stop bleeding from a deep cut?", "en");
        assert_eq!(content, vec!["stop", "bleeding", "deep", "cut"]);
        assert_eq!(g[1], vec!["bleeding", "haemorrhage", "hemorrhage", "blood loss"]);
        assert_eq!(g[3], vec!["cut", "wound", "wounds", "laceration", "bleeding"]);
        // a phrase becomes one idea and leaves the rest; a plural also looks for the singular
        let (content, g) = w.query_groups("power cut, my phone is dead and no batteries", "en");
        assert_eq!(g[0], vec!["outage", "blackout", "power outage"]);
        assert_eq!(g[1], vec!["phone", "phones", "cell phone"]);
        assert_eq!(g[2], vec!["charge", "charging", "charger", "power bank"]);
        assert_eq!(content[..3], ["outage", "phone", "charge"]);
        assert_eq!(*g.last().unwrap(), vec!["batteries", "batterie"]);
        // only stop words: they are searched for after all
        assert_eq!(w.query_groups("how do I", "en").1, vec![vec!["how"], vec!["do"], vec!["i"]]);
        assert_eq!(w.query_groups("", "en").1, Vec::<Vec<String>>::new());
        assert_eq!(fts_expr(&[vec!["a\"b".into(), "c d".into()], vec!["e".into()]]), r#"("a""b" OR "c d") AND ("e")"#);
    }

    #[test]
    fn other_languages_use_their_own_words_then_english() {
        let g = groups("Come purificare l'acqua?", "it");
        assert_eq!(g[0][0], "purificare");
        assert!(g[0].contains(&"purify".to_string()));
        assert_eq!(g[1], vec!["acqua", "water"]);
        let g = groups("manca la corrente", "it");
        assert!(g[0].contains(&"power outage".to_string()));
        assert_eq!(groups("Kein Strom, was tun?", "de")[0][0], "stromausfall");
        assert_eq!(groups("Wie koche ich Wasser ab?", "de-AT").iter().find(|g| g[0] == "wasser").unwrap()[2], "water");
        assert_eq!(groups("Plus d’électricité", "fr")[0][0], "panne de courant");
        assert_eq!(groups("purifier l'eau", "fr")[1], vec!["eau", "eau potable", "water"]);
        assert_eq!(groups("se fue la luz", "es")[0][0], "apagón");
        assert_eq!(groups("brak prądu", "pl")[0][0], "awaria prądu");
        assert!(groups("złamanie nogi", "pl")[0].contains(&"fracture".to_string()));
        // English words still work in another language, and an unknown language is English
        assert_eq!(groups("bleeding", "it")[0], groups("bleeding", "en")[0]);
        assert_eq!(groups("bleeding", "xx"), groups("bleeding", "en"));
        // accents fold: niño and nino are the same word
        assert_eq!(groups("niño", "es"), groups("nino", "es"));
    }

    #[test]
    fn the_shipped_lists_are_well_formed() {
        let dir = repo().join("content/search");
        let w = words();
        assert_eq!(w.languages(), vec!["de", "en", "es", "fr", "it", "pl"]);
        for lang in w.languages() {
            let v: Value = serde_json::from_slice(&std::fs::read(dir.join(format!("{lang}.json"))).unwrap()).unwrap();
            assert!(v["stop"].as_array().unwrap().len() > 30, "{lang}");
            assert!(v["synonyms"].as_object().unwrap().len() > 30, "{lang}");
            for p in v["phrases"].as_array().unwrap() {
                let pattern = p["pattern"].as_str().unwrap();
                assert!(Regex::new(pattern).is_ok(), "{lang}: {pattern}");
                assert_eq!(fold(pattern), pattern, "{lang}: a phrase is matched on the folded query, so it must be written folded");
                assert!(!p["groups"].as_array().unwrap().is_empty());
            }
            // no two synonym keys fold to the same word
            let keys: Vec<String> = v["synonyms"].as_object().unwrap().keys().map(|k| fold(k)).collect();
            let unique: HashSet<&String> = keys.iter().collect();
            assert_eq!(unique.len(), keys.len(), "{lang}");
        }
        // the built-in copy is the shipped English
        let built = Lang::from_json(&serde_json::from_str(BUILTIN_EN).unwrap());
        assert_eq!(built.stop, w.langs["en"].stop);
        let none = Words::load(Path::new("/nonexistent"));
        assert_eq!(none.languages(), vec!["en"]);
        assert_eq!(none.query_groups("cut", "it"), w.query_groups("cut", "en"));
    }

    fn qstop() -> HashSet<String> {
        let app = std::fs::read_to_string(repo().join("web/app.js")).unwrap();
        Regex::new(r"const QSTOP = new Set\('([^']*)'\.split").unwrap().captures(&app).unwrap()[1].split(' ').map(str::to_string).collect()
    }

    fn guides() -> Vec<Value> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(repo().join("content/topics")).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
        files.sort();
        files.iter().map(|p| serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()).collect()
    }

    /// Guide matching as it was before words.js (app/tests/topic_search_test.mjs, before): a-z words, each found at a
    /// word start.
    fn rank_before(q: &str, topics: &[Value], stop: &HashSet<String>) -> Vec<(String, Option<String>)> {
        let words: Vec<String> = Regex::new("[^a-z0-9]+").unwrap().split(&q.to_lowercase()).filter(|w| w.len() > 2 && !stop.contains(*w)).map(str::to_string).collect();
        if words.is_empty() {
            return Vec::new();
        }
        let has = |hay: &str, w: &str| hay.match_indices(w).any(|(i, _)| i == 0 || !hay[..i].chars().last().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_'));
        let s = |v: &Value| v.as_str().unwrap_or("").to_string();
        let mut out: Vec<(f64, String, Option<String>)> = Vec::new();
        for t in topics {
            let mut hay = vec![s(&t["title"]), s(&t["short"])];
            hay.extend(t["keywords"].as_array().into_iter().flatten().map(s));
            let hay = hay.join(" ").to_lowercase();
            let score = words.iter().filter(|w| has(&hay, w)).count() as f64;
            let (mut best, mut bs) = (None, 0);
            for c in t["choices"].as_array().into_iter().flatten().filter(|c| !truthy(&c["goto"])) {
                let ch = format!("{} {}", s(&c["label"]), s(&c["hint"])).to_lowercase();
                let n = words.iter().filter(|w| has(&ch, w)).count();
                if n > bs {
                    bs = n;
                    best = Some(s(&c["id"]));
                }
            }
            if score > 0.0 || bs > 0 {
                out.push((score + bs as f64 * 1.5, s(&t["id"]), best));
            }
        }
        out.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        let top = out.first().map(|o| o.0).unwrap_or(0.0);
        out.into_iter().filter(|o| o.0 >= top * 0.6).take(3).map(|(_, t, c)| (t, c)).collect()
    }

    #[test]
    fn english_guide_search_ranks_as_before() {
        // app/tests/test_topic_search.py ENGLISH
        let (stop, topics) = (qstop(), guides());
        let english = [
            "how do I purify water", "boil water", "is the tap water safe", "bleach for drinking water", "find water", "deep cut bleeding", "broken leg",
            "burn on my hand", "cpr", "choking", "fever", "dehydration", "snake bite", "hypothermia", "the power is out", "generator", "solar panel",
            "car battery", "earthquake", "flood", "wildfire smoke", "store food", "cook without power", "stay warm", "shelter", "toilet", "radio",
            "grow vegetables", "save seeds", "first aid kit", "water's gone brown", "don't have power", "can't breathe", "how to start a fire", "baby",
            "medicine", "keep food cold", "clean water storage container", "water.boil", "first_aid kit", "e.coli", "boil:water", "https://example.org/water",
        ];
        let mut reached = 0;
        for q in english {
            let before = rank_before(q, &topics, &stop);
            assert_eq!(rank_topics(q, &topics, &stop), before, "{q}");
            reached += !before.is_empty() as usize;
        }
        assert!(reached > 30, "the list does reach the guides");
        // a made-up guide in Spanish and Chinese: found by Spanish words and by one Han character
        let own = vec![serde_json::json!({"id": "own", "title": "Agua potable", "short": "Hervir el agua", "keywords": ["水"], "choices": []})];
        for q in ["agua", "水", "hervir"] {
            assert_eq!(rank_topics(q, &own, &stop), vec![("own".to_string(), None)], "{q}");
        }
    }

    /// The English guides are read across Europe and beyond: the words of British and American English, and the
    /// emergency numbers people bring from home, find the same guides; everyday words find the right situation.
    #[test]
    fn words_from_different_countries_find_the_same_guides() {
        let (stop, topics) = (qstop(), guides());
        let top = |q: &str| rank_topics(q, &topics, &stop);
        let pair = |t: &str, c: &str| (t.to_string(), Some(c.to_string()));
        for (q, us) in [("torch", "flashlight"), ("tinned food", "canned food"), ("999", "911"), ("adrenaline", "epinephrine"), ("sanitiser", "sanitizer"),
                        ("bushfire", "wildfire")] {
            assert!(!top(us).is_empty(), "{us}");
            assert_eq!(top(q), top(us), "{q} / {us}");
        }
        for (q, us) in [("rubbish", "garbage"), ("lift", "elevator"), ("power cut", "power outage"), ("diarrhoea", "diarrhea"), ("112", "911"),
                        ("heatstroke", "heat stroke"), ("forest fire", "wildfire")] {
            assert_eq!(top(q).first().map(|t| t.0.clone()), top(us).first().map(|t| t.0.clone()), "{q} / {us}");
        }
        for (q, t, c) in [("power cut", "power", "first-hours"), ("blackout", "power", "first-hours"), ("stuck in a lift", "power", "lift"),
                          ("generator", "power", "generator"), ("cut", "hurt", "bleeding"), ("tick", "hurt", "bites"), ("flood", "disaster", "flood-warning"),
                          ("earthquake", "disaster", "quake-now"), ("tsunami", "disaster", "tsunami"), ("forest fire", "disaster", "forest-fire"),
                          ("boil water", "water", "boil"), ("toilet", "sanitation", "no-flush"), ("heating", "shelter", "cold"), ("heat wave", "shelter", "heat"),
                          ("frozen pipes", "shelter", "home"), ("112", "comms", "call")] {
            assert_eq!(top(q).first(), Some(&pair(t, c)), "{q}");
        }
    }
}
