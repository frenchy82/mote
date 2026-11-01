#!/usr/bin/env bash
#
# Build the AppImage.
#
#   tools/build-appimage.sh          # builds it in a container, from the host
#   MOTE_APPIMAGE_SRC=<checkout> tools/build-appimage.sh
#                                    # builds another checkout (a tag) with
#                                    # this script
#
# Output lands in packaging/out/: Mote-<arch>.AppImage, and the .zsync an
# updater reads to fetch only what changed. Both go on the GitHub release,
# which is where the bundle looks for its updates.
#
# Unlike the RPM and the deb, this carries every library the app needs,
# down to glibc and the dynamic linker, so one file runs on any
# distribution. The machinery is pkgforge's: `quick-sharun` collects the
# libraries, rewrites the binary to load them from inside the bundle, and
# packs the result. See https://github.com/pkgforge-dev/Anylinux-AppImages.
#
# FFmpeg is left out on purpose: H.264 comes from whoever holds its patent
# license (docs/BUILDING.md#h264), so the AppImage decodes it with Cisco's
# OpenH264, downloaded from Settings, which needs the libstdc++ bundled
# below.
#
# The build happens inside Arch, for current libraries and because Arch
# keeps 32-bit libraries out of /usr/lib, which the collector relies on. Run
# from the host, this script re-runs itself in a container; in CI it is
# already inside one and builds in place.
set -euo pipefail

# HERE is the checkout this script is in; ROOT is the source being built.
# They differ when an older tag is built with today's script.
HERE="$(cd "$(dirname "$0")/.." && pwd)"
ROOT="${MOTE_APPIMAGE_SRC:-$HERE}"
ARCH="$(uname -m)"
IMAGE="${MOTE_APPIMAGE_IMAGE:-ghcr.io/pkgforge-dev/archlinux:latest}"
APP_ID="co.hyprlab.Mote"
NAME="Mote"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"

# ---------------------------------------------------------------- host side

if [ ! -f /etc/arch-release ]; then
    OUT="$ROOT/packaging/out"
    mkdir -p "$OUT"
    echo "==> Building in $IMAGE (podman)"
    # The tree is copied in rather than built in place: the build installs
    # itself into the container's /usr, and target/ must not be shared with
    # the host's own builds. Named volumes keep the container's target/ and
    # crates between runs. `label=disable` spares relabelling the working
    # tree for SELinux.
    exec podman run --rm \
        --security-opt label=disable \
        -v "$ROOT":/src:ro \
        -v "$HERE":/here:ro \
        -v "$OUT":/out \
        -v mote-appimage-target:/build/target \
        -v mote-appimage-cargo:/root/.cargo \
        "$IMAGE" \
        sh -euc '
            mkdir -p /build
            tar -C /src --exclude=./target --exclude=./temp --exclude=./.git \
                        --exclude=./build-dir --exclude=./.flatpak-builder \
                        --exclude=./packaging/out --exclude=./AppDir -cf - . \
                | tar -C /build -xf -
            MOTE_APPIMAGE_SRC=/build exec /here/tools/build-appimage.sh
        '
fi

# ------------------------------------------------------------ container side

# /out is the host's packaging/out when this script re-ran itself above; in
# CI nothing is mounted there and the artifacts stay in the checkout.
OUT="$([ -d /out ] && echo /out || echo "$ROOT/packaging/out")"
mkdir -p "$OUT"

echo "==> Installing build and runtime dependencies"
# Xvfb gives quick-sharun a display to start the app on under strace, which
# is how it finds the libraries loaded with dlopen at startup.
pacman -Syu --noconfirm --needed \
    base-devel git rust pkgconf patchelf wget strace file xorg-server-xvfb \
    gtk4 libadwaita libpulse opus gcc-libs \
    gettext desktop-file-utils adwaita-icon-theme hicolor-icon-theme

