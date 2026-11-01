#!/usr/bin/env bash
# Regenerate the translation template (po/mote.pot) from the source, and
# refresh every po/<lang>.po against it.
#
# Strings reach the template through the helpers in src/i18n.rs: i18n(),
# i18n_f(), ni18n_f() and i18n_noop(), extracted by xtr (the Rust-aware
# xgettext: `cargo install xtr`), plus the launcher's entries through
# xgettext. Translators never touch the source: they edit po/<lang>.po
# (Poedit, Gtranslator, or any editor) and send that back.
#
#   tools/update-pot.sh            # run after strings change, before a release
set -euo pipefail
cd "$(dirname "$0")/.."
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT

cat > "$tmp/header.pot" <<'HDR'
# Translation template for Mote.
# This file is distributed under the same license as the Mote package.
#
msgid ""
msgstr ""
"Project-Id-Version: mote\n"
"Report-Msgid-Bugs-To: https://github.com/hyprlab/mote/issues\n"
"MIME-Version: 1.0\n"
"Content-Type: text/plain; charset=UTF-8\n"
"Content-Transfer-Encoding: 8bit\n"
"Plural-Forms: nplurals=2; plural=(n != 1);\n"
HDR
# Every source file, not only src/main.rs: xtr looks for a child module
# beside its parent file before looking in the parent's folder, so a child
# named like one of its parent's siblings would be read from the wrong file
# and its strings lost. main.rs first keeps the order.
xtr -k i18n -k i18n_f -k ni18n_f:1,2 -k i18n_noop \
    -o "$tmp/src.pot" src/main.rs $(find src -name '*.rs' | sort)
xgettext --from-code=UTF-8 --language=Desktop \
    -k --keyword=Name --keyword=GenericName --keyword=Comment --keyword=Keywords \
    -o "$tmp/desktop.pot" data/co.hyprlab.Mote.desktop
msgcat --use-first --no-location "$tmp/header.pot" "$tmp/src.pot" "$tmp/desktop.pot" \
  | sed '/^"POT-Creation-Date/d' > po/mote.pot

for po in po/*.po; do
  [ -e "$po" ] || continue
  msgmerge --quiet --update --backup=none "$po" po/mote.pot
done
echo "==> $(grep -c '^msgid ' po/mote.pot) strings in po/mote.pot"
