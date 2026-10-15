#!/bin/bash
# RPM for Fedora (and other distributions with the same libraries).
# Image: fedora:42
. /src/host/packaging/linux/common.sh

dnf -y -q install gcc gcc-c++ clang clang-devel curl patch file pkgconf-pkg-config \
    nasm meson ninja-build pipewire-devel SDL2-devel ffmpeg-free-devel libusb1-devel \
    rpm-build >/dev/null

install_rust
copy_source
build_extendo

V=$(version)
TOP="$WORK/rpmbuild"
mkdir -p "$TOP"/{SPECS,BUILD,RPMS,SOURCES,SRPMS}
STAGE="$WORK/stage"
stage_files "$STAGE"

cat > "$TOP/SPECS/extendo.spec" <<SPEC
Name:           extendo
Version:        $V
Release:        1%{?dist}
Summary:        Use an Android tablet as a second monitor over USB or WiFi
License:        MIT AND Apache-2.0
URL:            https://github.com/Hafij-BGE/Second-monitor
Recommends:     android-tools

%description
Extendo streams a virtual GNOME monitor (or a mirrored one) to the Extendo
Android app over USB or WiFi, with touch and trackpad input, keyboard,
clipboard sync and file transfer. It also mirrors and controls phones and
opens their storage in the file manager. Made by Md Hafijur Rahman.

%install
cp -a $STAGE/. %{buildroot}/

%files
/usr/bin/extendo
/usr/lib/extendo
/usr/share/applications/extendo.desktop
/usr/share/icons/hicolor/*/apps/extendo.*
%doc /usr/share/doc/extendo
SPEC

# Prebuilt binaries: keep them as built (no debuginfo split or re-strip).
rpmbuild -bb --define "_topdir $TOP" --define "debug_package %{nil}" \
    --define "__strip /bin/true" "$TOP/SPECS/extendo.spec" >/dev/null
RPM=$(find "$TOP/RPMS" -name '*.rpm' | head -n1)
rpm -qp --requires "$RPM" | grep -v '^rpmlib' | sort -u | head -40
publish "$RPM"
