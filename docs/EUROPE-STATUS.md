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

## Being done now

- The map, places and European address search (`crates/engine/src/map/`), with per-country address tests for all 31
  countries.
- The library: catalog, search, shelves, videos, PDF pages, encyclopedias, saved pages (`crates/engine/src/library/`).
- The interface in six languages (`web/i18n.js`, `web/locales/`), metric only, 112.

## Left, in order

1. The local AI in the engine: model files, checks, the three-way fit (in memory, from the disk, too big) with the disk
   measurement, the runner's process; then the assistant with citations, and the chat by polling.
2. The data tools in Rust and the four test regions (Liechtenstein, Malta, Luxembourg, Bremen): map, places, addresses,
   terrain, and a small European library.
3. The whole path in Italian with Liechtenstein: setup, map, addresses, chat, photographed in the app.
4. Snapshots on Windows and Linux, so CI photographs the app on all three systems; the update check; the release
   checks (pinned roots cover the hosts, nothing that must not ship).
5. A faster "from the disk": experts kept in memory by use, only the missing ones read (BigMoeOnEdge, llama.cpp's expert
   streaming).

## Test models

AI tests use model files already on the maintainer's computer; no model is downloaded for them:
Qwen3.6-35B-A3B Q8_0 (37.8 GB, a mixture of experts: on a 16 GB Mac it runs from the disk) and small Qwen 3.5 test
models.
