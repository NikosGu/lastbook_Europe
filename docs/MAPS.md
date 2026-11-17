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

- `places.sqlite`: `place(id, name, kind, detail, lat, lon, rank, source, country, key, area)` and
  `place_fts USING fts5(key, area, content='place', content_rowid='id', tokenize='unicode61 remove_diacritics 0')`.
  `key` holds every name the place has (its name, and from the PBF its names in other languages, alternative, old,
  official and short names); `area` the words that say where it is (its municipality or district, and its country in
  its own languages and English). A postcode's names are the postcode as written and its key (`1012 AB`, `1012AB`).
  `rank`: 1 a city or town, 2 a village, municipality or postcode, 3 a smaller place or a peak, 4 a named point.
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

The detail is what the two extracts hold, and Geofabrik's country extracts stop at the border. A region's boxes reach
into its neighbours, so that strip has only the base map and the elevation: Liechtenstein's map has no streets on the
Swiss bank of the Rhine and a handful near Feldkirch, and its address index only the few Austrian addresses the extract
has. Filling the strip needs the neighbours' extracts as well; `build-map` takes one of each for now.

How each file is made (`crates/tools/src/map/`):

- `detail.rs`: the original Lastbook's layers, classes and zooms from the GeoPackage; an area appears from the zoom
  where it spans about a sixteenth of a tile. `base.rs` adds the Natural Earth base with each feature's own `min_zoom`.
- `places.rs`, `water.rs`, `buildings.rs`, `addresses.rs`, `terrain.rs`: the other files as described above. A
  terrain pixel is the mean of DEM samples spread across it: one at zoom 12, 8 × 8 at zoom 9.

`pack-map` and `pack-addresses` make `Lastbook-Map-<REGION>.zip` (the map set at the top of the zip, deflated, the
elevation tiles stored) and `Lastbook-Addresses-<REGION>.zip` (`derived/geo/addresses/<REGION>.sqlite` alone), check
each with setup's own `install::check_zip`, and print the entry for `maps` or `address_indexes` in
`manifest-v1.json` ([SETUP-API.md](SETUP-API.md)), with the download address in the `maps/` or `addresses/` folder of
`--base` (by default the placeholder host of `crates/engine/src/dist.rs`).

Liechtenstein, built on 17 November 2026 from Geofabrik's extract of 16 November (about 3 seconds):

| | Size |
|---|---|
| Sources: GeoPackage zip, PBF, two DEM tiles, Natural Earth | 6.1 MB, 3.5 MB, 82 MB, 15 MB |
| national.sqlite: 32,191 features (12,767 base, 19,424 detail), 441 help points | 8.1 MB |
| terrain.sqlite: 86 tiles | 8.2 MB |
| buildings.sqlite: 17,946 outlines, 1,026 with heights | 0.7 MB |
| places.sqlite (341 places), water.sqlite (25 records) | 0.2 MB |
| The map set installed, and `Lastbook-Map-LI.zip` | 17.1 MB, 12.1 MB |
| The address index (12,499 addresses), and `Lastbook-Addresses-LI.zip` | 1.6 MB, 0.6 MB |

## Checking a map change

- `cargo test -p lastbook-tools`: each builder against small sources made in the test, and a whole region from them to
  what setup installs and what the engine answers.
- A change people see is checked in the running app: `lastbook --data <a LastbookData with the map> --snapshot
  FILE.png --eval ...`, and the picture looked at.
- Nothing a contributor adds may make the app open a connection. New data comes in through a build tool and ships as a
  file.
