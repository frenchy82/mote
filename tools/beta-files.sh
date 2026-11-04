#!/usr/bin/env bash
#
# Make what the beta app has in place of the stable app's files: its launcher,
# metainfo, symbolic icon and Flatpak manifest. They are made from the stable
# ones for every beta build rather than kept beside them, so the two can't
# drift apart, and aren't committed.
#
#   tools/beta-files.sh [checkout]
#
# Only the Flatpak and the AppImage are the beta app (see docs/BUILDING.md).
set -euo pipefail
cd "${1:-$(dirname "$0")/..}"
STABLE=co.hyprlab.Mote
BETA=co.hyprlab.Mote.Beta

sed -e 's/^Name=Mote$/Name=Mote (beta)/' -e "s/$STABLE/$BETA/g" \
    "data/$STABLE.desktop" > "data/$BETA.desktop"

sed -e 's|<name>Mote</name>|<name>Mote (beta)</name>|' \
    -e "s|<id>$STABLE</id>|<id>$BETA</id>|" \
    -e "s|$STABLE.desktop|$BETA.desktop|" \
    "data/$STABLE.metainfo.xml" > "data/$BETA.metainfo.xml"

cp "data/icons/hicolor/symbolic/apps/$STABLE-symbolic.svg" \
    "data/icons/hicolor/symbolic/apps/$BETA-symbolic.svg"

{
    echo "# Made by tools/beta-files.sh from $STABLE.yml. Change that one."
    sed -e "s/$STABLE/$BETA/g" \
        -e 's/cargo --offline build --release --verbose/& --features beta/' \
        "$STABLE.yml"
} > "$BETA.yml"

# A stable manifest reworded so that these stop matching would otherwise
# build the stable app under the beta's name, without a word.
grep -q -- '--features beta' "$BETA.yml"
grep -q "^id: $BETA$" "$BETA.yml"
grep -q '^Name=Mote (beta)$' "data/$BETA.desktop"
grep -q "<id>$BETA</id>" "data/$BETA.metainfo.xml"
