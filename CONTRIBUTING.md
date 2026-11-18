# Contributing to Lastbook Europe

Thank you for helping. Lastbook Europe is meant for the day the internet is gone, so what matters most is that it works
offline, stays correct, and never sends anything anywhere.

## Set up

1. Install Rust (stable), CMake and a C/C++ compiler; on Linux also the WebKitGTK and `libclang` packages listed in the
   README.
2. `cargo build` builds the app (`crates/app`), the engine (`crates/engine`), the local AI runner (`crates/llm`) and the
   data tools (`crates/tools`).
3. `cargo run -p lastbook-app` opens the app. With no library it opens setup; `--data /path/to/LastbookData` opens a
   library you have.

The README explains the layout and how to build the installers.

## Test

```bash
cargo test --workspace
```

Run it before you open a pull request; GitHub Actions runs it on macOS, Windows and Ubuntu. If you change something a
person sees, also look at it in the running app (`lastbook --snapshot shot.png`) in light and dark, and in a long
language such as German. If you change the packaging, build the installer for your system.

## Rules that keep Lastbook Europe what it is

**No network.** The app opens no port and makes no connection, except setup's downloads, started by the person, through
`crates/engine/src/setup/net.rs`, to the hosts in `crates/engine/src/dist.rs` only, trusting only the roots built into
the program.

- No telemetry, analytics, crash reporting, fonts or scripts from the web, or "phone home" of any kind.
- Our own pages (`web/`) contain no web address.
- A new dependency must work fully offline and must not open a socket or start a server. Say in the pull request what it
  is, its licence, and why it is needed, and add it to `THIRD_PARTY_NOTICES.md`.
- The local AI runner stays socket-free: no server, no download code, nothing of the engine linked in.

**Europe, in plain words.** Write for any reader in any of the 31 countries: 112 for emergencies, metric units, the
country's own way of writing addresses. Name a country or region only when the source is about it. Interface text is
short, concrete and calm, in every language: many readers will be stressed, tired, or reading in a second language.

**Sources first.** A guide card points at a real page of a real source in the library, with its page number. If the
library does not cover something, record that in `content/gaps.json` instead of writing advice from memory.

**No personal details.** Nothing shared may name a real person's home, street, coordinates, machines or accounts. Test
paths use placeholders such as `/path/to/LastbookData`.
