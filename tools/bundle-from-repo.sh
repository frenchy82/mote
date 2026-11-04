#!/usr/bin/env bash
#
# Make the Flatpak bundles of a published version, from the signed repository.
#
#   tools/bundle-from-repo.sh 1.3.0-beta.1
#
# Output lands in packaging/out/: Mote-<arch>.flatpak, or Mote-Beta-<arch>.flatpak
# for a beta. These are the bundles people download.
#
# A bundle that carries the repository's address and signing key installs with
# that repository as its origin, so it gets updates like any other install.
# Flatpak then refuses a commit the key didn't sign, and the key stays on the
# Flatout server, so CI's unsigned bundles can't carry them. This takes the
# commits the server signed instead, which also sit on the right branch, where
# CI's are on master and would look for updates on a branch nobody publishes.
# Upload CI's bundles first, so the version is there to fetch, then these.
set -euo pipefail

VERSION="${1:?usage: $0 VERSION}"
SITE="https://mote.hyprlab.co"
RUNTIME_REPO="https://dl.flathub.org/repo/flathub.flatpakrepo"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/packaging/out"

case "$VERSION" in
    *-beta.*) APP_ID="co.hyprlab.Mote.Beta"; NAME="Mote-Beta"; BRANCH="beta" ;;
    *) APP_ID="co.hyprlab.Mote"; NAME="Mote"; BRANCH="stable" ;;
esac

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$OUT"

# The key the packages trust, so a changed key on the server fails the pull
# rather than being passed on to everyone who downloads a bundle.
sed -n '/BEGIN PGP PUBLIC KEY BLOCK/,/END PGP PUBLIC KEY BLOCK/p' "$ROOT/packaging/debian/mote.sources" \
    | sed 's/^ //; s/^\.$//' | gpg --dearmor > "$WORK/key.gpg"

ostree init --repo="$WORK/repo" --mode=archive
ostree remote add --repo="$WORK/repo" --gpg-import="$WORK/key.gpg" site "$SITE/repo/"

for arch in x86_64 aarch64; do
    ref="app/$APP_ID/$arch/$BRANCH"
    echo "==> $ref"
    ostree pull --repo="$WORK/repo" --mirror site "$ref"
    # The channel's newest may be another version: the upload didn't finish,
    # or a newer one went out since.
    got="$(ostree cat --repo="$WORK/repo" "$ref" "/files/share/metainfo/$APP_ID.metainfo.xml" \
        | sed -n 's/.*<release [^>]*version="\([^"]*\)".*/\1/p' | head -1)"
    [ "$got" = "$VERSION" ] || { echo "$ref is $got, not $VERSION" >&2; exit 1; }
    flatpak build-bundle --arch="$arch" --runtime-repo="$RUNTIME_REPO" \
        --repo-url="$SITE/repo/" --gpg-keys="$WORK/key.gpg" \
        "$WORK/repo" "$OUT/$NAME-$arch.flatpak" "$APP_ID" "$BRANCH"
done

echo "==> Bundles in $OUT:"
ls -1 "$OUT/$NAME"-*.flatpak
