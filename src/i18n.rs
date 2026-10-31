//! Translations, through gettext.
//!
//! Every user-facing string goes through [`i18n`] (or [`i18n_f`] when it has
//! placeholders), which looks it up in the catalogue for the session's
//! language and falls back to the English source string. Placeholders are
//! named, `i18n_f("Connecting to {host}", &[("host", &host)])`, so a
//! translation can reorder them.

use gettextrs::LocaleCategory;

const DOMAIN: &str = "mote";

/// Where compiled catalogues live: `MOTE_LOCALEDIR` for a source-tree run,
/// otherwise the prefix the binary was built for.
fn locale_dir() -> String {
    std::env::var("MOTE_LOCALEDIR")
        .ok()
        .or_else(|| option_env!("MOTE_LOCALEDIR").map(str::to_owned))
        .unwrap_or_else(|| "/usr/share/locale".to_owned())
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
