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
| Whether the starter library may be published: several documents state no licence (BBK and WHO reserve their rights, BBK allowing quotations; IFRC allows non-commercial copies) | built here, not published | `content/library/starter.json` (`about`, and each document's terms) |

## Decisions taken along the way

| When | Decision | Why |
|---|---|---|
| 17 Nov 2026 | The app opens on "What do you need?" (the search and the guides), with Home first in the sidebar; videos keep their page | the original opened on its videos; the starter library has none |
| 17 Nov 2026 | Setup suggests the smallest available library, whatever its name, and leaves 5% of the disk free in all (between 1 and 5 GB) | it looked for a pack named "small", and two margins added together turned down a 43 MB library on a disk with 7 GB free |
| 17 Nov 2026 | Windows and Linux run the local AI on the processor until Vulkan builds on GitHub's machines; Vulkan is a build option tried in a CI job of its own | the first Vulkan builds failed there (Cargo's newer folder layout on Linux, Visual Studio's generator on Windows) |
| 17 Nov 2026 | No Liechtenstein document in the starter library yet | llv.li refuses automated downloads, and that refusal is respected |
| 17 Nov 2026 | First aid in the starter library comes from the IFRC guidelines, in English | no Italian first-aid document from a public body was found |
| 17 Nov 2026 | Guides the starter library cannot support (growing food, preserving, repairs, home energy, land) keep an honest intro and their search words, with no readings; what is missing is in `content/gaps.json` | a guide gives only what its documents say: nothing from memory, nothing from the old US library |

## Done and checked

| Step | What | Checked by |
|---|---|---|
| Regions | 110 regions with boxes from Eurostat NUTS 2024 (`crates/tools`, `region-boxes`); Portugal three boxes, Norway without Svalbard | tests: unique codes, the 31 countries, capitals inside their region, overseas parts outside |
| Identity | name, download host, manifest and update addresses in one place (`dist.rs`) | tests |
| Setup | manifest, downloader (own HTTPS client, pinned roots built in, resume with ETag), installer (check, unpack beside, switch at restart), hardware, `/api/setup/*` ([SETUP-API.md](SETUP-API.md)) | 49 engine tests, 9 of them whole flows against a local HTTPS server; on macOS, Windows and Ubuntu in CI |
| The app's start | settings and data folders on each system, the remembered library, setup when there is none, the system's file picker, a restart after setup | snapshots of the app (setup page) |
| Setup's region choice | country, then region for a split country | snapshots: Germany → region list, Bremen → sizes, Liechtenstein → one choice |
| Local AI runner | `lastbook-llm` on llama.cpp: JSON lines, no sockets; llama.cpp's fit; Metal on a Mac, the processor on Windows and Linux (Vulkan, for their graphics cards, is a build option being made to build on GitHub's machines, in a job of its own); "from the disk" for models larger than the memory, with each layer's chosen experts read ahead | protocol tests in CI; a small Qwen 3.5 mixture-of-experts test model on Metal, and from the disk with experts read ahead writing exactly the same; Qwen3.6-35B-A3B Q8_0 (37.8 GB) loads on a 16 GB Mac with its weights read from the disk |
| Prompt formats | Qwen 3.5/3.6 (ChatML, XML tool calls) and Gemma 4, chosen by the model file's header | tests against the chat templates |
| Installers and releases | `.dmg` (6.5 MB), Inno Setup installer and `.zip`, `.deb` and `.tar.gz`; a draft release on each push | the `.dmg` opened and photographed here; the release workflow on GitHub |
| Windows | downloads, paths and file locks fixed after the first run on GitHub's Windows machines | CI |
| The map | vector tiles, terrain, buildings, help pins, the nearest help in metres, place and lake search with both spellings of a name, and address search for the 31 countries (word order, postcodes, street words, house numbers) ([ADDRESSES.md](ADDRESSES.md)) | 62 tests, one per country; the app photographed on a small Vaduz data folder ("Aeulestr. 10, 9490 Vaduz" found) |
| The library | catalog, guides, search with the interface's language preferred (words for six languages in `content/search/`), shelves and videos, PDF pages through PDFium, saved pages and encyclopedias (ZIM) cleaned, the library's files with their checks | 47 tests; the app photographed on a small library (search, a PDF page, an article) |
| The local AI in the engine | the model files the catalogue offers, their SHA-256, the three-way fit with a disk speed test, the runner started on a question and stopped when unused | tests, among them a whole cycle with a small real model |
| The assistant and the chat | the question searched at once, tools (search, read pages, transcripts, places and nearest help), every citation checked against what the tools returned, the answer in the person's language; the chat streamed to the page by polls | tests with a scripted model |
| Check for updates | only when asked; nothing while the download site is the placeholder | tests against a local HTTPS server |
| Pictures on all three systems | `--snapshot` with WKWebView, WebView2 and WebKitGTK; CI photographs the app on each | the pictures from GitHub's Windows and Ubuntu machines, looked at |
| Six languages | the interface in English, Italian, German, French, Spanish and Polish (990 keys each, the five translations marked for review), chosen in setup and the app, kept, changed in place; metric only; Europe on the map; setup's errors translated by code | `tools/check_locales.mjs`; pictures in Italian, German, Spanish and Polish |
| Storage anywhere | a library and its models on this computer, an external disk, a USB stick or a NAS (SMB, NFS, mapped drives, UNC paths); disk kinds shown in setup; SQLite read immutable; a library out of reach said plainly at start and while running; models beside the library or on this computer | tests on each system, and a library read through a network path on Windows in CI |
| The map's data tools | `lastbook-tools build-map`, `pack-map`, `pack-addresses`, `build-terrain`: the detailed map from Geofabrik's GeoPackage, addresses and buildings from OpenStreetMap's PBF, places and water, elevation from Copernicus DEM GLO-30, the zoomed-out base from Natural Earth ([MAPS.md](MAPS.md)); Liechtenstein built in 3 seconds: 32,191 map features, 441 help points, 12,499 addresses, 12 MB to download | 23 tools tests; packs checked by setup's own zip check; the map, an address and a postcode photographed |
| The library's tools | `lastbook-tools build-library` and `build-pack`: each document checked against its size and SHA-256, page texts through PDFium (words split at a line end joined again), the search index, a Sources page; the starter library: 14 public documents in Italian, German and English (BWL, Protezione Civile, BBK, Zivilschutz, IFRC, WHO), 707 pages, 39 MB to download; PDFium in each installer, pinned by SHA-256 | tests with and without PDFium; the release photographs each packaged app drawing a PDF page |
| The path in Italian with Liechtenstein | setup finds the three packs on this computer, suggests the starter library, Liechtenstein chosen, installed and opened; the map of Vaduz with relief, help points and the nearest help in metres; "Städtle 11, Vaduz" and "9490" found | photographed in the app on this Mac |
| The guides for Europe | fourteen guides rewritten on the starter library: water, food, power cuts, heat and cold, disasters, first aid, health, toilets and hygiene, warnings and calls; 316 readings and warnings, each on a real page of a document the library holds; in the six languages (the five translations marked for review), each guide in the interface's language where it is translated; what the library lacks, and where its sources disagree, in `content/gaps.json` | engine tests (every reading on a page of a listed document, every translation opening the same pages); the app photographed in Italian |

