#!/bin/bash
# .deb for the Ubuntu/Debian release of the container image, named after it.
# Images: ubuntu:24.04, ubuntu:26.04, debian:13 ...
. /src/host/packaging/linux/common.sh

export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq build-essential curl file patch pkg-config clang libclang-dev \
    nasm meson ninja-build libpipewire-0.3-dev libspa-0.2-dev libsdl2-dev \
    libavcodec-dev libavdevice-dev libavformat-dev libavutil-dev libswresample-dev \
    libusb-1.0-0-dev dpkg-dev >/dev/null

install_rust
copy_source
build_extendo
cargo install cargo-deb --locked --quiet 2>/dev/null || cargo install cargo-deb --quiet

. /etc/os-release
TAG="${ID}${VERSION_ID}"
cd "$WORK/host"
DEB=$(cargo deb --no-build --deb-revision "1~$TAG" 2>/dev/null | tail -n1)
# GitHub release files can't contain "~".
NAMED="$(dirname "$DEB")/$(basename "$DEB" | tr '~' '.')"
mv "$DEB" "$NAMED"
publish "$NAMED"
