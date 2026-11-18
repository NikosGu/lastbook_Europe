#!/bin/bash
# The starter library's releases on GitHub: the copies of its sources, and the published pack, which holds every
# document of content/library/starter.json (the maintainer's decision of 18 Nov 2026, whatever each publisher's terms
# say; "redistribute" records them). Nothing here publishes a release: each is made, when missing, as a draft, and only
# the release named is touched. Needs gh (signed in, with write access to the repository), jq, curl and unzip.
#
#   packaging/data/library-release.sh sources [DOCS]
#       Each document of the list, exactly as its publisher published it (downloaded into DOCS, default
#       derived/sources/library, if it is not there; checked against the list's size and SHA-256), uploaded to the
#       draft release sources-v1 with a note naming each document's publisher, licence and terms. The library workflow
#       takes a document from there when its publisher turns GitHub's machines away. (The Library sources workflow does
#       the same on GitHub's machines for documents not yet measured.)
#   packaging/data/library-release.sh check ZIP ENTRY
#       The checks the published pack must pass: the entry names the zip by its size and SHA-256 and the list's
#       revision, and the zip's catalog lists every document of the list and the Sources page.
#   packaging/data/library-release.sh starter ZIP ENTRY
#       The checks, then the zip and its entry uploaded to the draft release data-v1 (crates/engine/src/dist.rs DATA_TAG),
#       replacing the files of the same names.
#
# REPO (default: crates/engine/src/dist.rs REPO) names the repository; TARGET, the commit a new draft's tag will point
# at once it is published (default: the repository's default branch). DRY_RUN=1 checks everything and says what it
# would make and upload, without asking GitHub anything.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
LIST="${LIST:-$ROOT/content/library/starter.json}"   # the library workflow gives the list it built from
REPO="${REPO:-$(sed -n 's/^pub const REPO: &str = "\(.*\)";/\1/p' "$ROOT/crates/engine/src/dist.rs")}"
DATA_TAG="$(sed -n 's/^pub const DATA_TAG: &str = "\(.*\)";/\1/p' "$ROOT/crates/engine/src/dist.rs")"
SOURCES_TAG=sources-v1
UA="lastbook-tools/0.1 (offline library builder)"
sha256() { if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1; }
size() { wc -c < "$1" | tr -d ' '; }
die() { echo "library-release: $*" >&2; exit 1; }
# gh, or with DRY_RUN what it would be asked
gh_write() { if [ -n "${DRY_RUN:-}" ]; then echo "would run: gh $*"; else gh "$@"; fi; }

# a release of this repository with this tag, draft or published (drafts are not found by their tag, so the list of
# releases is read; a failure to read it stops here)
has_release() {
  local tags
  if [ -n "${DRY_RUN:-}" ]; then echo "would look for the release $1 in $REPO" >&2; return 1; fi
  tags="$(gh api "repos/$REPO/releases?per_page=100" --paginate --jq '.[].tag_name')" || die "the releases of $REPO could not be read"
  grep -qxF "$1" <<< "$tags"
}
draft_if_missing() { # TAG TITLE NOTES_FILE
  if [ -n "${DRY_RUN:-}" ]; then echo "--- the note of $1:"; cat "$3"; echo "---"; fi
  if has_release "$1"; then
    gh_write release edit "$1" --repo "$REPO" --notes-file "$3"
  else
    gh_write release create "$1" --repo "$REPO" --draft ${TARGET:+--target "$TARGET"} --title "$2" --notes-file "$3"
    echo "${DRY_RUN:+dry run: }$1: made, as a draft"
  fi
}

