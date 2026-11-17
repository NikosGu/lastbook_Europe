# Address indexes and address search

How "Find a place" finds a street address in Europe, and the file that holds the addresses. The search is in
`crates/engine/src/map/address/`; the builder is `map-data` in `crates/tools` (see "Building one"). Nothing here
opens a connection: an index is a file installed with a region map, and the search only reads it.

## The file

One SQLite file per region: `derived/geo/addresses/<REGION>.sqlite`, where `<REGION>` is the region's code from the
registry (`LI`, `DE-HB`, `FR-IDF`; see `crates/engine/src/regions.rs`). As with region maps, a region's index holds
every address inside the region's boxes, so it also holds a strip of each neighbour: Liechtenstein's index holds
addresses in Switzerland and Austria. Each row therefore says which country it is in, and its keys are made by that
country's rules.

### `addr`: one row per address

The first eight columns are the original Lastbook's American index, kept; four are new.

| Column | What it holds | Example |
|---|---|---|
| `num` | The house number as the source writes it | `12`, `12a`, `12 bis`, `846/1`, `14-16` |
| `street` | The street as the source writes it, shown to people | `Hauptstraße`, `rue de la Paix`, `Via del Corso` |
| `norm` | The street's key: `street_key(street, country)` | `haupt`, `paix`, `corso` |
| `city` | The town or locality, shown | `Vaduz`, `Praha 1`, `Il-Belt Valletta` |
| `state` | The first-level subdivision as the source names it, or `''`; shown nowhere, searched by nothing | `Bayern` |
| `zip` | The postcode's key: `postcode_key(postcode, country)` | `9490`, `1012AB`, `00950`, `1000001`, `D02X285` |
| `lat`, `lon` | WGS 84 degrees | |
| `country` | ISO 3166-1 alpha-2 code of the country the address is in | `LI`, `CH`, `AT` |
| `postcode` | The full postcode as the source writes it, shown | `1012 AB`, `00-950`, `D02 X285`, `LV-1050` |
| `numkey` | The house number's key: `number_key(num)` | `12`, `12a`, `12bis`, `846/1`, `14-16` |
| `citykey` | The town's key: `text::name_key(city)` | `vaduz`, `praha 1`, `il belt valletta` |

An address with two numbers may be written once under each. Czech and Slovak buildings carry a conscription number
and an orientation number (`846/1`); people use either, so a builder should write the row three times, with `num`
`846/1`, `846` and `1`. The search shows each match once.

The `zip` of the original Lastbook's index was the first five digits of a US ZIP code. Here it is the whole postcode, as a
key; `postcode` keeps it as written.

### `area`: one row per postcode and town

Made from `addr` once every address is in (see "Building one"): the middle of the addresses of each postcode and town,
so a postcode or a town typed alone can be shown without reading every address.

| Column | What it holds |
|---|---|
| `country`, `zip`, `postcode`, `citykey`, `city` | As in `addr` |
| `lat`, `lon` | The average of those addresses |
| `n` | How many addresses |

### Indexes

- `addr_num_norm(numkey, norm)`: a house number on a street, the search's main question.
- `addr_norm(norm, numkey)`: a street without a number.
- `area_zip(zip)`, `area_city(citykey)`.

### `meta`

| Key | Value |
|---|---|
| `schema` | `2` |
| `region` | The region code |
| `countries` | The countries the rows are in, comma separated (`LI,CH,AT`). Without it the search assumes the region's own country only. |
| `publisher` | A short source name shown with each result (`Overture Maps`, `BEV Adressregister`). Without it: `Overture Maps`. |
| `source`, `release`, `count` | Where the addresses come from, the source's release and the number of rows |

