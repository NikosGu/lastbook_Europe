# Lastbook Europe

Lastbook Europe is an offline survival library for Europe, for macOS, Windows and Linux. It puts practical guides,
manuals, videos, encyclopedias, an offline map with help points and address search, and an optional local AI on one
computer. After setup it works with no internet. The AI answers from the library and links every answer to its pages.

It covers the 31 countries of the EU and EFTA (the 27 EU countries, Switzerland, Norway, Iceland and Liechtenstein) in
110 regions. The interface is in English, Italian, German, French, Spanish and Polish; more languages will follow.

The app is written in Rust; the interface is plain HTML, CSS and JavaScript.

> **Status: under construction.** The window, setup with the region choice, the map with European address search,
> the library, the local AI with its chat, and the installers are written in Rust and tested on macOS, Windows and Linux.
> The tools that build the European map data and a first small library are being written now; the library packs,
> region maps and models are not published yet. What is done, what is left and what is waiting for a decision is in
> [docs/EUROPE-STATUS.md](docs/EUROPE-STATUS.md).

## Download and install

Installers are on the [Releases](https://github.com/NikosGu/lastbook_Europe/releases) page. They are built on GitHub's
machines from this repository by [.github/workflows/release.yml](.github/workflows/release.yml).

| System | File | Install |
|---|---|---|
| macOS 13 or later, Apple silicon | `LastbookEurope-<version>-macos-arm64.dmg` | Open the disk image and drag **Lastbook Europe** to Applications. The app is signed ad hoc, not with a paid Apple Developer ID, so the first time macOS asks: on macOS 15 and later open **System Settings → Privacy & Security** and choose **Open Anyway**; on macOS 13 and 14, Control-click the app and choose **Open**. |
| Windows 10 and 11, x64 | `LastbookEurope-<version>-windows-x64-setup.exe` | Run it; no administrator rights are needed. The installer carries no paid code-signing certificate, so SmartScreen may warn the first time: choose **More info → Run anyway**. A `.zip` to unpack anywhere is there too. |
| Linux x86-64 (Debian, Ubuntu and similar) | `lastbook-europe_<version>_amd64.deb` | `sudo apt install ./lastbook-europe_<version>_amd64.deb`, then open **Lastbook Europe** from the menu. A `.tar.gz` is there too; it needs WebKitGTK 4.1. |

The installers are small (13 to 31 MB): the library, the maps and the AI model are chosen and downloaded by setup, or
taken from files already on the computer.

## What it does

- **Guides first.** Short guides for urgent needs (`content/topics/`), written for Europe in the interface's six
  languages, each pointing to real pages of the library's sources, with page numbers. What the library does not cover
  yet is said plainly, not filled in from memory.
- **The library.** Manuals and PDFs, videos with their transcripts, saved web pages and offline encyclopedias (ZIM
  files), with search everywhere.
- **The offline map.** A base map of Europe and a detailed map of the region chosen in setup: streets, buildings, terrain,
  water, and places to get help (hospitals, pharmacies, drinking water, shelters). Addresses can be found offline, written
  the way each country writes them. Distances are metric. The person chooses the region; Lastbook never detects location.
- **The local AI, optional.** Ask in plain words, in your language, and get an answer that cites the library's pages,
  from a model running on the computer itself (see below). Without it, the chat still finds the right pages.
- **Setup in one step.** On first run, setup shows this computer's free space and memory, suggests a library and the
  largest AI model that fits, and downloads what the person chooses, checking every file. A library or a model file
  already on the computer can be used instead of downloading.

## Where the library can live

On this computer, or anywhere else it can reach: an external disk, a USB stick, or a NAS on the home network (by Wi-Fi
or cable, through SMB or NFS; a mapped drive or a `\\server\share` folder on Windows). Setup lists the disks it finds,
says which kind each is, and downloads to the one chosen; the app then reads the library from there.

- Keep that disk or network connected. If it goes away, the app says so plainly and goes on when it is back; at start,
  it offers to try again or to choose another library.
- The AI model can stay beside the library or on this computer. When the library is on a network share or a stick, setup
  suggests this computer: a model of many gigabytes loads slowly over a network, and running a model "from the disk"
  needs a fast local SSD. The assistant always reads its sources from the open library, wherever it is.
- The library's databases are read without locks, which network file systems handle badly, and are never written while
  the app runs.

## The local AI

The model is never part of the app. The person adds one in setup, or points setup at a GGUF model file already on the
computer. Lastbook knows each model it offers by name, size and SHA-256, and checks the file before using it.

**Which models.** Lastbook prompts two families, telling them apart by the model file's own header: **Qwen 3.5 / 3.6**
(including mixtures of experts such as Qwen3.6-35B-A3B) and **Gemma 4**. The answers cite the library in every
language.

**Does it fit this computer?** Lastbook asks llama.cpp to fit the model to this computer's memory, then gives one of
three answers:

- **In memory.** The model fits. It runs on the graphics processor where there is one: Metal on a Mac, Vulkan on Windows
  and Linux (with the graphics card's own driver; without one, the best variant for the processor). `lastbook-llm
  --devices` lists what it found.
- **From the disk.** A mixture-of-experts model larger than the memory can still run: each word uses only a few of its
  experts, so the experts stay in the model file and are read from the disk as they are needed (the file is mapped, not
  copied into memory). It is much slower. It is offered only as an explicit choice with a warning, only on an SSD and only
  after a short measurement on that computer, and it is never the default. On a Mac this mode runs on the CPU, because
  llama.cpp gives Metal the mapped file from the first to the last of its tensors, which there spans the experts too.
  Qwen3.6-35B-A3B at 8 bits (37.8 GB) on a 16 GB Mac this way: about 20 words a second read, one written.
- **Too big.** Setup says so in plain words, and why.

**How answers stay with the library.** The model has no internet: it sees only the rules, the conversation and the
library's pages the app gives it, and it may ask for more through four tools (search the library, read whole pages,
read a video's transcript, look up a place and the nearest help), twice at most. What the app makes sure of, whatever
the model does:

- Every sentence must end with the key of its source (`[S2]`). Each key is checked against the pages the tools really
  returned in that answer, with a page number that exists; a key the model made up is removed.
- Every number in a cited sentence (doses, times, temperatures, quantities) must appear in the pages that sentence cites,
  as figures or as words in any of the six languages; a sentence whose number is not there is marked.
- Sentences without a source are marked, and what the library does not cover, where sources disagree and the model's own
  reasoning are shown apart.
- A document's text can never act as an instruction or start a turn: it reaches the model as plain text only.

What is not guaranteed: that a sentence says exactly what its page says. A model also knows what it learned in training,
and can mistranslate a source written in another language. That is why every answer links the exact pages, and says
it can be wrong.

**The runner.** `lastbook-llm` (`crates/llm`) is a small Rust program on [llama.cpp](https://github.com/ggml-org/llama.cpp),
through the `llama-cpp-2` crate (`third_party/llama-cpp-2` adds one setting to it, see its `PROVENANCE.md`). It runs one
model, talks to the app only through its standard input and output (one JSON object per line), opens no socket and
contains no network code. It stops when unused, and when the app closes.

Next: the chat with citations in Rust, then a faster "from the disk" mode that keeps the most used experts in memory and
reads only the missing ones, as [BigMoeOnEdge](https://github.com/Helldez/BigMoeOnEdge) and llama.cpp's expert
streaming work do.

## Privacy and network use

- **No listening port.** The window (`crates/app`, on [tao](https://github.com/tauri-apps/tao) and
  [wry](https://github.com/tauri-apps/wry)) shows the interface in the system's own web view: WKWebView on a Mac,
  WebView2 on Windows, WebKitGTK on Linux. It talks to the engine through the app's own address scheme
  (`lastbook://app/`; on Windows `http://lastbook.localhost/`, which WebView2 hands to the app without any network), never
  a network port. Pages from anywhere else are blocked.
- **One use of the network, started by the person.** Setup's downloader (`crates/engine/src/setup/net.rs` and
  `download.rs`) is the only code that connects anywhere: HTTPS on port 443, only to the hosts in
  `crates/engine/src/dist.rs` (this project's releases on github.com, where GitHub serves their files, and Hugging Face
  for the AI model; on github.com, this repository's release files only), no proxy from the system, and a redirect only
  to another allowed host. The update check, when asked, reads one small file the same way.
- **A local AI that cannot reach the network.** The runner imports no network function, and llama.cpp is built without
  its model downloader (`crates/llm/llama/`); `lastbook-tools scan-package` checks every package for it.
- **Pinned trust.** The downloader trusts only the root certificates in `crates/engine/src/setup/roots.pem`, built into
  the program, never the system's trust store.
- **Checked files.** Every download is checked against the SHA-256 published for it before it is used. Zips are checked
  for unsafe paths, links and sizes before anything is written.
- **A working library is never broken.** New data is unpacked beside the old one, checked, and switched in when the app
  next starts. A failure at any step leaves the old library as it was.
- **Nothing else.** No telemetry, no account, and nothing that reaches the network on its own.

## What's where

| Path | What it is |
|---|---|
| `crates/engine` | The engine: the request router, setup (manifest, downloader, installer, hardware), the European regions, the map, the library, the local AI and the assistant. |
| `crates/app` | The desktop window and the app's start: which library opens, setup when there is none, the system's file picker. |
| `crates/llm` | `lastbook-llm`, the local AI runner, and `llama/`: how llama.cpp is built without its model downloader. |
| `crates/tools` | The data tools, one command, `lastbook-tools`: a region's map and address index (`build-map`, `pack-map`, `pack-addresses`, `build-terrain`), a library and its pack (`build-library`, `build-pack`), and the regions' boxes from Eurostat's NUTS boundaries (`region-boxes`). |
| `third_party/llama-cpp-2` | The `llama-cpp-2` crate with one addition (see `PROVENANCE.md`). |
| `web/` | The interface, in plain HTML, CSS and JavaScript, with no build step and nothing loaded from the web. |
| `content/` | The guides (`topics/*.json` in English, `topics/<lang>/` in the other languages; rules in `SCHEMA.md`), what the library does not cover (`gaps.json`), the words search uses in each language (`search/`), and the lists of documents libraries are built from (`library/`). |
| `packaging/rust/` | The installers for macOS, Windows and Linux. |
| `docs/` | How a region's map is built (`MAPS.md`), how addresses are searched (`ADDRESSES.md`), the setup contract (`SETUP-API.md`) and the state of the work (`EUROPE-STATUS.md`). |

## Build and run from source

You need Rust (stable, 1.85 or later), CMake and a C/C++ compiler (llama.cpp is built from source).

- **macOS:** the Xcode command line tools.
- **Windows:** Visual Studio's C++ tools and LLVM (for `libclang`); for the installer, [Inno Setup](https://jrsoftware.org/isinfo.php).
- **Linux (Debian, Ubuntu):** `sudo apt install build-essential cmake libclang-dev libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev`

```bash
cargo build --release
cargo run --release -p lastbook-app                                   # opens setup when there is no library
cargo run --release -p lastbook-app -- --data /path/to/LastbookData   # opens this library
```

Without `--data`, the app opens the library named by `LASTBOOK_DATA`, else the one chosen last time, else its own; with
none, it opens setup. The runner is built as `target/release/lastbook-llm`, beside the app, where the app looks for it.

A region's map and address index, from public data (OpenStreetMap through Geofabrik, Copernicus DEM, Natural Earth;
[docs/MAPS.md](docs/MAPS.md) says which files and where they come from):

```bash
cargo run --release -p lastbook-tools -- build-map --region LI --gpkg <geofabrik .gpkg.zip> --pbf <geofabrik .osm.pbf> \
    --dem <Copernicus DEM tile>... --ne <Natural Earth folder> --out derived/maps/LI
cargo run --release -p lastbook-tools -- pack-map --map derived/maps/LI/map --out derived/packs
cargo run --release -p lastbook-tools -- pack-addresses --index derived/maps/LI/addresses/LI.sqlite --out derived/packs
```

A library, from a list of documents with their sizes and SHA-256 (`content/library/starter.json`); PDFium reads the
pages (`LASTBOOK_PDFIUM` names its folder):

```bash
cargo run --release -p lastbook-tools -- build-library --sources content/library/starter.json --docs <folder of the documents> \
    --out derived/packs/starter/LastbookData
cargo run --release -p lastbook-tools -- build-pack --data derived/packs/starter/LastbookData --out derived/packs
```

That library is for this computer: several of its documents may not be passed on. The pack that may be published holds
only the documents whose terms let them be passed on (`"redistribute": "yes"` in the list, with the terms' address and
the sentence that decides): `build-library --redistributable-only`. The library workflow
([.github/workflows/library.yml](.github/workflows/library.yml), run by hand) builds it on GitHub's machines and uploads
it to the data release.

The packers (`pack-map`, `pack-addresses`, `build-pack`) print the manifest entry setup needs. Setup also installs
packs it finds on the computer, so a region and a library built here can be tried without a download site.

The installers:

```bash
bash packaging/rust/package_macos.sh                         # dist/LastbookEurope-<version>-macos-arm64.dmg
bash packaging/rust/package_linux.sh                         # dist/lastbook-europe_<version>_amd64.deb and a .tar.gz
powershell -File packaging\rust\package_windows.ps1          # dist\LastbookEurope-<version>-windows-x64-setup.exe and a .zip
```

## Tests

```bash
cargo test --workspace
LASTBOOK_TEST_MODEL=/path/to/a-small-model.gguf cargo test -p lastbook-llm --release   # the runner with a real model
```

The tests make their own small data: the setup tests run whole downloads against a local HTTPS server, and need no
library and no model. GitHub Actions runs them on macOS, Windows and Ubuntu at every push
([.github/workflows/tests.yml](.github/workflows/tests.yml)).

Before a release, two checks run on GitHub's machines (the release workflow): `lastbook-tools check-roots`, which
connects to every host setup downloads from through the app's own pinned roots and makes sure a site outside them is
refused, and `lastbook-tools scan-package <package>`, which fails if the local AI's runner or llama.cpp's libraries
could reach the network, or if anything that must not ship is in the package.

What a person sees is also checked in the running app: `lastbook --snapshot shot.png [--eval JS]...` drives the page and
saves a picture of it, on macOS, Windows and Linux; CI photographs the app on each.

## What is not in this repository

- **The library**: manuals, videos and encyclopedias, each under its own terms, published as packs with a Sources and
  permissions page in each.
- **Maps and models**: large, and built from public data or published by their makers. Setup fetches them and checks them.
- **Generated data and builds** (`derived/`, `dist/`, `target/`), and **signing material**.

## Credits and licence

Lastbook Europe started from [Lastbook](https://github.com/Immortal-Protocols/lastbook), whose interface it keeps, and is
rewritten in Rust for Europe, with guides written anew from European sources. Its code and documentation use the
[MIT License](LICENSE), which keeps Lastbook's notice. Third-party software keeps its own licence: see
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). The regions' boxes are made from Eurostat's NUTS 2024 boundaries,
© EuroGeographics for the administrative boundaries.

See [CONTRIBUTING.md](CONTRIBUTING.md) and [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
