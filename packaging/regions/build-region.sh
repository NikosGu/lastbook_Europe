#!/usr/bin/env bash
# One region from its public sources to the zips setup downloads, with their manifest entries, checked as setup takes
# them and as a person uses them (.github/workflows/regions.yml runs it on GitHub's machines; docs/MAPS.md):
#
#   packaging/regions/build-region.sh REGION [yes]
#
# With yes, the neighbours' extracts too, read only inside the region's boxes. What to fetch comes from
# `lastbook-tools region-sources` (crates/tools/data/region-sources.json). A file already in the sources folder is not
# fetched again; with OFFLINE=1 nothing is fetched at all: a missing neighbour or DEM tile is left out, a missing
# extract of the region's own stops the build.
#
#   TOOLS    the lastbook-tools program              (default: target/release/lastbook-tools)
#   SOURCES  where downloads are kept                (default: derived/sources/download)
#   OUT      where each region's build and packs go  (default: derived/regions)
set -euo pipefail

region=${1:?usage: build-region.sh REGION [yes]}
neighbours=${2:-no}
tools=${TOOLS:-target/release/lastbook-tools}
sources=${SOURCES:-derived/sources/download}
out=${OUT:-derived/regions}/$region
plan=$("$tools" region-sources --region "$region" --neighbours "$neighbours")

# fetch URL FILE: kept if it is there already; under OFFLINE=1 only what is there
fetch() {
  local url=$1 file=$2
  [ -s "$file" ] && return 0
  if [ "${OFFLINE:-0}" = 1 ]; then
    echo "$region: not here, and offline: $file" >&2
    return 1
  fi
  mkdir -p "$(dirname "$file")"
  if curl -fsSL --retry 3 --retry-delay 10 -o "$file.part" "$url"; then
    mv "$file.part" "$file"
  else
    rm -f "$file.part"
    return 1
  fi
}

args=(build-map --region "$region" --out "$out/build")
for group in extracts neighbours; do
  while IFS=$'\t' read -r path gpkg_url pbf_url; do
    [ -n "$path" ] || continue
    dir="$sources/geofabrik/$(dirname "$path")"
    name=$(basename "$path")
    gpkg="$dir/$name-latest-free.gpkg.zip"
    pbf="$dir/$name-latest.osm.pbf"
    if fetch "$gpkg_url" "$gpkg" && fetch "$pbf_url" "$pbf"; then
      if [ "$group" = extracts ]; then
        args+=(--gpkg "$gpkg" --pbf "$pbf")
      else
        args+=(--neighbour-gpkg "$gpkg" --neighbour-pbf "$pbf")
      fi
    elif [ "$group" = extracts ]; then
      echo "$region: its extract $path could not be fetched" >&2
      exit 1
    else
      echo "$region: the neighbour $path is left out" >&2
    fi
  done < <(jq -r ".${group}[] | [.path, .gpkg, .pbf] | @tsv" <<<"$plan")
done

dem=0
while IFS=$'\t' read -r name url; do
  # a tile over open sea does not exist
  if fetch "$url" "$sources/dem/$name.tif" 2>/dev/null; then
    args+=(--dem "$sources/dem/$name.tif")
    dem=$((dem + 1))
  fi
done < <(jq -r '.dem[] | [.name, .url] | @tsv' <<<"$plan")
echo "$region: $dem of $(jq '.dem | length' <<<"$plan") DEM tiles (the others are open sea, or not fetched)" >&2

ne=yes
while IFS=$'\t' read -r name url; do
  fetch "$url" "$sources/ne/$name.zip" || ne=no
done < <(jq -r '.natural_earth[] | [.name, .url] | @tsv' <<<"$plan")
if [ "$ne" = yes ]; then
  args+=(--ne "$sources/ne")
else
  echo "$region: without Natural Earth, no zoomed-out base" >&2
fi

rm -rf "$out"
mkdir -p "$out/packs"
"$tools" "${args[@]}" > "$out/build-report.json"
"$tools" pack-map --map "$out/build/map" --out "$out/packs" > "$out/packs/map-entry-$region.json"
"$tools" pack-addresses --index "$out/build/addresses/$region.sqlite" --out "$out/packs" > "$out/packs/addresses-entry-$region.json"
"$tools" check-map --region "$region" --packs "$out/packs" --work "$out/check" > "$out/packs/check-$region.json"
rm -rf "$out/build"
echo "$region: built, packed and checked: $out/packs" >&2
