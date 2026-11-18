#!/bin/bash
# manifest-v1.json written again from what the data release holds now, and put into it: the model catalogue
# (packaging/data/manifest-base.json), every map-entry-*.json and addresses-entry-*.json among the release's files, and
# starter-entry.json when it is there, checked by `lastbook-tools build-manifest` as setup checks a manifest (an entry
# whose zip is not in the release whole is left out). Setup reads this file to learn what it can install, so it is
# written after every change to the release: by the Data manifest workflow, after each region, and after a library pack.
#
#   RELEASE=data-v1 GH_REPO=owner/repo [TOOLS=lastbook-tools] [BRANCH=name] packaging/data/write-manifest.sh [SUMMARY_FILE]
#
# The model catalogue is the branch's newest packaging/data/manifest-base.json (BRANCH, read through the GitHub API), so a
# run started on an older commit does not put an older catalogue back; without BRANCH, or when it cannot be read, this
# checkout's. Needs gh (with write access to the repository) and jq. The release is the one release with that tag, a
# draft or published; anything else stops here.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TOOLS="${TOOLS:-$ROOT/target/release/lastbook-tools}"
: "${RELEASE:?the tag of the data release}" "${GH_REPO:?the repository, as owner/name}"
summary="${1:-/dev/null}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# every release tagged so, drafts included (the list shows them to writers; a failure to read it stops here)
n=$(gh api "repos/$GH_REPO/releases?per_page=100" --paginate --jq ".[] | select(.tag_name == \"$RELEASE\") | .id" | wc -l | tr -d ' ')
if [ "$n" != 1 ]; then
  echo "write-manifest: $n releases are tagged $RELEASE, not one: the manifest is not written" >&2
  exit 1
fi
gh release view "$RELEASE" --json assets > "$work/assets.json"
mkdir -p "$work/entries"
jq -r '.assets[] | select(.state == null or .state == "uploaded") | .name
       | select(test("^(map|addresses)-entry-[A-Z0-9-]+[.]json$") or . == "starter-entry.json")' "$work/assets.json" > "$work/names.txt"
echo "$(wc -l < "$work/names.txt" | tr -d ' ') entries in $RELEASE"
while read -r name; do
  gh release download "$RELEASE" --dir "$work/entries" --pattern "$name"
done < "$work/names.txt"
base="$ROOT/packaging/data/manifest-base.json"
if [ -n "${BRANCH:-}" ] && gh api "repos/$GH_REPO/contents/packaging/data/manifest-base.json?ref=$BRANCH" --jq .content 2>/dev/null | base64 -d > "$work/base.json" 2>/dev/null \
   && jq -e '.version == 1 and (.models | type == "array")' "$work/base.json" >/dev/null 2>&1; then
  base="$work/base.json"
  echo "the model catalogue of $BRANCH"
fi
"$TOOLS" build-manifest --base "$base" --entries "$work/entries" \
  --assets "$work/assets.json" --out "$work/manifest-v1.json" > "$work/held.json"
jq -r '"### manifest-v1.json",
       "- maps: \(.maps | length) (\(.maps | join(" ")))",
       "- address indexes: \(.address_indexes | length)",
       "- packs: \(.packs | join(" "))",
       "- models: \(.models | join(" "))",
       (if (.left_out | length) > 0 then "- left out, their zips not in the release whole: \(.left_out | map(.entry) | join(" "))" else empty end),
       (if (.maps_without_addresses | length) > 0 then "- maps without their address index: \(.maps_without_addresses | join(" "))" else empty end)' \
  "$work/held.json" | tee -a "$summary"
gh release upload "$RELEASE" "$work/manifest-v1.json" --clobber
