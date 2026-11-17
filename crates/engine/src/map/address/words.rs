//! Street words of the languages of the 31 countries, for street keys. Each entry is written as people write it; the
//! table is folded and loosened ([`crate::map::text::loose`]) when it is first used, so "Straße", "strasse" and "STRASSE"
//! are one entry.
//!
//! What a word stands for in a key:
//! - `""`: the street word every street of the language has (Straße, rue, via, calle, ulica, utca, gatan...). It is
//!   dropped, so "Hauptstraße", "Hauptstr." and "Haupt" find the same street, and "ul. Marszałkowska" finds
//!   "Marszałkowska".
//! - `"plaza"` for a square, `"avenue"` for an avenue, `"boulevard"`: shared by the languages of one country, so an
//!   abbreviation they share (pl., av., bd) means one thing in Belgium, Switzerland or Spain.
//! - another word: the full word an abbreviation stands for (c.so corso, ctra carretera, mnt maantee).

pub struct Lang {
    pub code: &'static str,
    /// Words and abbreviations (dots taken out: "v.le" is "vle"), anywhere in a street name.
    pub words: &'static [(&'static str, &'static str)],
    /// Only as the first word: single letters such as c (calle), v (via), r (rue).
    pub first: &'static [(&'static str, &'static str)],
    /// Only as the last word: g (gatvė, gatan), k (katu), St (Street), Dr (Drive).
    pub last: &'static [(&'static str, &'static str)],
    /// The ends of compound street names, split off and read as their word: Bahnhofstraße, Drottninggatan, Laugavegur.
    pub suffixes: &'static [(&'static str, &'static str)],
    /// Small words dropped from keys, so "Rue de la Paix" and "rue Paix", "Via della Spiga" and "Via Spiga" meet.
    pub particles: &'static [&'static str],
}

/// Titles in street names, written out or short, in every language: one key each.
pub const TITLES: &[(&str, &str)] = &[
    ("saint", "st"), ("sankt", "st"), ("sint", "st"), ("st", "st"), ("sainte", "ste"), ("ste", "ste"),
    ("docteur", "dr"), ("doktor", "dr"), ("doctor", "dr"), ("dottore", "dr"), ("dr", "dr"),
    ("général", "gen"), ("generale", "gen"), ("gen", "gen"), ("gal", "gen"), ("maréchal", "mal"), ("mal", "mal"),
    ("professor", "prof"), ("professeur", "prof"), ("professore", "prof"), ("prof", "prof"),
    ("président", "pdt"), ("presidente", "pdt"), ("pdt", "pdt"),
];

const ROMANCE_FR: &[&str] = &["de", "du", "des", "d", "la", "le", "les", "l", "au", "aux"];

