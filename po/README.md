# Translating Mote

Mote's interface text is translated with gettext. Everything a translator
needs is in this directory; no Rust knowledge is required, and no source
file has to be touched.

## Adding or updating a language

1. Take the template, `mote.pot`. It lists every string the app shows,
   with an empty slot for the translation.
2. Create `<lang>.po` from it, where `<lang>` is the language code (`fr`
   for French, `pt_BR` for Brazilian Portuguese):

       msginit --locale=fr --input=mote.pot --output=fr.po

   or open the template in Poedit or Gtranslator and save it as `fr.po`.
   To update an existing translation after the template changed, use the
   editor's "update from template" (or `msgmerge --update fr.po mote.pot`);
   new strings show up untranslated and changed ones as "fuzzy".
3. Translate. Keep the `{placeholders}` exactly as they are (they are
   filled in when the app runs), but move them around to suit the
   language. Key names such as Ctrl+Alt+Delete can follow what the
   language's keyboards print.
4. Add the language code to `LINGUAS` (one per line) if it is new.
5. Check it compiles: `msgfmt --check fr.po -o /dev/null`.
6. Send the `.po` file as a pull request, or attach it to an issue.

## Trying a translation in the app

From a source checkout:

    tools/build-locale.sh              # compiles po/*.po for a source-tree run
    LANG=fr_FR.UTF-8 ./target/debug/mote

The app follows the desktop's language.
