#!/usr/bin/env bash
# One region from its public sources to the zips setup downloads, with their manifest entries, checked as setup takes
# them and as a person uses them (.github/workflows/regions.yml runs it on GitHub's machines; docs/MAPS.md):
#
#   packaging/regions/build-region.sh REGION [yes]
#
# With yes, the neighbours' extracts too, read only inside the region's boxes. What to fetch comes from
# `lastbook-tools region-sources` (crates/tools/data/region-sources.json). A file already in the sources folder is not
# fetched again; with OFFLINE=1 nothing is fetched at all: a missing neighbour or DEM tile is left out, a missing
# extract of the region's own stops the build. With FETCH_ONLY=1 the script stops once the sources are fetched.
#
# Each extract has a folder of its own, <SOURCES>/geofabrik/<path with _ for />, holding its GeoPackage and PBF, both
# whole or neither: the workflow keeps each folder in GitHub's cache. A neighbour's extract is taken when it is already
# here (this week's cache) or, to spare Geofabrik, when its two files are smaller than NEIGHBOUR_MAX bytes together.
#
#   TOOLS          the lastbook-tools program              (default: target/release/lastbook-tools)
#   SOURCES        where downloads are kept                (default: derived/sources/download)
#   OUT            where each region's build and packs go  (default: derived/regions)
#   NEIGHBOUR_MAX  a neighbour fetched only below this     (default: 300000000, 300 MB)
set -euo pipefail

region=${1:?usage: build-region.sh REGION [yes]}
neighbours=${2:-no}
tools=${TOOLS:-target/release/lastbook-tools}
sources=${SOURCES:-derived/sources/download}
out=${OUT:-derived/regions}/$region
neighbour_max=${NEIGHBOUR_MAX:-300000000}
agent="lastbook-europe region builder"
# a release's file must be smaller than 2 GiB
release_max=2147483648
plan=$("$tools" region-sources --region "$region" --neighbours "$neighbours")

# geofabrik_or_stop URL: Geofabrik, shared by everyone, turns some of GitHub's machines away for hours while others get
# through. It is asked once, before the first file this run would take from it; a machine it refuses stops the region at
# once (exit 75), and the workflow's next pass builds it on another machine, rather than trying each file for 20 minutes
# or leaving a neighbour out.
geofabrik=unknown
geofabrik_or_stop() {
  case "$1" in https://download.geofabrik.de/*) ;; *) return 0 ;; esac
  [ "${OFFLINE:-0}" = 1 ] || [ "$geofabrik" = ok ] && return 0
  if curl -fsS -o /dev/null -I -A "$agent" --connect-timeout 20 --retry 5 --retry-connrefused --retry-delay 30 --retry-max-time 300 \
       https://download.geofabrik.de/ 2>/dev/null; then
    geofabrik=ok
    return 0
  fi
  echo "$region: Geofabrik turns this machine away; the region is left for a pass on another machine" >&2
  exit 75
}

# fetch URL FILE: kept if it is there already; under OFFLINE=1 only what is there
fetch() {
  local url=$1 file=$2
  [ -s "$file" ] && return 0
  if [ "${OFFLINE:-0}" = 1 ]; then
    echo "$region: not here, and offline: $file" >&2
    return 1
  fi
  geofabrik_or_stop "$url"
  mkdir -p "$(dirname "$file")"
  local try
  for try in 1 2 3; do
    # a refused or broken connection is tried again too, waiting longer each time (Geofabrik turns away bursts)
    if curl -fsSL -A "$agent" --retry 8 --retry-connrefused --retry-all-errors --retry-max-time 1500 \
         -o "$file.part" "$url" && whole "$url" "$file.part"; then
      mv "$file.part" "$file"
      return 0
    fi
    rm -f "$file.part"
    echo "$region: $url came incomplete or not at all (try $try of 3)" >&2
    sleep $((try * 60))
  done
  return 1
}

# whole URL FILE: the file is what the site meant to send. Against the MD5 sum Geofabrik publishes beside a file, where
# there is one; a zip must also open whole (a cut download once passed as a GeoPackage).
whole() {
  local url=$1 file=$2 want got
  if want=$(curl -fsSL -A "$agent" --retry 3 "$url.md5" 2>/dev/null | awk '{print $1}') && [ -n "$want" ]; then
    got=$( (md5sum "$file" 2>/dev/null || md5 -r "$file") | awk '{print $1}')
    [ "$got" = "$want" ] || { echo "$region: $url: MD5 $got, not $want" >&2; return 1; }
  fi
  case "$file" in
    *.zip|*.zip.part) unzip -tqq "$file" >/dev/null 2>&1 || { echo "$region: $url: not a whole zip" >&2; return 1; } ;;
  esac
  return 0
}

# size URL: the bytes the site would send (0 when it says nothing, or sends a page instead of the file)
size() {
  local code len type
  read -r code len type < <(curl -sIL -A "$agent" --retry 3 --retry-connrefused -o /dev/null \
    -w '%{http_code} %header{content-length} %{content_type}\n' "$1" 2>/dev/null || echo "0 0 none")
  case "$code:$len:$type" in
    200:*:text/html*) echo 0 ;;
    200:[0-9]*) echo "$len" ;;
    *) echo 0 ;;
  esac
}