check() { # ZIP ENTRY
  local zip="$1" entry="$2" tmp
  [ -f "$zip" ] && [ -f "$entry" ] || die "check: $zip or $entry is missing"
  tmp="$(mktemp -d)"
  [ "$(jq -r '.files[0].sha256' "$entry")" = "$(sha256 "$zip")" ] || die "$entry does not name $zip's SHA-256"
  [ "$(jq -r '.files[0].size' "$entry")" = "$(size "$zip")" ] || die "$entry does not name $zip's size"
  [ "$(jq -r '.revision' "$entry")" = "$(jq -r '.pack.revision' "$LIST")" ] || die "$entry is not the pack of the list's revision"
  unzip -p "$zip" LastbookData/library-data/catalog.json | jq -r '.[].id' | sort > "$tmp/listed"
  { jq -r '.documents[].id' "$LIST"; echo lastbook-sources; } | sort > "$tmp/all"
  diff "$tmp/all" "$tmp/listed" >&2 || die "$zip's catalog is not every document of the list and the Sources page"
  [ "$(jq -r '.records' "$entry")" = "$(wc -l < "$tmp/listed" | tr -d ' ')" ] || die "$entry's record count is not the catalog's"
  echo "$zip: $(wc -l < "$tmp/listed" | tr -d ' ') records, every document of the list and the Sources page; its entry matches"
  rm -rf "$tmp"
}

sources() { # [DOCS]
  local docs="${1:-$ROOT/derived/sources/library}" notes files=()
  mkdir -p "$docs"
  while IFS=$'\t' read -r id file url bytes sum; do
    if [ ! -f "$docs/$file" ]; then
      echo "$id: downloading from $url"
      curl -fsSL --retry 3 -A "$UA" -o "$docs/$file.part" "$url" || die "$id: $url could not be downloaded here either"
      mv "$docs/$file.part" "$docs/$file"
    fi
    [ "$(size "$docs/$file")" = "$bytes" ] && [ "$(sha256 "$docs/$file")" = "$sum" ] || die "$id: $docs/$file is not the file the list names ($bytes bytes, SHA-256 $sum)"
    files+=("$docs/$file")
  done < <(jq -r '.documents[] | [.id, .file, .url, (.bytes | tostring), .sha256] | @tsv' "$LIST")
  notes="$(mktemp)"
  {
    echo "Copies of the starter library's documents, exactly as their publishers published them: each is the file content/library/starter.json names by its size and SHA-256. They are kept here so the library can be built again when a publisher's site turns automated downloads away (the library workflow takes a document from here only then, and checks it as always). Each document stays its publisher's; download it from its publisher where you can, as they keep their guidance up to date."
    echo
    jq -r '.documents[] | "- **\(.title)**\(if .year then " (\(.year))" else "" end), \(.publisher). `\(.file)`, from \(.url)\n  Licence: \(.licence)\(if .conditions then "; \(.conditions)" else "" end). Terms: \(.licence_url)"' "$LIST"
  } > "$notes"
  draft_if_missing "$SOURCES_TAG" "Lastbook Europe: the starter library's sources" "$notes"
  gh_write release upload "$SOURCES_TAG" --repo "$REPO" --clobber "${files[@]}"
  echo "${DRY_RUN:+dry run: }$SOURCES_TAG: ${#files[@]} documents, each checked against the list"
  rm -f "$notes"
}

starter() { # ZIP ENTRY
  local notes
  check "$1" "$2"
  notes="$(mktemp)"
  echo "The data Lastbook Europe downloads during setup: the library packs, the region maps, the address indexes and their manifest (manifest-v1.json). A draft until the maintainer publishes it: setup downloads only from a published release." > "$notes"
  has_release "$DATA_TAG" || draft_if_missing "$DATA_TAG" "Lastbook Europe data (v1)" "$notes"
  gh_write release upload "$DATA_TAG" --repo "$REPO" --clobber "$1" "$2"
  echo "${DRY_RUN:+dry run: }$DATA_TAG: $(basename "$1") and $(basename "$2") uploaded"
  rm -f "$notes"
}

case "${1:-}" in
  sources) shift; sources "$@" ;;
  check) shift; [ $# -eq 2 ] || die "check ZIP ENTRY"; check "$@" ;;
  starter) shift; [ $# -eq 2 ] || die "starter ZIP ENTRY"; starter "$@" ;;
  *) sed -n '2,24p' "$0" | sed 's/^# \{0,1\}//'; exit 2 ;;
esac
