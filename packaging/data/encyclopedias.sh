#!/bin/bash
# The packs of a language's encyclopedias (content/library/encyclopedias.json) in the data release: each is the starter
# pack's zip with that language's encyclopedias, Kiwix's ZIM files, each in a zip of its own. Only the data release is
# touched; nothing is published here.
#
#   RELEASE=data-v1 GH_REPO=owner/repo packaging/data/encyclopedias.sh build [PACK...]
#       Each encyclopedia of the packs named (none: every pack), downloaded from Kiwix and checked against the SHA-256
#       Kiwix publishes beside it, put in a zip of its own (LastbookData/library-data/collections/<file>, stored), and
#       uploaded with a note of what it holds (encyclopedia-<file>.json). One already in the release is left as it is.
#   RELEASE=data-v1 GH_REPO=owner/repo packaging/data/encyclopedias.sh entries
#       Each pack's manifest entry (pack-entry-<id>.json): the starter pack's zip as starter-entry.json names it, and its
#       encyclopedias' zips, uploaded to the release. Made again whenever the starter pack changes (the library
#       workflow runs it); a pack whose encyclopedias are not all in the release yet is passed over.
#
# Needs gh (with write access to the repository), jq, curl, zip and sha256sum.
set -euo pipefail
set -f   # pack ids are split into words, never read as file names
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
LIST="$ROOT/content/library/encyclopedias.json"
: "${RELEASE:?the tag of the data release}" "${GH_REPO:?the repository, as owner/name}"
BASE="https://github.com/$GH_REPO/releases/download/$RELEASE"
KIWIX="https://download.kiwix.org/zim"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
sha() { sha256sum "$1" | cut -d' ' -f1; }
have="$(gh release view "$RELEASE" --json assets --jq '.assets[] | select(.state == null or .state == "uploaded") | .name')"
there() { grep -qxF "$1" <<< "$have"; }

build() {
  local packs=("$@") pack z file dir content stem zipname note into zbytes want size
  [ ${#packs[@]} -gt 0 ] || mapfile -t packs < <(jq -r '.packs[].id' "$LIST")
  for pack in "${packs[@]}"; do
    jq -e --arg p "$pack" '.packs[] | select(.id == $p)' "$LIST" > /dev/null || { echo "encyclopedias: no pack $pack in the list" >&2; exit 1; }
    while read -r z; do
      file=$(jq -r .file <<< "$z"); dir=$(jq -r .dir <<< "$z"); content=$(jq -r .content <<< "$z")
      stem="${file%.zim}"; zipname="Lastbook-Encyclopedia-$stem.zip"; note="encyclopedia-$stem.json"
      if there "$zipname" && there "$note"; then
        echo "$file: already in $RELEASE"
        continue
      fi
      into="$work/z/LastbookData/library-data/collections"
      mkdir -p "$into"
      echo "$file: downloading from Kiwix"
      curl -fsSL --retry 5 --retry-delay 20 -o "$into/$file" "$KIWIX/$dir/$file"
      want=$(curl -fsSL --retry 5 --retry-delay 20 "$KIWIX/$dir/$file.sha256" | cut -d' ' -f1)
      [ "$(sha "$into/$file")" = "$want" ] || { echo "encyclopedias: $file is not the file Kiwix lists (SHA-256 $want)" >&2; exit 1; }
      zbytes=$(stat -c %s "$into/$file")
      (cd "$work/z" && zip -0 -q -r "$work/$zipname" LastbookData)
      rm -rf "$work/z"
      size=$(stat -c %s "$work/$zipname")
      jq -n --arg name "$zipname" --argjson size "$size" --arg sha256 "$(sha "$work/$zipname")" --arg zim "$file" --argjson zim_bytes "$zbytes" \
            --arg zim_sha256 "$want" --arg content "$content" --arg from "$KIWIX/$dir/$file" \
            '{name: $name, size: $size, sha256: $sha256, zim: $zim, zim_bytes: $zim_bytes, zim_sha256: $zim_sha256, content: $content, from: $from}' > "$work/$note"
      gh release upload "$RELEASE" --clobber "$work/$zipname" "$work/$note"
      rm -f "$work/$zipname" "$work/$note"
      echo "$file: in $RELEASE as $zipname ($size bytes)"
    done < <(jq -c --arg p "$pack" '.packs[] | select(.id == $p) | .zims[]' "$LIST")
  done
}

entries() {
  local pack lang file note notes ok
  there starter-entry.json || { echo "encyclopedias: no starter-entry.json in $RELEASE: no pack entry made"; return 0; }
  gh release download "$RELEASE" --pattern starter-entry.json --dir "$work" --clobber
  for pack in $(jq -r '.packs[].id' "$LIST"); do
    lang=$(jq -r --arg p "$pack" '.packs[] | select(.id == $p) | .lang' "$LIST")
    notes=(); ok=1
    for file in $(jq -r --arg p "$pack" '.packs[] | select(.id == $p) | .zims[].file' "$LIST"); do
      note="encyclopedia-${file%.zim}.json"
      if there "$note"; then
        gh release download "$RELEASE" --pattern "$note" --dir "$work" --clobber
        notes+=("$work/$note")
      else
        ok=0
      fi
    done
    if [ "$ok" != 1 ]; then
      echo "$pack: not every encyclopedia is in $RELEASE yet: no entry"
      continue
    fi
    # the starter's documents, then the encyclopedias: setup unpacks the zips in this order into one library
    jq -s --arg id "$pack" --arg lang "$lang" --arg base "$BASE" '
      {"it": "Italiano", "de": "Deutsch", "fr": "Français", "es": "Español", "pl": "Polski", "en": "English"} as $native
      | {"it": "Italian", "de": "German", "fr": "French", "es": "Spanish", "pl": "Polish", "en": "English"} as $english
      | {"wikipedia_medicine": "Wikipedia'"'"'s medical articles", "wikipedia_top": "Wikipedia'"'"'s most read articles (without pictures)",
         "wikivoyage": "Wikivoyage'"'"'s travel guides", "wikipedia_mini": "the opening of Wikipedia'"'"'s most read articles"} as $words
      | .[0] as $s | .[1:] as $z
      | [$z[].content | $words[.] // .] as $w
      | (if ($w | length) > 1 then ($w[:-1] | join(", ")) + " and " + $w[-1] else $w[0] end) as $list
      | {id: $id, title: ("Starter library and encyclopedias · " + $native[$lang]),
         summary: ("Every document of the starter library, and " + $list + " in " + $english[$lang] + ", from the Kiwix collection, to read offline."),
         revision: $s.revision, map: "base", records: $s.records,
         installed_bytes: ($s.installed_bytes + ([$z[].zim_bytes] | add)),
         download_bytes: ($s.download_bytes + ([$z[].size] | add)),
         available: true, contents: [$z[].content],
         files: ($s.files + [$z[] | {name, url: ($base + "/" + .name), size, sha256, kind: "zip"}])}' \
      "$work/starter-entry.json" "${notes[@]}" > "$work/pack-entry-$pack.json"
    gh release upload "$RELEASE" --clobber "$work/pack-entry-$pack.json"
    echo "$pack: $(jq -r '"\(.files | length) files, \(.download_bytes / 1000000 | floor) MB to download"' "$work/pack-entry-$pack.json")"
  done
}

case "${1:-}" in
  build) shift; build "$@" ;;
  entries) entries ;;
  *) sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'; exit 2 ;;
esac