An index without the new columns (the original Lastbook's American files) is passed over by the search, not an error.

## Building one

The engine gives the builder what it needs, so the keys are the ones the search makes:

1. Create the tables: `lastbook_engine::map::address::SCHEMA`.
2. For each address, `Address { num, street, city, state, postcode, country, lat, lon }.insert(&connection)`. It
   computes `norm`, `zip`, `numkey` and `citykey`.
3. Run `lastbook_engine::map::address::FINISH`: the indexes and the `area` table.
4. Write `meta`.

The tests in `crates/engine/src/map/address/tests.rs` build their tiny indexes exactly this way, and so does the
builder, `crates/tools/src/addresses.rs`, run by `map-data region <REGION>` ([MAPS.md](MAPS.md), "Building a region"),
from the `addr:*` tags of the region's OpenStreetMap PBF:

- An address is a node, or a way or relation (mostly a building) at the middle of its outline. One mapped on both,
  close together, is kept once. Only what the extract holds is there: Geofabrik's extracts stop at the border, so
  Liechtenstein's index has a few Austrian addresses by the border and no Swiss ones.
- The country is `addr:country`; without it, the region's own country when the address lies inside it (else the
  address is left to its own country's index).
- The town is `addr:city`, written as the region's own municipality or place of that name (`MAuren` is Mauren). A rare
  name the region does not know (`Balzers (FL)`, a street typed in the wrong field), used fewer than three times, gives
  way to the municipality the address lies in, as does a missing one.
- One row per number: each of `12;14`; a Czech or Slovak `846/1` also as `846` and `1`.
- After `FINISH`, a postcode–town pair given by fewer than three addresses, under 2% of the postcode's main town, is
  taken out of `area` (a postcode typed for the wrong town would otherwise be offered as a town of it). The addresses
  themselves stay.

`meta.publisher` is `OpenStreetMap`, `meta.source` names the Geofabrik extract and its date.

## The keys

All keys start from `text::fold`, which follows `fold()` in `web/words.js` (lower case, NFKD, accent marks on Latin
letters removed, NFC, final ς as σ) with two additions: Greek accents go too, and letters with no accent to remove are
written out (ß ss, æ ae, œ oe, ø o, ł l, đ and ð d, þ th, ħ h, ı i). Then `text::loose` brings the second spelling of a
letter back to the first: ae, oe, ue, aa and dj become a, o, u, a and d. So both spellings of a name give one key:
Straße and Strasse, München, Munchen and Muenchen, Tromsø and Tromsoe, Ålborg and Aalborg, Łódź and Lodz. The price is
that a few different names share a key (Muester and Muster); the house number and the town keep such streets apart.

`street_key(street, country)` then uses the country's street words (`address/words.rs`, chosen by the country's
languages in `address/countries.rs`):

- Abbreviations are written out: Str. Straße, v.le viale, C/ calle, ul. ulica, Pº paseo, mnt maantee, ул. улица.
  Single letters count only where they are used: first (C/, V., R.) or last (Pilies g., Váci u., Main St).
- The word every street of a language has is dropped: Straße, straat, rue, via, calle, carrer, rua, ulica, ulice,
  utca, iela, gatvė, tänav, katu, gata, gatan, gate, gade, strada (Romania), улица, οδός, triq, street. So "Hauptstraße",
  "Hauptstr." and "Haupt" find the same street, and "ul. Marszałkowska" finds Marszałkowska, written without "ulica" in
  Polish data.
- The other street words stay, so Via Roma and Piazza Roma, Hauptstraße and Hauptweg, Váci utca and Váci út are
  different streets. Squares share one key in every language (`plaza`: Platz, plein, place, piazza, plaça, plac,
  náměstí...), as do avenues and boulevards, so an abbreviation two languages of one country share (pl., av., bd)
  means one thing in Belgium, Switzerland or Spain.
- Compound names are split at their street word, which then reads as above: Bahnhofstraße and Bahnhofstr. are
  `bahnhof`, Sveavägen is `svea vag`, Mannerheimintie is `mannerheimin tie`, Laugavegur is `lauga vegur`. "Berliner
  Straße" and "Berlinerstraße" meet.
- The small words of Romance and Maltese names are dropped: Rue de la Paix is `paix`, Via della Spiga is `spiga`, Triq
  il-Kbira is `kbira`.
- Titles are one: Saint, Sankt, Sint and St; Docteur, Doktor and Dr.

