#!/usr/bin/env bash
#
# Build the RPM from the host's release build.
#
#   tools/build-packages.sh
#
# The package lands in packaging/out/mote-<version>-1.<dist>.<arch>.rpm. It
# wraps a release binary built against this machine's libraries, so it is
# for the Fedora release it was built on.
set -euo pipefail

APP_ID="co.hyprlab.Mote"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/packaging/out"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
# An RPM version can't hold a dash there: 0.2.0-beta.1 becomes 0.2.0~beta.1,
# which sorts before 0.2.0.
PKG_VERSION="${VERSION//-/\~}"

[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"
mkdir -p "$OUT"

# Keep the spec's version in step with Cargo.toml.
sed -i "s/^Version:.*/Version:        $PKG_VERSION/" "$ROOT/packaging/fedora/mote.spec"

echo "==> Building the release binary"
( cd "$ROOT" && cargo build --release )

echo "==> Staging"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
stage="$work/mote-$PKG_VERSION-bin"
mkdir -p "$stage/icons/256x256" "$stage/icons/512x512" "$stage/icons/scalable" "$stage/icons/symbolic"
cp "$ROOT/target/release/mote" "$stage/mote"
cp "$ROOT/LICENSE" "$stage/LICENSE"
# The launcher and metainfo with their translated fields merged in, and a
# catalogue for each po/<lang>.po.
msgfmt --desktop --template="$ROOT/data/$APP_ID.desktop" -d "$ROOT/po" -o "$stage/$APP_ID.desktop"
msgfmt --xml --template="$ROOT/data/$APP_ID.metainfo.xml" -d "$ROOT/po" -o "$stage/$APP_ID.metainfo.xml"
for po in "$ROOT"/po/*.po; do
    [ -e "$po" ] || continue
    lang=$(basename "$po" .po)
    mkdir -p "$stage/locale/$lang/LC_MESSAGES"
    msgfmt -o "$stage/locale/$lang/LC_MESSAGES/mote.mo" "$po"
done
for size in 256x256 512x512; do
    cp "$ROOT/data/icons/hicolor/$size/apps/$APP_ID.png" "$stage/icons/$size/$APP_ID.png"
done
cp "$ROOT/data/icons/hicolor/scalable/apps/$APP_ID.svg" "$stage/icons/scalable/$APP_ID.svg"
cp "$ROOT/data/icons/hicolor/symbolic/apps/$APP_ID-symbolic.svg" "$stage/icons/symbolic/$APP_ID-symbolic.svg"
mkdir -p "$work/rpmbuild/SOURCES"
tar -C "$work" -cf "$work/rpmbuild/SOURCES/mote-$PKG_VERSION-bin.tar" "mote-$PKG_VERSION-bin"

echo "==> rpmbuild"
rpmbuild -bb --define "_topdir $work/rpmbuild" "$ROOT/packaging/fedora/mote.spec"
cp "$work/rpmbuild/RPMS/$(rpm --eval '%{_arch}')/"mote-*.rpm "$OUT/"
echo "==> $(ls "$OUT"/mote-*.rpm)"
