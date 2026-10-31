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
#[allow(dead_code)]
pub(crate) fn i18n_f(msgid: &str, args: &[(&str, &dyn std::fmt::Display)]) -> String {
    let mut out = gettextrs::gettext(msgid);
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), &value.to_string());
    }
    out
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
