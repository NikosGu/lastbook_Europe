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
  edges at zoom 9 and 10).

`pack-map` and `pack-addresses` make `Lastbook-Map-<REGION>.zip` (the map set at the top of the zip, deflated, the
elevation tiles stored) and `Lastbook-Addresses-<REGION>.zip` (`derived/geo/addresses/<REGION>.sqlite` alone), check
each with setup's own `install::check_zip`, and print the entry for `maps` or `address_indexes` in
`manifest-v1.json` ([SETUP-API.md](SETUP-API.md)), with the download address in the `maps/` or `addresses/` folder of
`--base` (by default the placeholder host of `crates/engine/src/dist.rs`).

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

`.github/workflows/regions.yml` builds regions only when asked (Actions, Regions, Run workflow): the region codes,
separated by spaces or commas, and whether to take the neighbours' extracts. Each region is a job of its own, at most
four at once, which runs `packaging/regions/build-region.sh REGION [yes]`:

1. `lastbook-tools region-sources --region R [--neighbours yes]` says what to fetch: the region's Geofabrik extracts
   from `crates/tools/data/region-sources.json`, its neighbours', the Copernicus DEM tiles its zoom-9 terrain tiles
   reach (a tile over open sea does not exist and is left out) and Natural Earth's six zips.
2. The script fetches them (a file already there is kept; `OFFLINE=1` fetches nothing), then runs `build-map`,
   `pack-map` and `pack-addresses`.
3. `lastbook-tools check-map --region R --packs DIR` takes the zips as setup does: the entries through setup's manifest
   check, their sizes and SHA-256, `install::check_zip`, the unpacking with setup's member rules. Then it asks the engine
   what a person would: the region's town (the table's `capital`) found by the place search inside its boxes, the
   addresses above a floor (100, or the table's `min_addresses`), the index's first address found as typed, detail in
   every box (an extract missing for one of Portugal's boxes would leave it empty), elevation tiles. A failed check
   fails the job.
4. The zips, their entries (`map-entry-R.json`, `addresses-entry-R.json`), the check (`check-R.json`) and the build's
   report come back as the job's artifact, `region-R`.

`region-sources.json` maps each of the 110 regions to its Geofabrik extracts, as paths under the site. Several regions
share one, which their boxes clip: Italy's extracts are its five macro-regions (Lombardy is `italy/nord-ovest` with
Piedmont, Aosta and Liguria), France's are its old regions, two or three to a new one; Spain's Canaries come from
`africa/canary-islands`, and Portugal's Azores from their own extract. A test checks the file against `regions.rs`:
every region once, an extract shared only within one country. A region's neighbours are named in the file where boxes
alone would mislead (Liechtenstein: Switzerland and Austria, not Bayern, whose box reaches its corner; Malta: none,
though Sicily's box reaches Lampedusa; Luxembourg; Bremen: Lower Saxony). For the others they are the regions whose
boxes, without their margin, meet the region's: every extract of such a region, so a French neighbour may bring an old
region that does not touch the strip, a download for nothing.

Tried here, offline, on what was already downloaded: Malta, and Liechtenstein with its neighbours asked for and not
there (left out, with a note); both built, packed and passed every check. Not tried here: anything that needs a new
download. What to expect on the larger regions, from `region-sources`: the DEM tiles listed (fewer exist, open sea has
none) are 2 for Liechtenstein, 12 for Luxembourg, 28 for Bayern, 36 for Nouvelle-Aquitaine, 180 for Finland, 240 for
Sweden and 420 for Norway. A tile is 30 to 45 MB to download, and the terrain builder holds every tile a region needs in
memory: 52 MB south of 50°N, narrower further north (35, 26, then 17 MB above 70°N). Norway's would take some 7 GB of
a runner's 16, to be measured on the first run.

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