pub const LANGS: &[Lang] = &[
    Lang {
        code: "de",
        words: &[("straße", ""), ("str", ""), ("platz", "plaza"), ("pl", "plaza")],
        first: &[],
        last: &[],
        suffixes: &[("straße", ""), ("str", ""), ("platz", "plaza"), ("gasse", "gasse"), ("gässle", "gässle"), ("gässli", "gässli"), ("weg", "weg"),
            ("allee", "allee"), ("ring", "ring"), ("damm", "damm"), ("ufer", "ufer"), ("kai", "kai"), ("steig", "steig"), ("stieg", "stieg"), ("steg", "steg"),
            ("pfad", "pfad"), ("chaussee", "chaussee"), ("zeile", "zeile"), ("lände", "lände"), ("gürtel", "gürtel"), ("markt", "markt"), ("graben", "graben"),
            ("promenade", "promenade"), ("passage", "passage")],
        particles: &[],
    },
    Lang {
        code: "lb",
        words: &[("strooss", ""), ("plaz", "plaza")],
        first: &[],
        last: &[],
        suffixes: &[("strooss", ""), ("wee", "wee")],
        particles: &[],
    },
    Lang {
        code: "nl",
        words: &[("straat", ""), ("str", ""), ("plein", "plaza"), ("pln", "plaza"), ("pl", "plaza"), ("ln", "laan"), ("gr", "gracht"), ("kd", "kade"),
            ("sngl", "singel"), ("stwg", "steenweg")],
        first: &[],
        last: &[],
        suffixes: &[("straat", ""), ("str", ""), ("plein", "plaza"), ("laan", "laan"), ("weg", "weg"), ("gracht", "gracht"), ("kade", "kade"), ("singel", "singel"),
            ("dijk", "dijk"), ("dreef", "dreef"), ("steeg", "steeg"), ("pad", "pad"), ("markt", "markt"), ("plantsoen", "plantsoen"), ("burgwal", "burgwal"),
            ("baan", "baan"), ("haven", "haven"), ("steenweg", "steenweg"), ("lei", "lei"), ("vest", "vest"), ("kaai", "kaai")],
        particles: &[],
    },
    Lang {
        code: "fr",
        words: &[("rue", ""), ("avenue", "avenue"), ("av", "avenue"), ("ave", "avenue"), ("bd", "boulevard"), ("bld", "boulevard"), ("boul", "boulevard"),
            ("bvd", "boulevard"), ("place", "plaza"), ("pl", "plaza"), ("ch", "chemin"), ("chem", "chemin"), ("imp", "impasse"), ("all", "allée"), ("rte", "route"),
            ("crs", "cours"), ("pass", "passage"), ("sq", "square"), ("fg", "faubourg"), ("fbg", "faubourg"), ("mte", "montée"), ("sent", "sentier"),
            ("esp", "esplanade"), ("prom", "promenade"), ("rés", "résidence"), ("ham", "hameau"), ("chée", "chaussée"), ("carr", "carrefour"), ("lot", "lotissement")],
        first: &[("r", "")],
        last: &[],
        suffixes: &[],
        particles: ROMANCE_FR,
    },
    Lang {
        code: "it",
        words: &[("via", ""), ("vle", "viale"), ("piazza", "plaza"), ("pza", "plaza"), ("pzza", "plaza"), ("ple", "piazzale"), ("pzle", "piazzale"),
            ("pzta", "piazzetta"), ("cso", "corso"), ("lgo", "largo"), ("vlo", "vicolo"), ("vic", "vicolo"), ("str", "strada"), ("lmare", "lungomare"),
            ("sal", "salita"), ("cda", "contrada"), ("ctr", "contrada"), ("loc", "località"), ("fraz", "frazione"), ("bgo", "borgo"), ("fta", "fondamenta"),
            ("trav", "traversa"), ("gall", "galleria"), ("circ", "circonvallazione"), ("ss", "strada statale"), ("sp", "strada provinciale"),
            ("sr", "strada regionale"), ("sc", "strada comunale")],
        first: &[("v", "")],
        last: &[],
        suffixes: &[],
        particles: &["di", "del", "della", "dello", "delle", "dei", "degli", "da", "dal", "dalla", "dai", "il", "lo", "la", "le", "i", "gli", "l", "d", "e", "al", "alla", "ai"],
    },
    Lang {
        code: "rm",
        words: &[("via", ""), ("plazza", "plaza")],
        first: &[],
        last: &[],
        suffixes: &[],
        particles: &["da", "dal", "dals", "la", "il"],
    },
    Lang {
        code: "es",
        words: &[("calle", ""), ("cl", ""), ("cll", ""), ("avenida", "avenue"), ("av", "avenue"), ("avd", "avenue"), ("avda", "avenue"), ("plaza", "plaza"),
            ("pl", "plaza"), ("pza", "plaza"), ("plza", "plaza"), ("po", "paseo"), ("pso", "paseo"), ("ps", "paseo"), ("ctra", "carretera"), ("cno", "camino"),
            ("cmno", "camino"), ("rda", "ronda"), ("trva", "travesía"), ("trav", "travesía"), ("gta", "glorieta"), ("cjon", "callejón"), ("bulevar", "boulevard"),
            ("blvr", "boulevard"), ("rbla", "rambla"), ("psje", "pasaje"), ("urb", "urbanización"), ("pol", "polígono"), ("cta", "cuesta"), ("ptda", "partida")],
        first: &[("c", "")],
        last: &[],
        suffixes: &[],
        particles: &["de", "del", "la", "las", "los", "el", "y", "d", "l"],
    },
    Lang {
        code: "ca",
        words: &[("carrer", ""), ("cr", ""), ("avinguda", "avenue"), ("av", "avenue"), ("avda", "avenue"), ("avgda", "avenue"), ("plaça", "plaza"),
            ("pl", "plaza"), ("pça", "plaza"), ("passeig", "paseo"), ("pg", "paseo"), ("psg", "paseo"), ("rbla", "rambla"), ("travessera", "travesía"),
            ("travessia", "travesía"), ("trav", "travesía"), ("ctra", "carretera"), ("camí", "camino"), ("rda", "ronda"), ("passatge", "pasaje"),
            ("ptge", "pasaje"), ("carreró", "callejón")],
        first: &[("c", "")],
        last: &[],
        suffixes: &[],
        particles: &["de", "del", "dels", "d", "l", "la", "les", "els", "el", "i"],
    },
    Lang {
        code: "gl",
        words: &[("rúa", ""), ("avenida", "avenue"), ("av", "avenue"), ("praza", "plaza"), ("pr", "plaza"), ("pz", "plaza"), ("estrada", "carretera"),
            ("cmño", "camiño"), ("travesa", "travesía"), ("trav", "travesía"), ("rolda", "ronda"), ("ruela", "callejón")],
        first: &[("r", "")],
        last: &[],
        suffixes: &[],
        particles: &["de", "do", "da", "dos", "das", "o", "a", "os", "as"],
    },
    Lang {
        code: "eu",
        words: &[("kalea", ""), ("kale", ""), ("etorbidea", "avenue"), ("etorb", "avenue"), ("enparantza", "plaza"), ("pasealekua", "paseo"), ("pas", "paseo"),
            ("bidea", "camino"), ("errepidea", "carretera"), ("zeharkalea", "travesía")],
        first: &[],
        last: &[("k", "")],
        suffixes: &[],
        particles: &[],
    },
    Lang {
        code: "pt",
        words: &[("rua", ""), ("avenida", "avenue"), ("av", "avenue"), ("avda", "avenue"), ("praça", "plaza"), ("pç", "plaza"), ("pr", "plaza"), ("pça", "plaza"),
            ("lg", "largo"), ("lgo", "largo"), ("tv", "travessa"), ("trav", "travessa"), ("estr", "estrada"), ("cç", "calçada"), ("bc", "beco"), ("al", "alameda"),
            ("rot", "rotunda"), ("pct", "praceta"), ("cam", "caminho"), ("qta", "quinta"), ("urb", "urbanização"), ("pq", "parque")],
        first: &[("r", "")],
        last: &[],
        suffixes: &[],
        particles: &["de", "da", "do", "das", "dos", "d", "e"],
    },
    Lang {
        code: "pl",
        words: &[("ulica", ""), ("ul", ""), ("al", "aleja"), ("aleje", "aleja"), ("plac", "plaza"), ("pl", "plaza"), ("os", "osiedle"), ("bulwar", "boulevard"),
            ("bulw", "boulevard"), ("wyb", "wybrzeże")],
        first: &[],
        last: &[],
        suffixes: &[],
        particles: &[],
    },
    Lang {
        code: "cs",
        words: &[("ulice", ""), ("ul", ""), ("náměstí", "plaza"), ("nám", "plaza"), ("tř", "třída"), ("nábř", "nábřeží")],
        first: &[],
        last: &[],
        suffixes: &[],
        particles: &[],
    },
    Lang {
        code: "sk",
        words: &[("ulica", ""), ("ul", ""), ("námestie", "plaza"), ("nám", "plaza"), ("tr", "trieda"), ("nábr", "nábrežie")],
        first: &[],
        last: &[],
        suffixes: &[],
        particles: &[],
    },
    Lang {
        code: "hu",
        words: &[("utca", ""), ("tér", "plaza"), ("krt", "körút"), ("rkp", "rakpart"), ("ltp", "lakótelep")],
        first: &[],
        last: &[("u", "")],
        suffixes: &[],
        particles: &[],
    },
    Lang {
        code: "sl",
        words: &[("ulica", ""), ("ul", ""), ("trg", "plaza")],
        first: &[],
        last: &[("c", "cesta")],
        suffixes: &[],
        particles: &[],
    },
    Lang {
        code: "hr",
        words: &[("ulica", ""), ("ul", ""), ("trg", "plaza"), ("avenija", "avenue"), ("av", "avenue")],
        first: &[],
        last: &[],
        suffixes: &[],
        particles: &[],
    },
    Lang {
        code: "ro",
        words: &[("strada", ""), ("str", ""), ("bulevardul", "boulevard"), ("bulevard", "boulevard"), ("bd", "boulevard"), ("bdul", "boulevard"),
            ("blvd", "boulevard"), ("cal", "calea"), ("al", "aleea"), ("piața", "plaza"), ("pța", "plaza"), ("sos", "șoseaua"), ("spl", "splaiul"),
            ("intr", "intrarea"), ("fund", "fundătura"), ("prel", "prelungirea")],
        first: &[],
        last: &[],
        suffixes: &[],
        particles: &[],
    },
    Lang {
        code: "bg",
        words: &[("улица", ""), ("ул", ""), ("булевард", "boulevard"), ("бул", "boulevard"), ("площад", "plaza"), ("пл", "plaza"),
            ("ulitsa", ""), ("ul", ""), ("bulevard", "boulevard"), ("bul", "boulevard"), ("ploshtad", "plaza"), ("pl", "plaza")],
        first: &[],
        last: &[],
        suffixes: &[],
        particles: &[],
    },
    Lang {
        code: "el",
        words: &[("οδός", ""), ("οδ", ""), ("λεωφόρος", "avenue"), ("λεωφ", "avenue"), ("πλατεία", "plaza"), ("πλ", "plaza"), ("παρ", "πάροδος"),
            ("odos", ""), ("od", ""), ("leoforos", "avenue"), ("leof", "avenue"), ("plateia", "plaza"), ("platia", "plaza"), ("plat", "plaza")],
        first: &[],
        last: &[],
        suffixes: &[],
        particles: &[],
    },
    Lang {
        code: "tr",
        words: &[("sokak", ""), ("sokağı", ""), ("sk", ""), ("caddesi", "avenue"), ("cad", "avenue"), ("cd", "avenue"), ("meydanı", "plaza"), ("bulvarı", "boulevard")],
        first: &[],
        last: &[],
        suffixes: &[],
        particles: &[],
    },
    Lang {
        code: "lv",
        words: &[("iela", ""), ("bulvāris", "boulevard"), ("bulv", "boulevard"), ("prospekts", "avenue"), ("pr", "avenue"), ("prosp", "avenue"), ("laukums", "plaza")],
        first: &[],
        last: &[],
        suffixes: &[],
        particles: &[],
    },
    Lang {
        code: "lt",
        words: &[("gatvė", ""), ("prospektas", "avenue"), ("pr", "avenue"), ("al", "alėja"), ("aikštė", "plaza"), ("kel", "kelias"), ("pl", "plentas"),
            ("skg", "skersgatvis"), ("tak", "takas"), ("krant", "krantinė"), ("aklg", "akligatvis")],
        first: &[],
        last: &[("g", "")],
        suffixes: &[],
        particles: &[],
    },
    Lang {
        code: "et",
        words: &[("tänav", ""), ("tn", ""), ("mnt", "maantee"), ("pst", "puiestee"), ("pk", "põik"), ("väljak", "plaza"), ("vlk", "plaza"), ("plats", "plaza")],
        first: &[],
        last: &[],
        suffixes: &[],
        particles: &[],
    },
    Lang {
        code: "fi",
        words: &[("katu", "")],
        first: &[],
        last: &[("k", ""), ("t", "tie")],
        suffixes: &[("katu", ""), ("tie", "tie"), ("kuja", "kuja"), ("polku", "polku"), ("väylä", "väylä"), ("tori", "tori"), ("aukio", "aukio"),
            ("kaari", "kaari"), ("raitti", "raitti"), ("rinne", "rinne"), ("ranta", "ranta"), ("puistikko", "puistikko"), ("puisto", "puisto"), ("piha", "piha"),
            ("silta", "silta"), ("laituri", "laituri")],
        particles: &[],
    },
    Lang {
        code: "sv",
        words: &[("gatan", ""), ("gata", ""), ("vägen", "väg"), ("gränden", "gränd"), ("stigen", "stig"), ("torget", "torg"), ("platsen", "plats"),
            ("backen", "backe"), ("allén", "allé"), ("kajen", "kaj"), ("leden", "led"), ("slingan", "slinga"), ("esplanaden", "esplanad"), ("stranden", "strand")],
        first: &[],
        last: &[("g", ""), ("v", "väg")],
        suffixes: &[("gatan", ""), ("gata", ""), ("vägen", "väg"), ("väg", "väg"), ("gränden", "gränd"), ("gränd", "gränd"), ("stigen", "stig"), ("torget", "torg"),
            ("torg", "torg"), ("platsen", "plats"), ("backen", "backe"), ("allén", "allé"), ("kajen", "kaj"), ("leden", "led"), ("slingan", "slinga"),
            ("esplanaden", "esplanad"), ("stranden", "strand"), ("ringen", "ring"), ("plan", "plan")],
        particles: &[],
    },
    Lang {
        code: "no",
        words: &[("gate", ""), ("gata", ""), ("gaten", ""), ("gt", ""), ("veien", "vei"), ("vegen", "vei"), ("veg", "vei"), ("plassen", "plass"), ("torget", "torg"),
            ("stien", "sti"), ("bakken", "bakke"), ("bryggen", "brygge"), ("kaia", "kai")],
        first: &[],
        last: &[("g", ""), ("v", "vei")],
        suffixes: &[("gata", ""), ("gaten", ""), ("gate", ""), ("veien", "vei"), ("vegen", "vei"), ("vei", "vei"), ("veg", "vei"), ("plassen", "plass"), ("plass", "plass"),
            ("allé", "allé"), ("torget", "torg"), ("torg", "torg"), ("stien", "sti"), ("bakken", "bakke"), ("bakke", "bakke"), ("bryggen", "brygge"),
            ("brygge", "brygge"), ("kaia", "kai"), ("ringen", "ring"), ("svingen", "sving"), ("tunet", "tun")],
        particles: &[],
    },
    Lang {
        code: "da",
        words: &[("gade", ""), ("pl", "plads"), ("blvd", "boulevard")],
        first: &[],
        last: &[("g", ""), ("v", "vej")],
        suffixes: &[("gade", ""), ("vej", "vej"), ("allé", "allé"), ("plads", "plads"), ("stræde", "stræde"), ("torv", "torv"), ("vænge", "vænge"),
            ("bakke", "bakke"), ("kaj", "kaj"), ("park", "park"), ("boulevard", "boulevard")],
        particles: &[],
    },
    Lang {
        code: "is",
        words: &[("gata", "")],
        first: &[],
        last: &[],
        suffixes: &[("gata", ""), ("vegur", "vegur"), ("braut", "braut"), ("stígur", "stígur"), ("torg", "torg"), ("stræti", "stræti"), ("tún", "tún"),
            ("hlíð", "hlíð"), ("gerði", "gerði"), ("hagi", "hagi")],
        particles: &[],
    },
    Lang {
        code: "en",
        words: &[("street", ""), ("rd", "road"), ("ave", "avenue"), ("av", "avenue"), ("ln", "lane"), ("pl", "place"), ("sq", "square"), ("tce", "terrace"),
            ("cres", "crescent"), ("ct", "court"), ("cl", "close"), ("pk", "park"), ("gdns", "gardens"), ("pde", "parade")],
        first: &[],
        last: &[("st", ""), ("dr", "drive")],
        suffixes: &[],
        particles: &["the"],
    },
    Lang {
        code: "ga",
        words: &[("sráid", ""), ("bóthar", "road"), ("ascaill", "avenue"), ("lána", "lane"), ("cearnóg", "square"), ("plás", "place"), ("ardán", "terrace")],
        first: &[],
        last: &[],
        suffixes: &[],
        particles: &["an", "na"],
    },
    Lang {
        code: "mt",
        words: &[("triq", ""), ("tq", ""), ("pjazza", "plaza"), ("misraħ", "plaza"), ("vjal", "avenue"), ("sqaq", "sqaq"), ("xatt", "xatt")],
        first: &[],
        last: &[],
        suffixes: &[],
        particles: &["il", "l", "ir", "is", "it", "iz", "id", "in", "ix", "iċ", "ta", "tal", "tad", "tas", "tat", "taz", "tan", "tar", "tax"],
    },
];