args=(build-map --region "$region" --out "$out/build" --terrain-zoom "$(jq -r '.terrain_zoom // 12' <<<"$plan")")
for group in extracts neighbours; do
  while IFS=$'\t' read -r path gpkg_url pbf_url; do
    [ -n "$path" ] || continue
    dir="$sources/geofabrik/${path//\//_}"
    name=${path##*/}
    gpkg="$dir/$name-latest-free.gpkg.zip"
    pbf="$dir/$name-latest.osm.pbf"
    if [ "$group" = neighbours ] && [ "${OFFLINE:-0}" != 1 ] && ! { [ -s "$gpkg" ] && [ -s "$pbf" ]; }; then
      # not in this week's cache: a large neighbour costs Geofabrik more than its strip is worth
      geofabrik_or_stop "$gpkg_url"   # here, not in size's subshell: a machine turned away stops the region, not the neighbour
      bytes=$(( $(size "$gpkg_url") + $(size "$pbf_url") ))
      sleep 1
      if [ "$bytes" -le 0 ] || [ "$bytes" -ge "$neighbour_max" ]; then
        echo "$region: the neighbour $path is left out: $((bytes / 1000000)) MB, not in this week's cache (0: its size unknown)" >&2
        continue
      fi
    fi
    if fetch "$gpkg_url" "$gpkg" && fetch "$pbf_url" "$pbf"; then
      if [ "$group" = extracts ]; then
        args+=(--gpkg "$gpkg" --pbf "$pbf")
      else
        args+=(--neighbour-gpkg "$gpkg" --neighbour-pbf "$pbf")
      fi
      continue
    fi
    # half an extract is never kept: the cache takes a folder whole or not at all
    [ "${OFFLINE:-0}" = 1 ] || rm -rf "$dir"
    if [ "$group" = extracts ]; then
      echo "$region: its extract $path could not be fetched" >&2
      exit 1
    fi
    echo "$region: the neighbour $path is left out" >&2
  done < <(jq -r ".${group}[] | [.path, .gpkg, .pbf] | @tsv" <<<"$plan")
done

# fetch_dem URL FILE: a DEM tile, or none: one over open sea does not exist (the bucket answers 403 or 404 at once, and
# is not asked again; asking as fetch does would wait some 20 minutes for each)
fetch_dem() {
  local url=$1 file=$2 code try
  [ -s "$file" ] && return 0
  [ "${OFFLINE:-0}" = 1 ] && return 1
  mkdir -p "$(dirname "$file")"
  for try in 1 2 3; do
    code=$(curl -sSL -A "$agent" --retry 5 --retry-connrefused -o "$file.part" -w '%{http_code}' "$url" 2>/dev/null) || code=broken
    case "$code" in
      200) mv "$file.part" "$file"; return 0 ;;
      403|404) rm -f "$file.part"; return 1 ;;
    esac
    rm -f "$file.part"
    sleep $((try * 10))
  done
  echo "$region: $url came incomplete or not at all" >&2
  return 1
}

dem=0
while IFS=$'\t' read -r name url; do
  if fetch_dem "$url" "$sources/dem/$name.tif"; then
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
if [ "${FETCH_ONLY:-0}" = 1 ]; then
  echo "$region: fetched" >&2
  exit 0
fi

# measured COMMAND...: run it, and say how much memory and time it took where the system tells (GNU time)
measured() {
  if /usr/bin/time -v true >/dev/null 2>&1; then
    local status=0
    /usr/bin/time -v -o "$out/time.txt" "$@" || status=$?
    awk -F': ' -v r="$region" '/Maximum resident set size/ {m = $2} /Elapsed \(wall clock\)/ {t = $2}
      END {printf "%s: build-map took %s, at most %.1f GB of memory\n", r, t, m / 1048576}' "$out/time.txt" | tee -a "$out/time.txt" >&2
    return "$status"
  fi
  "$@"
}

rm -rf "$out"
mkdir -p "$out/packs"
measured "$tools" "${args[@]}" > "$out/build-report.json"
"$tools" pack-map --map "$out/build/map" --out "$out/packs" > "$out/packs/map-entry-$region.json"
"$tools" pack-addresses --index "$out/build/addresses/$region.sqlite" --out "$out/packs" > "$out/packs/addresses-entry-$region.json"
rm -rf "$out/build"
for zip in "$out/packs/Lastbook-Map-$region.zip" "$out/packs/Lastbook-Addresses-$region.zip"; do
  bytes=$(wc -c < "$zip" | tr -d ' ')
  if [ "$bytes" -ge "$release_max" ]; then
    echo "$region: $zip is $bytes bytes, more than a release's file may be (2 GiB)" >&2
    exit 1
  fi
done
"$tools" check-map --region "$region" --packs "$out/packs" --work "$out/check" > "$out/packs/check-$region.json"
echo "$region: built, packed and checked: $out/packs" >&2
