#!/usr/bin/env bash
# Emit the notes for ONE release, for its GitHub release body: the version's
# section of docs/RELEASE_NOTES.md, then the compare link.
#
#   tools/release-notes.sh 1.3.0 > temp/release/1.3.0-github.md
#   tools/release-notes.sh --base 1.3.0    only the tag its link compares from
set -euo pipefail
cd "$(dirname "$0")/.."

only_base=
if [ "${1:-}" = "--base" ]; then
  only_base=1
  shift
fi
ver="${1:?usage: release-notes.sh [--base] X.Y.Z (no leading v)}"
repo="hyprlab/mote"

# The newest stable tag on GitHub that sorts before the one given. Local tags
# are no use here, as a clone keeps the tags GitHub has since dropped.
newest_stable_before() {
  { git ls-remote --tags --refs origin 'v*' | sed 's#.*refs/tags/##' |
      grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$'
    echo "$1"
  } | sort -uV | awk -v tag="$1" '$0 == tag { print last; exit } { last = $0 }'
}

# The tag a release's compare link starts from. A dropped tag kills every link
# naming it, and a superseded beta's tag is dropped (docs/RELEASING.md), so the
# link can only start from a tag that stays: a beta from the last stable, a
# stable from the last tag of the series before its own, as Hylki does. A beta
# that only catches up with a stable lists little, which beats a 404.
compare_base() {
  local base
  case "$1" in
    *-beta.*)
      newest_stable_before "v${1%%-beta.*}"
      return
      ;;
  esac
  base=$(newest_stable_before "v${1%.*}.0")
  # The first series has no series before it.
  [ -n "$base" ] || base=$(newest_stable_before "v$1")
  printf '%s\n' "$base"
}

if [ -n "$only_base" ]; then
  compare_base "$ver"
  exit
fi

notes=$(awk -v head="^## What.s new in ${ver}\$" '
  /^## / { on = ($0 ~ head); next }
  on { print }
' docs/RELEASE_NOTES.md | awk 'NF { f = 1 } f' | tac | awk 'NF { f = 1 } f' | tac)
if [ -z "$notes" ]; then
  echo "no notes found for ${ver} in docs/RELEASE_NOTES.md" >&2
  exit 1
fi
printf '%s\n' "$notes"

base=$(compare_base "$ver")
if [ -n "$base" ]; then
  printf '\nFull changelog: [%s...v%s](https://github.com/%s/compare/%s...v%s)\n' \
    "$base" "$ver" "$repo" "$base" "$ver"
fi
