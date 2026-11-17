# Lastbook Europe: where the work stands

What is done and checked, what is being done now, what is left, and what only the maintainer can decide. Kept up to date
with each step.

## The direction

- **Europe only:** the 31 countries of the EU and EFTA, in 110 regions (`crates/engine/src/regions.rs`): 26 countries
  whole, and Germany, France, Italy, Spain and Poland by first-level ISO 3166-2 subdivision.
- **Everything in Rust:** the engine, setup, the window, the local AI runner and the data tools. The interface stays
  plain HTML, CSS and JavaScript in `web/`.
- **Six languages to start:** English, Italian, German, French, Spanish and Polish; the others in waves.
- **Local AI:** a catalogue of models; "from the disk" for mixtures of experts larger than the memory, as an explicit
  choice with a warning, on an SSD, after a measurement.
- **Checked where a person sees it:** besides the tests, the running app is photographed (`lastbook --snapshot`) and
  the picture is looked at.
- **A repository of its own** (since 17 November 2026): not a fork; installers published as GitHub releases.

## Decisions waiting for the maintainer

None of these blocks the work: each has a placeholder.

| Decision | Placeholder now | Where it is used |
|---|---|---|
| Project name and domain | `Lastbook Europe`, no domain | `crates/engine/src/dist.rs` (`NAME`, `WEBSITE`), the window's title, the installers |
| Where the library packs, region maps, address indexes, models and manifest are hosted (GitHub release assets, up to 2 GB a file, are one option) | `downloads.lastbook-europe.invalid`, which never resolves | `crates/engine/src/dist.rs`, the pinned roots |
| Contact address for removal requests | none | each pack's Sources page |
| Signing certificates (Apple Developer ID, Windows Authenticode) | builds signed ad hoc on a Mac, unsigned on Windows | `packaging/rust/` |
| Native-speaker review of the Italian, German, French, Spanish and Polish text | translations marked for review | `web/locales/`, `content/` |
| Publishing the first release | each push makes a **draft** release | GitHub's Releases page |
| Whether the starter library may be published: many documents state no licence, the eight French, Spanish and Polish ones among them (BBK and WHO reserve their rights, BBK allowing quotations; IFRC allows non-commercial copies) | built here, not published | `content/library/starter.json` (`about`, and each document's terms) |

## Decisions taken along the way

| When | Decision | Why |
|---|---|---|
| 17 Nov 2026 | The app opens on "What do you need?" (the search and the guides), with Home first in the sidebar; videos keep their page | the original opened on its videos; the starter library has none |
| 17 Nov 2026 | Setup suggests the smallest available library, whatever its name, and leaves 5% of the disk free in all (between 1 and 5 GB) | it looked for a pack named "small", and two margins added together turned down a 43 MB library on a disk with 7 GB free |
| 17 Nov 2026 | Vulkan on Windows and Linux in the installers, built with Ninja in a short folder on Windows, llama.cpp's libraries copied beside the runner; local package builds keep the processor unless LASTBOOK_VULKAN=1 | Cargo's newer folder layout, Visual Studio's generator and Windows' 260-character paths each broke the first builds; the release now checks that the packaged runner finds the processor's backend |
| 17 Nov 2026 | Regions are built with their neighbours' extracts by default, read only inside the region's boxes, so the strip around a region is mapped too: Liechtenstein's map goes from 12 to 32 MB to download, its addresses from 0.6 to 4 MB (85,488, Feldkirch's and the Swiss Rhine valley's among them) | the hospitals and pharmacies across a border are often the nearest; an empty strip around the region looked like the end of the world |
| 17 Nov 2026 | Luxembourg's address index keeps its streets' French, Luxembourgish and German names: 59 MB installed, 15 MB to download, instead of about a third | addresses really use them (75%, 33% and 6% of the streets that carry them) |
| 17 Nov 2026 | The local AI writes its choices and log in the app's own folder when the library's folder cannot be written | a library on a read-only network share or stick must keep its AI |
| 17 Nov 2026 | No Liechtenstein document in the starter library yet | llv.li refuses automated downloads, and that refusal is respected |
| 17 Nov 2026 | First aid in the starter library comes from the IFRC guidelines, in English | no Italian first-aid document from a public body was found |
| 17 Nov 2026 | Spain's Interior Ministry and Education Ministry sites, which refuse automated downloads, are left alone: the Spanish guides come from proteccioncivil.es | as with llv.li |
| 17 Nov 2026 | A picture's description from an accessible PDF is kept in the page texts as "[Alt text: …]", for the tools and the chat's model, and the reader shows it under "Picture description" in the person's language | one fixed mark is easy to recognise in the data; people read their own language |
| 17 Nov 2026 | Guides the starter library cannot support (growing food, preserving, repairs, home energy, land) keep an honest intro and their search words, with no readings; what is missing is in `content/gaps.json` | a guide gives only what its documents say: nothing from memory, nothing from the old US library |

## Done and checked

| Step | What | Checked by |
|---|---|---|
| Regions | 110 regions with boxes from Eurostat NUTS 2024 (`crates/tools`, `region-boxes`); Portugal three boxes, Norway without Svalbard | tests: unique codes, the 31 countries, capitals inside their region, overseas parts outside |
| Identity | name, download host, manifest and update addresses in one place (`dist.rs`) | tests |
| Setup | manifest, downloader (own HTTPS client, pinned roots built in, resume with ETag), installer (check, unpack beside, switch at restart), hardware, `/api/setup/*` ([SETUP-API.md](SETUP-API.md)) | 49 engine tests, 9 of them whole flows against a local HTTPS server; on macOS, Windows and Ubuntu in CI |
| The app's start | settings and data folders on each system, the remembered library, setup when there is none, the system's file picker, a restart after setup | snapshots of the app (setup page) |
| Setup's region choice | country, then region for a split country | snapshots: Germany → region list, Bremen → sizes, Liechtenstein → one choice |
| Local AI runner | `lastbook-llm` on llama.cpp: JSON lines, no sockets; llama.cpp's fit; Metal on a Mac; on Windows and Linux, Vulkan for graphics cards or the best processor variant, as separate libraries beside the runner, each loaded only where it can run (`--devices` lists what it found); "from the disk" for models larger than the memory, with each layer's chosen experts read ahead | protocol tests in CI; a small Qwen 3.5 mixture-of-experts test model on Metal, and from the disk with experts read ahead writing exactly the same; Qwen3.6-35B-A3B Q8_0 (37.8 GB) loads on a 16 GB Mac with its weights read from the disk |
| Prompt formats | Qwen 3.5/3.6 (ChatML, XML tool calls) and Gemma 4, chosen by the model file's header | tests against the chat templates |
| Installers and releases | `.dmg` (12.7 MB), Inno Setup installer (16.2 MB) and `.zip`, `.deb` (16.2 MB) and `.tar.gz`, each with PDFium, and Vulkan's backends on Windows and Linux; a draft release on each push | the `.dmg` opened and photographed here; the release workflow on GitHub |
| Windows | downloads, paths and file locks fixed after the first run on GitHub's Windows machines | CI |
| The map | vector tiles, terrain, buildings, help pins, the nearest help in metres, place and lake search with both spellings of a name, and address search for the 31 countries (word order, postcodes, street words, house numbers) ([ADDRESSES.md](ADDRESSES.md)); drugstores apart from pharmacies, which keep the green cross; the date of the data and the credits taken from the installed map | 62 tests, one per country; the app photographed on a small Vaduz data folder ("Aeulestr. 10, 9490 Vaduz" found) |
| The library | catalog, guides, search with the interface's language preferred (words for six languages in `content/search/`), shelves and videos, PDF pages through PDFium, saved pages and encyclopedias (ZIM) cleaned, the library's files with their checks | 47 tests; the app photographed on a small library (search, a PDF page, an article) |
| The local AI in the engine | the model files the catalogue offers, their SHA-256, the three-way fit with a disk speed test, the runner started on a question and stopped when unused | tests, among them a whole cycle with a small real model |
| The assistant and the chat | the question searched at once, tools (search, read pages, transcripts, places and nearest help), every citation checked against what the tools returned, the answer in the person's language; the chat streamed to the page by polls | tests with a scripted model |
| Check for updates | only when asked; nothing while the download site is the placeholder | tests against a local HTTPS server |
| Pictures on all three systems | `--snapshot` with WKWebView, WebView2 and WebKitGTK; CI photographs the app on each | the pictures from GitHub's Windows and Ubuntu machines, looked at |
| Six languages | the interface in English, Italian, German, French, Spanish and Polish (1,271 keys each, the five translations marked for review), chosen in setup and the app, kept, changed in place; metric only; Europe on the map; what the engine says (setup, the map, the library, the local AI, updates) comes as a code with its numbers, which the interface words in the person's language, the English kept for the log | `tools/check_locales.mjs`; a test in each engine module that every code has words; pictures in Italian, German, Spanish and Polish |
| Storage anywhere | a library and its models on this computer, an external disk, a USB stick or a NAS (SMB, NFS, mapped drives, UNC paths); disk kinds shown in setup; SQLite read immutable; a library out of reach said plainly at start and while running; models beside the library or on this computer; a library on a read-only share or stick keeps its local AI, whose choices and log go to the app's own folder | tests on each system, and a library read through a network path on Windows in CI |
| The map's data tools | `lastbook-tools build-map`, `pack-map`, `pack-addresses`, `build-terrain`: the detailed map from Geofabrik's GeoPackage, addresses and buildings from OpenStreetMap's PBF, places and water, elevation from Copernicus DEM GLO-30, the zoomed-out base from Natural Earth ([MAPS.md](MAPS.md)); Liechtenstein built in 3 seconds: 32,191 map features, 441 help points, 12,499 addresses, 12 MB to download | 23 tools tests; packs checked by setup's own zip check; the map, an address and a postcode photographed |
| Regions on GitHub's machines | `.github/workflows/regions.yml`, run by hand with a list of region codes: one job per region, its Geofabrik extracts from a table of all 110 (`crates/tools/data/region-sources.json`), its DEM tiles and Natural Earth, then build, pack and `check-map` (setup's own checks on the zips, the region's town found, addresses above a floor, the first address found as typed, detail in every box); the neighbours' extracts read only inside the region's boxes; the place search ranks another name as the place's own ("Gozo" finds Għawdex first) and brings no lakes for a postcode | 33 tools tests and 222 engine tests; the script run here without downloads for Liechtenstein and Malta, all checks passed |
| The test regions | Liechtenstein, Malta (36 islands from OpenStreetMap's coastline, postcodes as "VLT 1117", streets found in Maltese and English), Luxembourg ("2, place Guillaume II, L-1648 Luxembourg", streets in French, Luxembourgish and German) and Bremen (two pieces, Bremerhaven's 22,679 addresses in): maps 8 to 66 MB to download, address indexes 0.3 to 15 MB | 30 tools tests, among them 31 real questions asked of the three built regions; each region photographed in Italian |
| The library's tools | `lastbook-tools build-library` and `build-pack`: each document checked against its size and SHA-256, page texts through PDFium (words split at a line end joined again), and on a page with few words of its own (an infographic) what a tagged PDF says its pictures show, its alt text, marked "[Alt text: …]" in the page texts and searched without the mark; the search index, a Sources page; the starter library: 22 public documents in the six languages (BWL, Protezione Civile, BBK, Zivilschutz, SGDSN, Protección Civil, the Polish government and RCB, IFRC, WHO), 930 pages, 82 MB to download; PDFium in each installer, pinned by SHA-256 | tests with and without PDFium, one on a tagged PDF the test makes; the release photographs each packaged app drawing a PDF page; a Spanish search for a word only an infographic's alt text carries, followed to its page, photographed |
| The whole path in Italian with Liechtenstein | setup finds the three packs on this computer, suggests the starter library, Liechtenstein chosen, installed and opened; the map of Vaduz with relief, help points and the nearest help in metres; "Städtle 11, Vaduz" and "9490" found; the chat with Qwen3.6-35B-A3B Q8_0 (37.8 GB) from the disk on a 16 GB Mac, offered after its file was checked and chosen by a click: "how much water per person" answered in Italian with each point cited to the Swiss federal leaflet's pages (9 litres a person, for 3 days), and "the nearest pharmacy to Städtle 11" through the offline map (Schloss Apotheke, 0.27 km south) | photographed in the app on this Mac |
| "From the disk", measured | Qwen3.6-35B-A3B Q8_0 on a 16 GB Mac, processor only, experts read ahead: a question read at about 20 words a second (3,558 in 174 s), an answer written at 0.9 to 1.4 words a second; an answer with a map look-up took about 6 minutes in all; the model loads in 4 seconds, and its 37.8 GB file is checked once, in about 2 minutes | the runner's own numbers in the log |
| The guides for Europe | fourteen guides rewritten on the starter library: water, food, power cuts, heat and cold, disasters, first aid, health, toilets and hygiene, warnings and calls; 316 readings and warnings, each on a real page of a document the library holds; in the six languages (the five translations marked for review), each guide in the interface's language where it is translated, the French, Spanish and Polish ones opening pages in their own language wherever a page there says the same; what the library lacks, and where its sources disagree, in `content/gaps.json` | engine tests (every reading on a page of a listed document, every translation with the English items, each on the same page or a page in its own language); the app photographed in Italian, and the water guide in French, Spanish and Polish |

## Being done now

- The regions workflow's first runs on GitHub's machines: the four test regions with their neighbours' extracts, then
  large regions (Bavaria, Lombardy, a Nordic country) to find the limits of a runner's memory and disk.

## Left, in order

1. All 110 regions run through the regions workflow, their packs published where the maintainer chooses to host them.
2. The release checks: the pinned roots cover the download hosts, nothing ships that must not.
3. "From the disk" faster: about a word a second is slow for long answers. Next, the same model at 4 bits (about 21 GB,
   closer to a 16 GB Mac's memory), the graphics processor for the layers that fit, and a cache of experts kept by use,
   as BigMoeOnEdge does; each measured as above.
4. Native-speaker review of the five translations of the interface and the guides.
5. Pictures with no alt text: of the ten Spanish readings on infographic pages, seven now have the infographics' alt
   text; the three on the weather guide (Fenómenos meteorológicos, pages 17 and 20), a PDF with no tags, need OCR.

## Test models

AI tests use model files already on the maintainer's computer; no model is downloaded for them:
Qwen3.6-35B-A3B Q8_0 (37.8 GB, a mixture of experts: on a 16 GB Mac it runs from the disk) and small Qwen 3.5 test
models.
