#!/bin/sh
# Puts the mirroring engine (scrcpy, Apache-2.0) into packaging/mirror/ so
# the .deb ships it as /usr/lib/extendo/extendo-mirror. Uses the scrcpy
# installed on the build machine (sudo apt install scrcpy) and downloads the
# matching phone-side server, checked against the release's SHA256SUMS.txt.
set -eu
cd "$(dirname "$0")/mirror"
VERSION=$(/usr/bin/scrcpy --version | head -n1 | awk '{print $2}')
BASE="https://github.com/Genymobile/scrcpy/releases/download/v$VERSION"

curl -sSfL -o SHA256SUMS.txt "$BASE/SHA256SUMS.txt"
EXPECTED=$(grep " scrcpy-server-v$VERSION\$" SHA256SUMS.txt | awk '{print $1}')
if ! echo "$EXPECTED  scrcpy-server" | sha256sum -c >/dev/null 2>&1; then
    curl -sSfL -o scrcpy-server "$BASE/scrcpy-server-v$VERSION"
    echo "$EXPECTED  scrcpy-server" | sha256sum -c
fi
rm -f SHA256SUMS.txt
install -m 755 /usr/bin/scrcpy extendo-mirror
cp /usr/share/common-licenses/Apache-2.0 LICENSE-scrcpy
echo "mirror engine ready: scrcpy $VERSION"