## Being done now

- The other test regions: Malta, Luxembourg, Bremen.
- The engine's remaining English sentences become codes the interface translates; setup's disk kinds, the "library
  cannot be reached" card and banner, and the choice of where an AI model goes, in the six languages.

## Left, in order

1. The chat in the Liechtenstein path, with Qwen3.6-35B-A3B from the disk, when the computer's memory is free (it is
   shared with other work now, and a model read from the disk needs the room for the pages it reads).
2. All regions built by the tools on GitHub's machines. A region's box reaches into its neighbours, whose country
   extracts are separate: the strip around a region stays empty until the build takes the neighbours' extracts too.
3. The map page's date of the data and its credits from the installed map, not fixed words.
4. The release checks: the pinned roots cover the download hosts, nothing ships that must not.
5. Vulkan for graphics cards on Windows and Linux, once it builds on GitHub's machines.
6. Measure "from the disk" with Qwen3.6-35B-A3B on a 16 GB Mac; if it is too slow, a cache of experts of its own, kept by
   use, as BigMoeOnEdge does.

## Test models

AI tests use model files already on the maintainer's computer; no model is downloaded for them:
Qwen3.6-35B-A3B Q8_0 (37.8 GB, a mixture of experts: on a 16 GB Mac it runs from the disk) and small Qwen 3.5 test
models.
