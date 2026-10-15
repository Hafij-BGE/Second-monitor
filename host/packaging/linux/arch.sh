#!/bin/bash
# Arch Linux package (.pkg.tar.zst), also fine for Manjaro and EndeavourOS.
# Image: archlinux:latest
. /src/host/packaging/linux/common.sh

pacman -Syu --noconfirm --needed base-devel clang curl nasm meson ninja pipewire \
    sdl2 ffmpeg libusb >/dev/null

install_rust
copy_source
build_extendo

V=$(version)
PKG="$WORK/pkg"
mkdir -p "$PKG"
stage_files "$PKG/stage"
cat > "$PKG/PKGBUILD" <<PKGBUILD
# Maintainer: Md Hafijur Rahman
pkgname=extendo
pkgver=$V
pkgrel=1
pkgdesc="Use an Android tablet as a second monitor over USB or WiFi"
arch=('x86_64')
url="https://github.com/Hafij-BGE/Second-monitor"
license=('MIT' 'Apache-2.0')
depends=('glibc' 'gcc-libs' 'pipewire' 'sdl2' 'ffmpeg' 'libusb')
optdepends=('android-tools: USB connections, phone mirroring and phone storage')
options=('!strip' '!debug')

package() {
    cp -a "$PKG/stage/." "\$pkgdir/"
}
PKGBUILD
# makepkg refuses to run as root.
useradd -m builder 2>/dev/null || true
chown -R builder "$PKG"
su builder -c "cd '$PKG' && PKGDEST='$PKG' makepkg -f --nodeps" >/dev/null
publish "$PKG"/extendo-"$V"-1-x86_64.pkg.tar.zst
