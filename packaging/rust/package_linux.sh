#!/bin/bash
# Lastbook Europe for Linux: a .deb (Debian, Ubuntu and their family) and a .tar.gz to unpack anywhere, for x86-64.
#   packaging/rust/package_linux.sh [OUT_DIR]          (default dist/)
# The .deb installs /opt/lastbook-europe/ (lastbook, lastbook-llm and the program's files), the command
# /usr/bin/lastbook-europe and a menu entry. The window is the system's GTK 3 and WebKitGTK 4.1.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="${1:-$ROOT/dist}"
VER="$("$ROOT/packaging/rust/version.sh")"
ARCH="$(dpkg --print-architecture 2>/dev/null || echo amd64)"
cd "$ROOT"
cargo build --release --locked -p lastbook-app -p lastbook-llm
STAGE="$(mktemp -d)"
OPT="$STAGE/opt/lastbook-europe"
mkdir -p "$OPT" "$STAGE/usr/bin" "$STAGE/usr/share/applications" "$STAGE/usr/share/pixmaps" "$STAGE/usr/share/doc/lastbook-europe" "$STAGE/DEBIAN"
cp target/release/lastbook target/release/lastbook-llm "$OPT/"
# llama.cpp's libraries and its backends (Vulkan, and the CPU variants), which the runner loads from beside itself
cp -P target/release/libllama*.so* target/release/libggml*.so* "$OPT/" 2>/dev/null || true
BACKENDS="$(find target/release/build -type d -path '*llama-cpp-sys-2-*/out/backends' | head -1)"
if [ -n "$BACKENDS" ]; then cp -P "$BACKENDS"/* "$OPT/"; fi
"$ROOT/packaging/rust/stage.sh" "$OPT"
ln -s /opt/lastbook-europe/lastbook "$STAGE/usr/bin/lastbook-europe"
cp packaging/icon/tile-1024.png "$STAGE/usr/share/pixmaps/lastbook-europe.png"
cat > "$STAGE/usr/share/applications/lastbook-europe.desktop" <<'DESK'
[Desktop Entry]
Type=Application
Name=Lastbook Europe
GenericName=Offline survival library
Comment=Read the survival library, the map and the guides on this computer, with no internet
Exec=/usr/bin/lastbook-europe
Icon=lastbook-europe
Terminal=false
Categories=Education;Literature;
Keywords=survival;library;offline;first aid;map;
StartupNotify=true
DESK
cp LICENSE "$STAGE/usr/share/doc/lastbook-europe/copyright"
SIZE=$(du -sk "$STAGE/opt" | cut -f1)
cat > "$STAGE/DEBIAN/control" <<CTRL
Package: lastbook-europe
Version: $VER
Architecture: $ARCH
Maintainer: Lastbook Europe <noreply@github.com>
Installed-Size: $SIZE
Depends: libwebkit2gtk-4.1-0 (>= 2.40), libgtk-3-0, libgstreamer1.0-0, gstreamer1.0-plugins-base, gstreamer1.0-plugins-good
Recommends: gstreamer1.0-libav, libvulkan1, mesa-vulkan-drivers
Section: education
Priority: optional
Homepage: https://github.com/NikosGu/lastbook_Europe
Description: Offline survival library for Europe
 Manuals, guides, videos, an offline map with help points and address search,
 and an optional local AI, all on this computer, with no internet.
CTRL
mkdir -p "$OUT"
DEB="$OUT/lastbook-europe_${VER}_${ARCH}.deb"
dpkg-deb --root-owner-group -Zxz --build "$STAGE" "$DEB" >/dev/null
TGZ="$OUT/LastbookEurope-$VER-linux-$ARCH.tar.gz"
tar -C "$STAGE/opt" -czf "$TGZ" lastbook-europe
rm -rf "$STAGE"
echo "$DEB"
echo "$TGZ"
