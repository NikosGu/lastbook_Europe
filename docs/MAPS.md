# How the offline map is put together

Which files the map reads, what is in them, and how a region's files are built and packed from public data. The app
draws only from files on the computer and never asks the network for a tile, a place or an address. The tools that
build the files (the map subcommands of `lastbook-tools`, in `crates/tools`) run on a build machine and read sources
downloaded beforehand; they are never part of the app.

## What the app reads

A region map unpacks into the data folder (`LastbookData`); `map-set.json` at its top lists the files and their sizes.
The engine reads them in `crates/engine/src/map/`. Each file is optional: one that is missing is a layer the map does
not have.

| File | Tables | What it holds | Read by (`crates/engine/src/map/`) |
|---|---|---|---|
| `derived/geo/national.sqlite` | `feat`, `feat_idx`, `poi`, `meta` | Roads, water, land, place names and help points: the zoomed-out base and the region's detail | `tiles.rs`, `pins.rs`, `info.rs` |
| `derived/geo/national-status.json` | | `{state: complete, features}`: the data version tiles are cached under | `info.rs` |
| `derived/geo/places.sqlite` | `place`, `place_fts`, `meta` | The place finder: towns, villages, municipalities, postcodes, peaks, named help points | `places.rs` (`/api/places`) |
| `derived/geo/water.sqlite` | `resource`, `spatial`, `water_fts`, `feat`, `feat_idx`, `meta` | Lakes, reservoirs, ponds and dams as records | `tiles.rs`, `pins.rs`, `places.rs` |
| `derived/geo/buildings.sqlite` | `bldg`, `bldg_osm`, `meta` | Building outlines, and mapped heights for the 3D view | `tiles.rs` |
| `derived/geo/terrain.sqlite` | `tile`, `meta` | Elevation tiles for relief, contours, elevation and the 3D view | `terrain.rs` |
| `derived/geo/addresses/<REGION>.sqlite` | `addr`, `area`, `meta` | Street addresses (installed on its own; [ADDRESSES.md](ADDRESSES.md)) | `address/` |
| `derived/geo/map.sqlite`, `crops.sqlite`, `aerial*.sqlite`, `library-data/geography/resources/atlas.sqlite` | | Layers of the original Lastbook that no European region has yet (a local street map, crops, aerial photos, resource records); `/api/map/aerial` answers 204 without them | |

Zoomed-out vector tiles (zoom 9 and below) are kept in `derived/geo/tilecache/<data version>/` once the map is complete;
that cache is the only thing the map writes.

### national.sqlite

- `feat(id, layer, cls, name, minz, geom)`: `layer` is what the page draws it as (`road`, `rail`, `waterway`, `water`,
  `wet`, `land`, `beach`, `place`), `cls` its kind (`primary`, `river`, `forest`, `town`...), `geom` its parts as
  little-endian int32: the number of parts, then for each part its number of points and the points as lon, lat steps
  in millionths of a degree (`map::tiles::pack`).
- `feat_idx USING rtree(id, minx, maxx, miny, maxy, minz, maxz)`: a feature shows from zoom `minz` to zoom `maxz`. The
  zoomed-out base has `maxz` 9; the region's detail 30.
- `poi(id, cls, name, lon, lat)`, indexed by `(cls, lon, lat)`: the points the pins, the "Nearest" panel and the
  assistant show (`map::HELP` names their kinds).
- `meta`: `source`, `data_as_of`, `region`, `bounds`, `built`, `base_source`.

### buildings.sqlite

- `bldg(x, y, part, n, data)`: every outline, filed under the zoom-14 tile holding its middle (`part` 0: a reader joins
  every part of a tile); `data` is zlib of int32 records `[npts, height_dm, X0, Y0, dX1, dY1, ...]` in zoom-30 pixels
  (2^-30 of the world), the first point whole and the rest as steps, height 0 when the map gives none. An outline
  reaching more than 4096 of those (about 100 m in central Europe) into a neighbouring tile is filed there too; readers
  keep one copy.
