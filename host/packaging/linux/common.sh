# Shared steps for the container builds in this folder. Sourced by the
# per-format scripts, which run as root inside a clean distro container:
#   docker run --rm -v "$PWD":/src:ro -v "$PWD/dist/linux":/out IMAGE \
#       bash /src/host/packaging/linux/<format>.sh
set -eu

SRC=/src
WORK=/build
OUT=/out
export CARGO_HOME=/opt/cargo RUSTUP_HOME=/opt/rustup
export PATH="$CARGO_HOME/bin:$PATH"

version() {
    sed -n 's/^version = "\(.*\)"/\1/p' "$SRC/host/Cargo.toml" | head -n1
}

install_rust() {
    if ! command -v cargo >/dev/null; then
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
            | sh -s -- -y --profile minimal --no-modify-path >/dev/null
    fi
}

# Copies the source tree so the build never writes into the host checkout.
copy_source() {
    rm -rf "$WORK"
    mkdir -p "$WORK"
    cp -a "$SRC/host" "$SRC/assets" "$SRC/LICENSE" "$SRC/THIRD_PARTY_NOTICES.md" "$WORK/"
    rm -rf "$WORK/host/target" "$WORK/host/packaging/mirror/build-src" \
           "$WORK/host/packaging/mirror/extendo-mirror"
}

build_extendo() {
    "$WORK/host/packaging/bundle-mirror.sh"
    (cd "$WORK/host" && cargo build --release --locked)
}

# Lays out the files every package shares under $1 (a staging root).
stage_files() {
    root="$1"
    install -Dm755 "$WORK/host/target/release/extendo" "$root/usr/bin/extendo"
    install -Dm755 "$WORK/host/packaging/mirror/extendo-mirror" "$root/usr/lib/extendo/extendo-mirror"
    install -Dm644 "$WORK/host/packaging/mirror/scrcpy-server" "$root/usr/lib/extendo/scrcpy-server"
    install -Dm644 "$WORK/host/packaging/extendo.desktop" "$root/usr/share/applications/extendo.desktop"
    install -Dm644 "$WORK/assets/extendo-256.png" "$root/usr/share/icons/hicolor/256x256/apps/extendo.png"
    install -Dm644 "$WORK/assets/extendo-512.png" "$root/usr/share/icons/hicolor/512x512/apps/extendo.png"
    install -Dm644 "$WORK/assets/extendo-icon.svg" "$root/usr/share/icons/hicolor/scalable/apps/extendo.svg"
    install -Dm644 "$WORK/host/packaging/mirror/LICENSE-scrcpy" "$root/usr/share/doc/extendo/LICENSE-scrcpy"
    install -Dm644 "$WORK/LICENSE" "$root/usr/share/doc/extendo/LICENSE"
    install -Dm644 "$WORK/THIRD_PARTY_NOTICES.md" "$root/usr/share/doc/extendo/THIRD_PARTY_NOTICES.md"
}

# Gives the results to the user who owns the checkout.
publish() {
    mkdir -p "$OUT"
    cp "$@" "$OUT/"
    owner=$(stat -c %u:%g "$SRC")
    for f in "$@"; do chown "$owner" "$OUT/$(basename "$f")"; done
    echo "built: $*"
}