/// Street words that open a French street name: in Belgium and Luxembourg their addresses put the number first.
pub const FRENCH_FIRST: &[&str] = &["rue", "avenue", "av", "boulevard", "bd", "place", "pl", "chaussée", "chemin", "quai", "square", "impasse", "allée",
    "route", "drève", "clos", "sentier", "cours", "passage", "rond-point", "esplanade", "galerie", "venelle", "voie", "cité", "résidence", "parvis",
    "promenade", "montée", "faubourg", "val", "ruelle", "r"];

/// Words after a house number that say which flat or floor: they and the short word after them are not part of the town.
pub const UNITS: &[&str] = &["apt", "app", "appt", "apartment", "appartement", "wohnung", "whg", "top", "stiege", "stg", "tür", "etage", "étage", "et", "etg",
    "esc", "escalier", "bât", "bat", "bâtiment", "bloc", "piso", "puerta", "pta", "izq", "izqda", "izda", "izquierda", "dcha", "dcho", "dch", "derecha", "int",
    "interno", "scala", "sc", "esq", "esquerdo", "dto", "direito", "rc", "bajo", "ático", "lgh", "lok", "lokal", "mieszk", "m", "byt", "flat", "unit",
    "suite", "tv", "th", "mf", "kl", "sal"];

/// Words that stand before a house number and are not part of it: Nr. 5, n° 5, No 5.
pub const NUMBER_MARKS: &[&str] = &["nr", "no", "n", "num", "nro", "numero", "número", "nummer", "č", "cp", "#"];
