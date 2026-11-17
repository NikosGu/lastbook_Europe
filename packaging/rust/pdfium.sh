#!/bin/bash
# PDFium for one system, from the build pinned in pdfium.txt: downloads the archive (kept in derived/pdfium/ for the next
# time), uses it only if its SHA-256 is the pinned one, and puts the library in LIB_DIR and its licence texts (PDFium's
# own and those of the libraries built into it) in LICENCE_DIR.
#   packaging/rust/pdfium.sh ASSET LIB_DIR LICENCE_DIR
#   e.g. packaging/rust/pdfium.sh pdfium-linux-x64.tgz "$OPT" "$OPT/licenses/pdfium"
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
ASSET="$1"
LIB_DIR="$2"
LIC_DIR="$3"
PINS="$ROOT/packaging/rust/pdfium.txt"
RELEASE="$(awk '$1 == "release" { print $2 }' "$PINS")"
SUM="$(awk -v a="$ASSET" '$1 == a { print $2 }' "$PINS")"
if [ -z "$RELEASE" ] || [ -z "$SUM" ]; then
  echo "pdfium.sh: $ASSET is not pinned in $PINS" >&2
  exit 1
fi
sha256() { if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1; }
CACHE="$ROOT/derived/pdfium/${RELEASE//\//-}"
ARCHIVE="$CACHE/$ASSET"
mkdir -p "$CACHE"
if [ ! -f "$ARCHIVE" ] || [ "$(sha256 "$ARCHIVE")" != "$SUM" ]; then
  rm -f "$ARCHIVE" "$ARCHIVE.part"
  curl -fsSL --retry 3 -o "$ARCHIVE.part" "https://github.com/bblanchon/pdfium-binaries/releases/download/${RELEASE//\//%2F}/$ASSET"
  GOT="$(sha256 "$ARCHIVE.part")"
  if [ "$GOT" != "$SUM" ]; then
    rm -f "$ARCHIVE.part"
    echo "pdfium.sh: $ASSET from $RELEASE has SHA-256 $GOT, not the pinned $SUM: not used" >&2
    exit 1
  fi
  mv "$ARCHIVE.part" "$ARCHIVE"
fi
case "$ASSET" in
  *-win-*) LIB=bin/pdfium.dll ;;
  *-mac-*) LIB=lib/libpdfium.dylib ;;
  *) LIB=lib/libpdfium.so ;;
esac
UNPACKED="$(mktemp -d)"
tar -xzf "$ARCHIVE" -C "$UNPACKED"
mkdir -p "$LIB_DIR" "$LIC_DIR"
cp "$UNPACKED/$LIB" "$LIB_DIR/"
cp "$UNPACKED/LICENSE" "$LIC_DIR/pdfium-binaries-LICENSE.txt"
cp "$UNPACKED"/licenses/* "$LIC_DIR/"
rm -rf "$UNPACKED"
echo "PDFium $RELEASE ($ASSET, SHA-256 checked): $LIB_DIR/$(basename "$LIB")"
