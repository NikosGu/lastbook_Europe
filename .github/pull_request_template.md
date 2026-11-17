## What this changes

<!-- One or two sentences. Link the issue if there is one. -->

## How I checked it

<!-- Tests you ran, and screenshots for anything a person sees (light and dark; Mac, Windows and/or Linux). -->

## Checklist

- [ ] `cargo test --workspace` passes.
- [ ] No new network access: no port, socket, web address in `web/`, telemetry or new allowed host. Network use only through setup's downloader (`crates/engine/src/setup/net.rs`).
- [ ] No personal details: no home area, coordinates, user or machine names, LAN addresses, keys or tokens.
- [ ] Guide changes cite real pages in the library.
- [ ] Interface text is in all the languages, in plain words.
- [ ] Any new dependency is offline-only and listed in `THIRD_PARTY_NOTICES.md` with its licence.
