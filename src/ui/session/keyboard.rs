//! The keyboard layout a session tells the server about. Keys go as
//! scancodes whatever the layout; xrdp picks the session's X keyboard
//! layout by this ID, where the other servers keep their own.

use gtk::gdk;
use gtk::prelude::*;

use crate::i18n::i18n_noop;

/// A layout: the name xkeyboard-config gives it, which is what GDK reports,
/// and the Windows keyboard layout ID that stands for it.
pub(crate) struct Layout {
    pub(crate) name: &'static str,
    pub(crate) id: u32,
}

const fn layout(name: &'static str, id: u32) -> Layout {
    Layout { name, id }
}

/// The layouts xrdp knows (its `xrdp_keyboard.ini`), and a few more
/// Windows has.
pub(crate) const LAYOUTS: &[Layout] = &[
    layout(i18n_noop("English (US)"), 0x0000_0409),
    layout(
        i18n_noop("English (US, intl., with dead keys)"),
        0x0002_0409,
    ),
    layout(i18n_noop("English (Dvorak)"), 0x0001_0409),
    layout(i18n_noop("English (UK)"), 0x0000_0809),
    layout(i18n_noop("Belgian"), 0x0000_0813),
    layout(i18n_noop("Czech"), 0x0000_0405),
    layout(i18n_noop("Danish"), 0x0000_0406),
    layout(i18n_noop("Dutch"), 0x0000_0413),
    layout(i18n_noop("Finnish"), 0x0000_040B),
    layout(i18n_noop("French"), 0x0000_040C),
    layout(i18n_noop("French (Canada)"), 0x0000_1009),
    layout(i18n_noop("French (Switzerland)"), 0x0000_100C),
    layout(i18n_noop("German"), 0x0000_0407),
    layout(i18n_noop("German (Austria)"), 0x0000_0C07),
    layout(i18n_noop("German (Switzerland)"), 0x0000_0807),
    layout(i18n_noop("Greek"), 0x0000_0408),
    layout(i18n_noop("Hungarian"), 0x0000_040E),
    layout(i18n_noop("Italian"), 0x0000_0410),
    layout(i18n_noop("Japanese"), 0x0000_0411),
    layout(i18n_noop("Korean"), 0x0000_0412),
    layout(i18n_noop("Latvian"), 0x0000_0426),
    layout(i18n_noop("Norwegian"), 0x0000_0414),
    layout(i18n_noop("Polish"), 0x0000_0415),
    layout(i18n_noop("Portuguese"), 0x0000_0816),
    layout(i18n_noop("Portuguese (Brazil)"), 0x0000_0416),
    layout(i18n_noop("Russian"), 0x0000_0419),
    layout(i18n_noop("Slovenian"), 0x0000_0424),
    layout(i18n_noop("Spanish"), 0x0000_040A),
    layout(i18n_noop("Spanish (Latin American)"), 0x0000_080A),
    layout(i18n_noop("Swedish"), 0x0000_041D),
    layout(i18n_noop("Turkish"), 0x0000_041F),
    layout(i18n_noop("Ukrainian"), 0x0000_0422),
];

/// What US English stands for, the layout servers assume.
const US: u32 = 0x0000_0409;

/// The layout ID to send: `chosen` when one was chosen in Settings, else
/// the one this computer's keyboard has now.
pub(crate) fn layout_id(chosen: u32, widget: &impl IsA<gtk::Widget>) -> u32 {
    if chosen != 0 {
        return chosen;
    }
    let keyboard = widget.display().default_seat().and_then(|s| s.keyboard());
    let Some(name) = keyboard.as_ref().and_then(active_name) else {
        return US;
    };
    match from_name(&name) {
        Some(id) => id,
        None => {
            tracing::info!("no Windows layout for the keyboard's {name:?}; sending US English");
            US
        }
    }
}

fn active_name(keyboard: &gdk::Device) -> Option<String> {
    let names = keyboard.layout_names();
    let index = usize::try_from(keyboard.active_layout_index()).ok()?;
    names.get(index).map(|n| n.to_string())
}

/// The ID for a layout by xkeyboard-config's name; a variant the table
/// lacks, such as "German (no dead keys)", takes its base layout's.
fn from_name(name: &str) -> Option<u32> {
    if let Some(found) = LAYOUTS.iter().find(|l| l.name == name) {
        return Some(found.id);
    }
    let base = name.split(" (").next()?;
    LAYOUTS.iter().find(|l| l.name == base).map(|l| l.id)
}

#[cfg(test)]
mod tests {
    use super::from_name;

    #[test]
    fn names_find_their_layout() {
        assert_eq!(from_name("English (US)"), Some(0x409));
        assert_eq!(from_name("German (Switzerland)"), Some(0x807));
        assert_eq!(from_name("German (no dead keys)"), Some(0x407));
        assert_eq!(from_name("Klingon"), None);
    }
}
