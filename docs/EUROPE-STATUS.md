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

## Done and checked

| Step | What | Checked by |
|---|---|---|
| Regions | 110 regions with boxes from Eurostat NUTS 2024 (`crates/tools`, `region-boxes`); Portugal three boxes, Norway without Svalbard | tests: unique codes, the 31 countries, capitals inside their region, overseas parts outside |
| Identity | name, download host, manifest and update addresses in one place (`dist.rs`) | tests |
| Setup | manifest, downloader (own HTTPS client, pinned roots built in, resume with ETag), installer (check, unpack beside, switch at restart), hardware, `/api/setup/*` ([SETUP-API.md](SETUP-API.md)) | 49 engine tests, 9 of them whole flows against a local HTTPS server; on macOS, Windows and Ubuntu in CI |
| The app's start | settings and data folders on each system, the remembered library, setup when there is none, the system's file picker, a restart after setup | snapshots of the app (setup page) |
| Setup's region choice | country, then region for a split country | snapshots: Germany → region list, Bremen → sizes, Liechtenstein → one choice |
| Local AI runner | `lastbook-llm` on llama.cpp: JSON lines, no sockets; llama.cpp's fit; "from the disk" placement for models larger than the memory | protocol tests in CI; a small Qwen 3.5 mixture-of-experts test model on Metal; Qwen3.6-35B-A3B Q8_0 (37.8 GB) loads on a 16 GB Mac with its weights read from the disk |
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

## Being done now

- The data tools in Rust (`crates/tools`) for the map: the detailed map, places, water, buildings, terrain and the
  address index of a region, and the region map and address index as setup installs them; first Liechtenstein.
- The library's tools: a library and its search index from a list of documents, the pack setup installs; a small
  starter library in Italian, German and English.
- The engine's remaining English sentences become codes the interface translates; setup's disk kinds, the "library
  cannot be reached" card and banner, and the choice of where an AI model goes, in the six languages.

## Left, in order

1. The whole path in Italian with Liechtenstein: setup, map, addresses, chat, photographed in the app.
2. The other test regions (Malta, Luxembourg, Bremen), then all regions built by the tools on GitHub's machines.
3. The release checks: the pinned roots cover the download hosts, nothing ships that must not.
4. A faster "from the disk": experts kept in memory by use, only the missing ones read (BigMoeOnEdge, llama.cpp's expert
   streaming).

## Test models

AI tests use model files already on the maintainer's computer; no model is downloaded for them:
Qwen3.6-35B-A3B Q8_0 (37.8 GB, a mixture of experts: on a 16 GB Mac it runs from the disk) and small Qwen 3.5 test
models.