- `bldg_osm(x, y, n, data)`: outlines and building parts with a mapped `height` or `building:levels` (3.2 m a storey
  and 1.5 m of roof), for 3D: `[npts, height_dm, min_height_dm, is_part, X0, Y0, ...]`.

### terrain.sqlite

`tile(z, x, y, png)`: 256-pixel web-mercator tiles, zoom 9 to 12, in terrain-RGB PNG
(metres = -10000 + (R * 65536 + G * 256 + B) * 0.1), written with the engine's `map::terrain::terrain_png`. A tile is
written only where the DEM covers all of it. `meta`: `source`, `licence`.

### Search keys

Place and lake names are searched by keys that both spellings of a name share (Straße and Strasse, München and
Muenchen, Łódź and Lodz; `map::text::fold` and `loose`, described in [ADDRESSES.md](ADDRESSES.md)), written with
`map::text::place_key`:

- `places.sqlite`: `place(id, name, kind, detail, lat, lon, rank, source, country, key, area, names)` and
  `place_fts USING fts5(key, area, content='place', content_rowid='id', tokenize='unicode61 remove_diacritics 0')`.
  `key` holds the place's names: its name, and from the PBF its names in its country's languages and the app's six
  (Lussemburgo for an Italian, Luksemburg for a Pole), and its alternative, old, official and short names. Not every
  language: Luxembourg's 150 names made its key so long that the search ranked villages lying in it above the capital.
  `names` holds those other names, a line each: a query that is one of them ranks as the place's own name does (`Gozo`
  is the island Għawdex before the shops called "Gozo …"). A file without the column ranks by `name` alone.
  `area` holds the words that say where it is (its municipality or district, and its country in its own languages and
  English). A postcode's names are the postcode as written and its key (`1012 AB`, `1012AB`). Places mapped only as
  areas (Malta's islands, some suburbs and localities) are there too, at their middle. `rank`: 1 a city or town, 2 a
  village, suburb, island, municipality or postcode, 3 a smaller place or a peak, 4 a named point.
- `water.sqlite`: `water_fts USING fts5(key)`, rowid the `resource` id, `key` the place key of its name.

Older files without a `key` column are still searched, by every way a word may be written in them.

Resource details are metric: `area_ha` (and `region`, `country`); records carrying US fields (`area_acres`,
`normal_storage_af`) are converted when shown.

## Building a region

`lastbook-tools build-map` builds a region's files from four public sources, read from local files:

| Source | What it gives | Licence |
|---|---|---|
| Geofabrik's free GeoPackage of the region (`<region>-latest-free.gpkg.zip`, https://download.geofabrik.de/europe/) | roads, rail, water, land use, place names, help points, municipalities | ODbL 1.0, © OpenStreetMap contributors |
| Geofabrik's PBF of the region (`<region>-latest.osm.pbf`) | addresses, building outlines and heights, other names of places, dams | ODbL 1.0, © OpenStreetMap contributors |
| Copernicus DEM GLO-30, one GeoTIFF a degree (https://copernicus-dem-30m.s3.amazonaws.com/) | elevation | © DLR e.V. 2010-2014 and © Airbus Defence and Space GmbH 2014-2018, provided under COPERNICUS by the European Union and ESA |
| Natural Earth 1:10m (https://naciscdn.org/naturalearth/10m/): `ne_10m_roads`, `ne_10m_populated_places_simple`, `ne_10m_lakes`, `ne_10m_lakes_europe`, `ne_10m_rivers_lake_centerlines`, `ne_10m_rivers_europe` | the zoomed-out base: Europe's main roads, towns, lakes and rivers, zoom 3 to 9 | public domain |

```
cargo run --release -p lastbook-tools -- build-map --region LI \
    --gpkg derived/sources/LI/liechtenstein-free.gpkg.zip --pbf derived/sources/LI/liechtenstein.osm.pbf \
    --dem derived/sources/dem/Copernicus_DSM_COG_10_N47_00_E009_00_DEM.tif \
    --dem derived/sources/dem/Copernicus_DSM_COG_10_N46_00_E009_00_DEM.tif \
    --ne derived/sources/ne --out derived/maps/LI
cargo run --release -p lastbook-tools -- pack-map --map derived/maps/LI/map --out derived/packs
cargo run --release -p lastbook-tools -- pack-addresses --index derived/maps/LI/addresses/LI.sqlite --out derived/packs
```

`build-map` writes `<out>/map/` (the map set: `map-set.json` and `derived/geo/*`) and
`<out>/addresses/<REGION>.sqlite`, and prints what it made; `build-terrain` writes the elevation tiles alone. Each
subcommand prints its options with `--help`. Everything kept lies inside the region's boxes
(`crates/engine/src/regions.rs`); the DEM tiles given must cover them (Liechtenstein's box reaches 46.90°N, so it
needs N46 as well as N47).

The detail is what the extracts hold, and Geofabrik's extracts stop at the border. A region's boxes reach into its
neighbours, so with its own extract alone that strip has only the base map and the elevation: Liechtenstein's map has
no streets on the Swiss bank of the Rhine, Bremen's none in the Lower Saxony land between the city and Bremerhaven
50 km down the Weser. `--gpkg` and `--pbf` take a region's own extracts (as many as it has: Grand Est is Alsace,
Champagne-Ardenne and Lorraine), read whole; `--neighbour-gpkg` and `--neighbour-pbf` take its neighbours', read only
inside its boxes, at a small cost:

- A neighbour's GeoPackage is scanned once (Geofabrik's carry no spatial index), and a row is decoded only if its
  header's envelope reaches into a box.
- A neighbour's PBF gets one more pass, which finds its nodes inside the boxes; a way is kept only if one of its nodes
  is, so memory grows with the boxes, not with Switzerland.
- A feature two extracts hold (a road across the border) comes once, from the region's own. Only the region's own
  extracts say which country an address lies in: a neighbour's national outline is larger and would otherwise be
  taken, so a neighbour's addresses in the strip come only when they say their country.

Tried here without downloading a neighbour: Liechtenstein with Malta as its "neighbour" (nothing of it in the box, 0.7 s
more) and with its own extract twice (everything held twice, once) both give the same zips, byte for byte, as
Liechtenstein alone. The four test regions below were built from their own extracts only.

How each file is made (`crates/tools/src/map/`):

- `detail.rs`: the original Lastbook's layers, classes and zooms from the GeoPackage; an area appears from the zoom
  where it spans about a sixteenth of a tile. `base.rs` adds the Natural Earth base with each feature's own `min_zoom`.
- `coast.rs`: the coast from the PBF's `natural=coastline` ways, which have the land on their left. The page paints the
  sea, then a coarse outline of the world's land (Natural Earth 1:110m, without Malta), then the tiles, where these
  rings are drawn at every zoom: `island` in the land's colour, `ocean` in the sea's. A coast the extract cuts (up the
  Weser to Bremerhaven and Bremen) is closed by a straight line between its cut ends, land or water by its side.
- `places.rs`, `water.rs`, `buildings.rs`, `addresses.rs`, `terrain.rs`: the other files as described above. The PBF
  is read once for all of them, so each takes only its own: buildings are `building` and `building:part` only (a
  town's boundary kept for the place finder is no building). A terrain pixel is the mean of DEM samples spread across
  it: one at zoom 12, 8 × 8 at zoom 9; a tile reaching past the DEM files given is left out (Malta's and Luxembourg's
  edges at zoom 9 and 10). The tiles are made a zoom-9 tile at a time, with every tile under it, from the DEM files
  under it alone: those are read when needed and kept while they fit in 2 GB, so Norway's 300 files (7 GB at once)
  need no more memory than Luxembourg's 12. The tiles under one zoom-9 tile are made on up to four processors.
  `--terrain-zoom` stops at a lower zoom than 12, for a region whose map would otherwise be too large.
- `build-map` reads the GeoPackages' detail before the PBFs, and lets both go before the terrain, so that a large
  region does not hold them all at once.

`pack-map` and `pack-addresses` make `Lastbook-Map-<REGION>.zip` (the map set at the top of the zip, deflated, the
elevation tiles stored) and `Lastbook-Addresses-<REGION>.zip` (`derived/geo/addresses/<REGION>.sqlite` alone), check
each with setup's own `install::check_zip`, and print the entry for `maps` or `address_indexes` in
`manifest-v1.json` ([SETUP-API.md](SETUP-API.md)), each zip at `<base>/<name>`: `--base` is by default this
project's data release on GitHub (`crates/engine/src/dist.rs`), which holds its files side by side.
`build-manifest` writes the data release's `manifest-v1.json` from the model catalogue
(`packaging/data/manifest-base.json`) and a folder of those entries, with the starter pack's `starter-entry.json`;
it fails, writing nothing, unless setup would take the result whole (`manifest::validate` on the product's hosts,
every entry kept and offered, every file in the data release).

The four test regions, built on 17 November 2026 from Geofabrik's extracts of 16 November, one after the other (the
first builds of Malta, Luxembourg and Bremen peaked at 440 to 460 MB of memory):

| | Liechtenstein | Malta | Luxembourg | Bremen |
|---|---|---|---|---|
| Sources: GeoPackage zip, PBF | 6.1, 3.5 MB | 12.9, 9.0 MB | 88.3, 48.0 MB | 32.5, 21.3 MB |
| DEM tiles (Natural Earth, 15 MB, for all) | N46, N47 E009: 82 MB | N35, N36 E014: 9 MB | N49, N50 E005, E006: 149 MB | N52, N53 E008, E009: 125 MB |
| Build | 3 s | 3 s | 41 s | 10 s |
| national.sqlite: features (detail), help points | 32,191 (19,424), 441: 8.1 MB | 67,464 (54,661), 2,432: 14.1 MB | 310,056 (297,289), 5,920: 73.0 MB | 109,083 (96,305), 3,471: 21.7 MB |
| Coast: islands, seas (cut and closed) | none | 36, 0 (0) | none | 8, 3 (5) |
| terrain.sqlite: tiles (left out at the edges) | 86 (0): 8.2 MB | 100 (19): 0.8 MB | 373 (15): 30.2 MB | 251 (5): 13.5 MB |
| buildings.sqlite: outlines, with heights | 17,906, 1,023: 0.7 MB | 38,667, 19,377: 2.1 MB | 221,377, 9,525: 8.0 MB | 197,760, 62,274: 6.9 MB |
| places.sqlite, water.sqlite | 341, 25: 0.2 MB | 2,784, 69: 0.6 MB | 9,044, 281: 1.8 MB | 2,521, 434: 0.7 MB |
| The map set installed, `Lastbook-Map-<R>.zip` | 17.1, 12.1 MB | 17.5, 8.4 MB | 113.1, 66.3 MB | 42.8, 28.5 MB |
| Address index: addresses (rows), installed, zip | 12,499: 1.7, 0.6 MB | 4,553 (6,529): 1.2, 0.3 MB | 167,509 (369,262): 58.9, 14.9 MB | 146,800: 31.6, 7.3 MB |

What the regions showed, besides the addresses ([ADDRESSES.md](ADDRESSES.md), "What the first regions showed"):

- Malta is islands the world outline does not have: without `coast.rs` the page drew them as sea with streets on it.
  Its 36 coast rings close within the extract. Gozo and Comino are mapped only as areas, Għawdex and Kemmuna, and are
  found by either name.
- Bremen's exclave: the state's outline is two pieces, Bremen and Bremerhaven, each a city that is its own district
  (level 6, with no level-8 municipality), which is then its town. The Weser is coast as far as Bremen in
  OpenStreetMap; cut at the state's edges, it is closed across the river.
- The PBF read once for every builder showed in Bremen: its city boundary, kept for the place finder, was drawn as one
  34 km building over the city from zoom 15, until buildings took only buildings.
- Two answers of the place finder, fixed in the engine: `Gozo` listed the shops called "Gozo …" before the island
  Għawdex (another name now ranks as the place's own, through `names`), and `L-1648` brought, after the postcode, every
  lake with a word starting with `l` (a query read as a postcode no longer searches the lakes, and the lake search
  never loosens a query down to lone letters).

## Regions on GitHub's machines

`.github/workflows/regions.yml` builds regions only when asked (Actions, Regions, Run workflow), into the data
release: the release tagged `data-v1`, a draft until the maintainer publishes it. The workflow makes the draft when
there is none, adds nothing once it is published, never publishes it and touches no other release. What to build:

- `missing` (the default): every region the draft does not hold whole, its two zips and its two entries;
- `all`: every region;
- region codes, separated by spaces or commas, built in the order given (`CH AT LI`).

`missing` and `all` take the regions sharing an extract one after another (Italy's macro-regions, France's old
regions), so that the cache serves the next. One region builds at a time, each in a job of its own: Geofabrik, shared
by everyone, turned GitHub's machines away when they asked for too much at once. A job that fails leaves the others
going, and `missing` builds it again in a later run. Each job ends with a two-minute pause. The tools are built once,
in a job of their own. A region's job:

1. `lastbook-tools region-sources --region R --neighbours yes` says what to fetch: the region's Geofabrik extracts
   from `crates/tools/data/region-sources.json`, its neighbours', the Copernicus DEM tiles its zoom-9 terrain tiles
   reach (a tile over open sea does not exist, and the bucket says so at once) and Natural Earth's six zips.
2. This week's copies come out of GitHub's cache. Each Geofabrik extract is an entry of its own, named after its path
   and the ISO week (`geofabrik-europe_italy_nord-ovest-2026-W47`), as are Natural Earth's zips. A workflow's steps
   cannot list a number of entries known only once the job runs, so a script step does it with GitHub's own cache
   library (`@actions/cache`, installed for the job); a cache that cannot be read only means fetching. The repository's
   cache holds 10 GB, the longest unused entries going first.
3. `packaging/regions/build-region.sh R yes` with `FETCH_ONLY=1` fetches the rest, each file checked whole (against
   Geofabrik's MD5, a zip opened). What it fetched goes into the cache before the build, so a build that fails does
   not fetch it again.
4. The script again with `OFFLINE=1` runs `build-map` (with the table's terrain zoom), `pack-map` and
   `pack-addresses`, then `lastbook-tools check-map --region R --packs DIR`, which takes the zips as setup does: the
   entries through setup's manifest check, their sizes and SHA-256, `install::check_zip`, the unpacking with setup's
   member rules. Then it asks the engine what a person would: the region's town (the table's `capital`) found by the
   place search inside its boxes, the addresses above a floor (100, or the table's `min_addresses`), the index's first
   address found as typed, detail in every box (an extract missing for one of Portugal's boxes would leave it empty),
   elevation tiles. A failed check fails the job, and so does a zip of 2 GiB or more, which a release cannot hold.
5. The region's old entries leave the release, its two zips go in, then its two entries: an entry never stands beside
   another build's zip.
6. The job's summary gives how long `build-map` took and the most memory it used, the zips' sizes and every check. Its
   artifact, `region-R`, holds the entries, the check, the build's report and those figures.

At the end `.github/workflows/data-manifest.yml`, which also runs by hand, writes the release's `manifest-v1.json`:
`lastbook-tools build-manifest` from the model catalogue, every entry in the release whose zips are there whole, at
their size, and the starter pack's `starter-entry.json` when the library's workflow has put it there. Only a manifest
setup would take whole goes into the release.

`region-sources.json` maps each of the 110 regions to its Geofabrik extracts, as paths under the site. Several regions
share one, which their boxes clip: Italy's extracts are its five macro-regions (Lombardy is `italy/nord-ovest` with
Piedmont, Aosta and Liguria), France's are its old regions, two or three to a new one; Spain's Canaries come from
`africa/canary-islands`, and Portugal's Azores from their own extract. Geofabrik makes no free GeoPackage of Czechia,
the Netherlands, Norway, Baden-Württemberg, Bayern or Nordrhein-Westfalen whole (the "latest" link of each leads to a
missing file or to the site's home page, which is what Bayern's "cut" download was), so those are built from their
parts, which have one: Czechia's 14 regions, the 12 provinces, Norway's 5 parts (Svalbard left out), and the 4, 7 and
5 Regierungsbezirke. The file also holds each extract's bounds, from Geofabrik's index. A test checks it against
`regions.rs`: every region once, an extract shared only within one country, the bounds of every extract.

The neighbours, and the rule for fetching them:

- A region's neighbours are named in the file where boxes alone would mislead (Liechtenstein: Switzerland and Austria,
  not Bayern, whose box reaches its corner; Malta: none, though Sicily's box reaches Lampedusa; Luxembourg; Bremen:
  Lower Saxony). Norway, Sweden and Finland have none: each one's single box holds much of the others (Norway's reaches
  31°E, across Lapland), so their "strip" would be half a country.
- For the others they are the extracts of the regions whose boxes, without their margin, meet the region's, and of
  those only the extracts whose bounds reach its boxes: South Moravia and Oberbayern for Austria, not Liberec or
  Unterfranken.
- A neighbour's extract is fetched only when its GeoPackage and PBF together are under 300 MB (Geofabrik is asked
  their size first), or when this week's cache already holds it, from a region built before. A larger one is left
  out with a note, and the strip then has the base map and the elevation only. So a small region gets its large
  neighbour's strip when it is built after that neighbour in the same week: `CH AT LI` gives Liechtenstein the Swiss
  bank of the Rhine.

Large regions on the runner. A runner has 16 GB of memory and, by GitHub's account, 14 GB of disk. The job removes
toolchains it does not use, puts the sources and the build on whichever disk then has more room (the system disk or
`/mnt`, always under the name `/mnt/lastbook`, where the cache puts them back), adds 16 GB of swap there when 40 GB
are free, so that a region needing more memory than the runner has slows down rather than stops, and prints both
disks at its start. From Geofabrik's sizes of 17 November 2026, and the test regions' memory of about five times
their PBF:

- Norway: PBFs 1.4 GB, GeoPackages 3.4 GB (some 7.5 GB unpacked), 420 DEM tiles listed (fewer exist, 15 to 30 MB
  each so far north): some 20 GB of disk, which a runner whose disks are smaller cannot give. Some 7 GB of memory
  for the PBFs and GeoPackages, then at most 2 GB of DEM files for the terrain, one after the other.
- The Netherlands (PBFs 1.4 GB, nearly every building and address mapped), Sweden (1.0 GB), Czechia and
  Nordrhein-Westfalen (0.9 GB) and Bayern: 4 to 7 GB of memory.
- Elevation to zoom 12 over Norway's, Sweden's and Finland's boxes would make maps of several GB, past the 2 GiB a
  release's file may be: those three stop at zoom 10 (`terrain_zoom` in the table).
- These are estimates: the first run's summaries give each region's measured memory and time.

Tried here, offline, on what was already downloaded: Liechtenstein through the script, its sources laid out as the
cache keeps them. Its national, places, water, buildings and address files come out byte for byte as before, its 86
elevation tiles too (the terrain file differs only in SQLite's change counter, in its header), all eight checks pass
with the release's addresses, and `build-manifest` makes a manifest of its entries that setup takes. Not tried here:
the workflows themselves, and anything that needs a new download.

## Checking a map change

- `cargo test -p lastbook-tools`: each builder against small sources made in the test, and a whole region from them to
  what setup installs and what the engine answers. With a region's zips unpacked into
  `derived/app-check/<REGION>/LastbookData`, `real.rs` also asks it what people there type and checks the first answer
  (31 queries for Malta, Luxembourg and Bremen), and that Luxembourg's postcodes bring no lakes.
- `lastbook-tools check-map` on a region's zips: what the workflow checks after each build.
- A change people see is checked in the running app: `lastbook --data <a LastbookData with the map> --snapshot
  FILE.png --eval ...`, and the picture looked at.
- Nothing a contributor adds may make the app open a connection. New data comes in through a build tool and ships as a
  file.
