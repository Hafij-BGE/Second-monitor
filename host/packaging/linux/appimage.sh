#!/bin/bash
# AppImage + portable tar.gz, built on Ubuntu 24.04 so they run on most
# distributions from 2024 on (glibc 2.39+; older PipeWire headers are too old).
# Image: ubuntu:24.04
. /src/host/packaging/linux/common.sh

export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq build-essential curl file patch pkg-config clang libclang-dev \
    nasm meson ninja-build libpipewire-0.3-dev libspa-0.2-dev libsdl2-dev \
    libavcodec-dev libavdevice-dev libavformat-dev libavutil-dev libswresample-dev \
    libusb-1.0-0-dev >/dev/null

install_rust
copy_source
build_extendo

V=$(version)
APPDIR="$WORK/AppDir"
stage_files "$APPDIR"

cd "$WORK"
LINUXDEPLOY=linuxdeploy-x86_64.AppImage
curl -sSfL -o "$LINUXDEPLOY" \
    "https://github.com/linuxdeploy/linuxdeploy/releases/download/1-alpha-20250213-2/$LINUXDEPLOY"
chmod +x "$LINUXDEPLOY"
# No FUSE in containers: run the tool from its extracted image.
export APPIMAGE_EXTRACT_AND_RUN=1 NO_STRIP=1
# PipeWire must come from the host so its plugins match its daemon.
ARCH=x86_64 LDAI_OUTPUT="Extendo-$V-x86_64.AppImage" ./"$LINUXDEPLOY" \
    --appdir "$APPDIR" \
    --executable "$APPDIR/usr/bin/extendo" \
    --deploy-deps-only "$APPDIR/usr/lib/extendo/extendo-mirror" \
    --exclude-library 'libpipewire-0.3.so*' \
    --desktop-file "$APPDIR/usr/share/applications/extendo.desktop" \
    --icon-file "$APPDIR/usr/share/icons/hicolor/256x256/apps/extendo.png" \
    --output appimage >/dev/null

# Portable folder: same files, started with ./extendo
PORT="extendo-$V-linux-x86_64"
mkdir -p "$PORT"
cp -a "$APPDIR/usr" "$PORT/"
cat > "$PORT/extendo" <<'SH'
#!/bin/sh
# Starts Extendo from this folder (no installation needed).
here="$(dirname "$(readlink -f "$0")")"
exec "$here/usr/bin/extendo" "$@"
SH
chmod +x "$PORT/extendo"
tar czf "$PORT.tar.gz" "$PORT"

publish "$WORK/Extendo-$V-x86_64.AppImage" "$WORK/$PORT.tar.gz"
