#!/bin/bash
# Puts the program's own files beside the executables: what the app reads at run time (the interface and the guides; the
# pinned certificate roots are built into the executable). Usage: stage.sh DEST. The engine finds them as the folder holding web/index.html.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DEST="$1"
mkdir -p "$DEST"
rm -rf "$DEST/web" "$DEST/content"
cp -R "$ROOT/web" "$DEST/web"
cp -R "$ROOT/content" "$DEST/content"
cp "$ROOT/LICENSE" "$DEST/LICENSE.txt"
find "$DEST" -name .DS_Store -delete