echo "==> Fetching quick-sharun"
sharun_base="https://raw.githubusercontent.com/pkgforge-dev/Anylinux-AppImages/refs/heads/main/useful-tools"
wget -q "$sharun_base/quick-sharun.sh"       -O /usr/bin/quick-sharun
wget -q "$sharun_base/get-debloated-pkgs.sh" -O /usr/bin/get-debloated-pkgs
chmod +x /usr/bin/quick-sharun /usr/bin/get-debloated-pkgs
# Slimmer builds of Mesa, libicu and the like.
get-debloated-pkgs --add-common --prefer-nano

echo "==> Building the release binary ($APP_ID $VERSION)"
cd "$ROOT"
cargo build --release

echo "==> Installing into /usr"
# The layout the RPM installs: the collector reads the app out of /usr, and
# the app's own paths are relative to the binary, so what works there works
# inside the bundle.
install -Dm755 target/release/mote /usr/bin/mote
install -d /usr/share/applications /usr/share/metainfo
msgfmt --desktop --template="data/$APP_ID.desktop" -d po -o "/usr/share/applications/$APP_ID.desktop"
msgfmt --xml --template="data/$APP_ID.metainfo.xml" -d po -o "/usr/share/metainfo/$APP_ID.metainfo.xml"
for po in po/*.po; do
    [ -e "$po" ] || continue
    lang="$(basename "$po" .po)"
    install -d "/usr/share/locale/$lang/LC_MESSAGES"
    msgfmt -o "/usr/share/locale/$lang/LC_MESSAGES/mote.mo" "$po"
done
for size in 256x256 512x512; do
    install -Dm644 "data/icons/hicolor/$size/apps/$APP_ID.png" \
        "/usr/share/icons/hicolor/$size/apps/$APP_ID.png"
done
install -Dm644 "data/icons/hicolor/scalable/apps/$APP_ID.svg" \
    "/usr/share/icons/hicolor/scalable/apps/$APP_ID.svg"
desktop-file-validate "/usr/share/applications/$APP_ID.desktop"

echo "==> Collecting the dependencies"
export APPDIR="$ROOT/AppDir"
export VERSION ARCH
export ICON="/usr/share/icons/hicolor/256x256/apps/$APP_ID.png"
export DESKTOP="/usr/share/applications/$APP_ID.desktop"
export OUTPATH="$OUT"
export OUTNAME="$NAME-$ARCH.AppImage"
# The window has to match the launcher's StartupWMClass or the desktop shows
# the app twice in the dock.
export STARTUPWMCLASS="$APP_ID"
export GTK_CLASS_FIX=1
# What AppImage updaters read to update in place: the .zsync beside the
# newest bundle of this architecture on the GitHub releases.
export UPINFO="gh-releases-zsync|hyprlab|mote|latest|$NAME-$ARCH.AppImage.zsync"

# Opened only once a session plays sound, so the collector, which watches
# the app start, wouldn't see them: PulseAudio's simple API and Opus. And
# libstdc++, which nothing here links but Cisco's OpenH264 needs.
quick-sharun /usr/bin/mote \
    /usr/lib/libpulse-simple.so.0 \
    /usr/lib/libopus.so.0 \
    /usr/lib/libstdc++.so.6

# FFmpeg's H.264 decoder isn't Mote's to hand out (see the top), should
# anything have brought it in.
if find "$APPDIR" -name 'libavcodec.so*' | grep -q .; then
    echo "==> Leaving out FFmpeg"
    find "$APPDIR" \( -name 'libavcodec.so*' -o -name 'libavutil.so*' \) -print -delete
fi

# Pin the message catalogues rather than leave them to be found: src/i18n.rs
# looks beside the binary, which happens to land in the right place in the
# bundle, and a layout change upstream should not quietly drop them.
echo 'MOTE_LOCALEDIR=${SHARUN_DIR}/share/locale' >> "$APPDIR/.env"

echo "==> Packing"
quick-sharun --make-appimage

echo "==> AppImage in $OUT:"
ls -lh "$OUT/$NAME-$ARCH.AppImage"*
