#!/bin/sh
# Builds Extendo's mirror engine: scrcpy (Apache-2.0) with Extendo's
# navigation bar patch, into packaging/mirror/extendo-mirror, plus the
# matching phone-side server. Needs: meson ninja-build libsdl2-dev
# libavcodec-dev libavdevice-dev libavformat-dev libavutil-dev
# libswresample-dev libusb-1.0-0-dev.
set -eu
HERE="$(cd "$(dirname "$0")" && pwd)"
VERSION=3.3.4
SRC_SHA256=03b72d1f71ca7783cff2995ca7cd8c67b9b75038a3cc35e443a1913e951b49dd
SERVER_SHA256=8588238c9a5a00aa542906b6ec7e6d5541d9ffb9b5d0f6e1bc0e365e2303079e
BASE="https://github.com/Genymobile/scrcpy"
OUT="$HERE/mirror"
WORK="$OUT/build-src"
mkdir -p "$OUT" "$WORK"
cd "$WORK"

if ! echo "$SRC_SHA256  scrcpy.tar.gz" | sha256sum -c >/dev/null 2>&1; then
    curl -sSfL -o scrcpy.tar.gz "$BASE/archive/refs/tags/v$VERSION.tar.gz"
    echo "$SRC_SHA256  scrcpy.tar.gz" | sha256sum -c
fi
rm -rf "scrcpy-$VERSION"
tar xzf scrcpy.tar.gz
cd "scrcpy-$VERSION"
patch -p1 < "$HERE/extendo-navbar.patch"
meson setup build --buildtype=release --strip -Db_lto=true -Dcompile_server=false >/dev/null
ninja -C build >/dev/null
install -m 755 build/app/scrcpy "$OUT/extendo-mirror"

cd "$OUT"
if ! echo "$SERVER_SHA256  scrcpy-server" | sha256sum -c >/dev/null 2>&1; then
    curl -sSfL -o scrcpy-server "$BASE/releases/download/v$VERSION/scrcpy-server-v$VERSION"
    echo "$SERVER_SHA256  scrcpy-server" | sha256sum -c
fi
cp "$WORK/scrcpy-$VERSION/LICENSE" LICENSE-scrcpy
echo "mirror engine ready: scrcpy $VERSION + Extendo navigation bar"
