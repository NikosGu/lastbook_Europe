#!/bin/bash
# Lastbook Europe for macOS: "Lastbook Europe.app" in a disk image, for Apple silicon.
#   packaging/rust/package_macos.sh [OUT_DIR]          (default dist/)
# The app: Contents/MacOS/lastbook (the window and the engine) and lastbook-llm (the local AI runner), Contents/Resources/
# with the interface, the guides and setup's certificate roots, and Contents/Frameworks/libpdfium.dylib (PDFium, which
# draws PDF pages; its licence texts in Contents/Resources/licenses/pdfium). Not signed with a Developer ID yet (the maintainer's
# decision): it is signed ad hoc, which Apple silicon requires, and the person opens it the first time with Control-click → Open.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="${1:-$ROOT/dist}"
VER="$("$ROOT/packaging/rust/version.sh")"
NAME="Lastbook Europe"
cd "$ROOT"
cargo build --release --locked -p lastbook-app -p lastbook-llm
STAGE="$(mktemp -d)"
APP="$STAGE/$NAME.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$APP/Contents/Frameworks"
cp target/release/lastbook target/release/lastbook-llm "$APP/Contents/MacOS/"
"$ROOT/packaging/rust/stage.sh" "$APP/Contents/Resources"
"$ROOT/packaging/rust/pdfium.sh" pdfium-mac-arm64.tgz "$APP/Contents/Frameworks" "$APP/Contents/Resources/licenses/pdfium"
cp packaging/Lastbook.icns "$APP/Contents/Resources/Lastbook.icns"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>$NAME</string>
  <key>CFBundleDisplayName</key><string>$NAME</string>
  <key>CFBundleExecutable</key><string>lastbook</string>
  <key>CFBundleIdentifier</key><string>io.github.nikosgu.lastbook-europe</string>
  <key>CFBundleIconFile</key><string>Lastbook</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VER</string>
  <key>CFBundleVersion</key><string>$VER</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.education</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
codesign --force --deep --sign - "$APP"
codesign --verify --deep "$APP"
ln -s /Applications "$STAGE/Applications"
mkdir -p "$OUT"
DMG="$OUT/LastbookEurope-$VER-macos-arm64.dmg"
rm -f "$DMG"
hdiutil create -volname "$NAME" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
rm -rf "$STAGE"
echo "$DMG"
