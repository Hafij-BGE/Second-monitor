#!/bin/sh
# Builds Extendo's mirror engine for Windows on Linux: scrcpy (Apache-2.0)
# with Extendo's navigation bar patch, cross-compiled by scrcpy's own
# release script (the one that makes its official win64 zip). Output:
# packaging/mirror/win64/extendo-mirror.exe and the DLLs it was built
# against. Needs: meson ninja-build nasm mingw-w64 mingw-w64-tools
# libz-mingw-w64-dev curl.
set -eu
HERE="$(cd "$(dirname "$0")" && pwd)"
VERSION=3.3.4
SRC_SHA256=03b72d1f71ca7783cff2995ca7cd8c67b9b75038a3cc35e443a1913e951b49dd
BASE="https://github.com/Genymobile/scrcpy"
OUT="$HERE/mirror/win64"
WORK="$HERE/mirror/build-src-win"
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
# The release script names the build after `git describe` otherwise.
VERSION="v$VERSION" release/build_windows.sh 64

DIST=release/work/build-win64/dist
install -m 644 "$DIST/scrcpy.exe" "$OUT/extendo-mirror.exe"
cp "$DIST"/*.dll "$OUT/"
echo "Windows mirror engine ready: scrcpy $VERSION + Extendo navigation bar"