`number_key` makes `12 bis` `12bis`, `12 A` and `12/A` `12a`, `10.` `10`, and keeps `12/3` and `12-14`.

`postcode_key` takes out a country prefix, spaces and dashes: `L-1611` `1611`, `1012 AB` `1012AB`, `00-950` `00950`.

## How a query is read

The query is read every way it can be, and the readings are tried in turn, the most precise first; the first that finds
addresses gives the answer (`address::parse`).

- The house number before the street (`12 rue de la Paix`, `12, rue de la Gare`) or after it (`Via Roma 12`, `Calle
  Mayor, 5`), in every country: the order a country writes is only how results are shown.
- A postcode in the shape of any of the 31 countries, written as people write it, with or without its prefix:

  | Shape | Countries |
  |---|---|
  | `999` | IS |
  | `9999` | AT, BE, BG, CH, CY, DK, HU, LI (9485–9498), LU (`L-1611` too), LV (`LV-1050` too), NO, SI |
  | `99999` | DE, EE, ES, FI, FR, HR, IT, LT (`LT-01100` too) |
  | `999 99` | CZ, GR, SE, SK (with or without the space) |
  | `999999` | RO |
  | `9999 AA` | NL (with or without the space; the four digits alone name an area) |
  | `99-999` | PL |
  | `9999-999` | PT (the four digits alone name an area) |
  | `AAA 9999` | MT |
  | `A99 XXXX` | IE, Eircode (the routing key alone names an area) |

  A country's old prefixes are read too (`D-80331`, `A-1010`, `CH-8001`, `FL-9490`, `F-75002`, `I-00184`).
- The town before or after the street, with or without commas. Every word typed is part of the street or the town:
  a wrong town or postcode finds nothing, never the same street elsewhere.
- A country's name or code at the end (`…, Liechtenstein`, `…, Österreich`, `…, AT`) limits the search to it. Without
  a comma before it, the query is also read as if the name were part of the street (`Corso Italia`).
- `Nr.`, `n°`, `No` before the number are dropped; so are a flat or a floor after it (`Top 5`, `2. tv`, `3º izq`,
  `lgh 1201`, `m. 3`), and a district number after the town (`Praha 1`, `Dublin 2`).
- Only exact house numbers are returned: 12a is never 12. A flat or a staircase after a slash (`12/3/14`) or the end of
  a range (`12-14`) is the same building, and is tried after the exact number.

Results are shown as people of the country write them: `Via Roma 12`, `12 rue de la Paix` (France, Ireland, Malta,
and French street names in Belgium and Luxembourg), `Calle Mayor, 5` (Spain, Portugal), with `9490 Vaduz,
Liechtenstein`, `Dublin 2, D02 X285, Ireland` or `Rīga, LV-1050, Latvia` below.

## What `/api/places` answers

The same array as before (`name`, `kind`, `detail`, `lat`, `lon`, `source`, `onMap`), so the page reads it unchanged.
The kinds from the address indexes are `Street address`, `Street` (a street without a number: the middle of its
addresses in each town), `Postcode` and `Town` (a postcode or a town alone, when the place index has nothing better).

The answer's `X-Query-Kind` header says how the engine read the query: `address`, `postcode` or `place`. The page
decides today whether a query "is an address" by testing whether it starts with a number (`web/map.js`, `findPlace`,
the line that sets `address` before the "not found" message), which is American: "Via Roma 12" does not start with a
number. The page should read the header instead:

```js
const res = await fetch('/api/places?q=' + encodeURIComponent(q) + `&near=${st.lat.toFixed(3)},${st.lon.toFixed(3)}`);
const r = await res.json();
// ...
const address = res.headers.get('X-Query-Kind') === 'address';
```

and its messages should say "postcode" rather than "5-digit ZIP", and "your region's address search" rather than "your
state's".

## Not covered yet

- Greek and Bulgarian streets typed in Latin letters (Ermou for Ερμού): a builder may add a row under a transliterated
  name.
- House names without numbers (common in Ireland and Malta).
- In Denmark "St." can be Store or Sankt; it is read as Sankt.
