//! Translations, through gettext.
//!
//! Every user-facing string goes through [`i18n`] (or [`i18n_f`] when it has
//! placeholders), which looks it up in the catalogue for the session's
//! language and falls back to the English source string. Placeholders are
//! named, `i18n_f("Connecting to {host}", &[("host", &host)])`, so a
//! translation can reorder them.

use std::path::{Path, PathBuf};

use gettextrs::LocaleCategory;

const DOMAIN: &str = "mote";

/// Where compiled catalogues live: `MOTE_LOCALEDIR` when set, then the
/// source tree's `po/.build` (from `tools/build-locale.sh`), then the
/// prefix the binary runs from, then the system's. The first that holds a
/// catalogue wins.
fn locale_dir() -> PathBuf {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(dir) = std::env::var_os("MOTE_LOCALEDIR") {
        candidates.push(PathBuf::from(dir));
    }
    candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("po/.build"));
    let prefix = std::env::current_exe().ok().and_then(|exe| {
        exe.parent()
            .and_then(Path::parent)
            .map(|p| p.join("share/locale"))
    });
    candidates.extend(prefix.clone());
    candidates.push(PathBuf::from("/usr/share/locale"));
    candidates
        .into_iter()
        .find(|dir| has_catalogue(dir))
        .or(prefix)
        .unwrap_or_else(|| PathBuf::from("/usr/share/locale"))
}

/// Whether `dir/<lang>/LC_MESSAGES/mote.mo` exists for any language.
fn has_catalogue(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry
                .path()
                .join("LC_MESSAGES")
                .join(format!("{DOMAIN}.mo"))
                .is_file()
        })
    })
}

/// Bind the text domain. Called once, first thing in `main`, before GTK
/// (which would otherwise set the locale without our domain in place).
pub(crate) fn init() {
    // A locale the C library lacks leaves the process in "C", which gettext
    // treats as ASCII; C.UTF-8 is built into glibc and keeps every string.
    // SAFETY: called at the start of main, before any other thread exists
    // (setlocale and set_var are not thread-safe).
    unsafe {
        if gettextrs::setlocale(LocaleCategory::LcAll, "").is_none() {
            std::env::set_var("LC_ALL", "C.UTF-8");
            gettextrs::setlocale(LocaleCategory::LcAll, "C.UTF-8");
        }
    }
    let _ = gettextrs::bindtextdomain(DOMAIN, locale_dir());
    let _ = gettextrs::bind_textdomain_codeset(DOMAIN, "UTF-8");
    let _ = gettextrs::textdomain(DOMAIN);
}

/// Marks `msgid` for the translation template where it can't be translated
/// yet, in a `const`; [`i18n`] translates it where it is shown.
pub(crate) const fn i18n_noop(msgid: &'static str) -> &'static str {
    msgid
}

/// The translation of `msgid`.
pub(crate) fn i18n(msgid: &str) -> String {
    gettextrs::gettext(msgid)
}

/// The translation of `msgid`, with each `{name}` replaced by its value.
pub(crate) fn i18n_f(msgid: &str, args: &[(&str, &dyn std::fmt::Display)]) -> String {
    fill(gettextrs::gettext(msgid), args)
}

/// The singular or plural translation for `n`, as the language counts, with
/// placeholders filled as in [`i18n_f`].
pub(crate) fn ni18n_f(
    single: &str,
    plural: &str,
    n: u32,
    args: &[(&str, &dyn std::fmt::Display)],
) -> String {
    fill(gettextrs::ngettext(single, plural, n), args)
}

fn fill(mut text: String, args: &[(&str, &dyn std::fmt::Display)]) -> String {
    for (name, value) in args {
        text = text.replace(&format!("{{{name}}}"), &value.to_string());
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_are_named_and_reorderable() {
        let s = i18n_f("{b} then {a}", &[("a", &1), ("b", &"two")]);
        assert_eq!(s, "two then 1");
    }
}
