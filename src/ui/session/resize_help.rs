//! What to do when GNOME Desktop Sharing keeps its screen's size. Its
//! default screen-share mode mirrors the signed-in desktop's screen and
//! offers no Display Control channel, so a session can't resize it; the
//! extend mode gives the session a virtual monitor that follows the window.

use adw::prelude::*;

use super::h264_help::{command_box, paragraph};
use crate::i18n::{i18n, i18n_f};
use crate::rdp::server::Server;

const EXTEND: &str = "gsettings set org.gnome.desktop.remote-desktop.rdp screen-share-mode extend";
const RESTART: &str = "systemctl --user restart gnome-remote-desktop";
const MIRROR: &str = "gsettings reset org.gnome.desktop.remote-desktop.rdp screen-share-mode";

/// Only Desktop Sharing has a mode that resizes; Remote Login always
/// does, and the other servers have nothing to switch.
pub(super) fn applies(server: Option<Server>) -> bool {
    server == Some(Server::GnomeDesktop)
}

/// The toast pointing to the advice; `open` shows it.
pub(super) fn toast(open: impl Fn() + 'static) -> adw::Toast {
    let toast = adw::Toast::new(&i18n("Desktop Sharing keeps the remote screen’s size"));
    toast.set_button_label(Some(&i18n("How to Resize")));
    toast.set_timeout(10);
    toast.connect_button_clicked(move |_| open());
    toast
}

/// Show how to make the computer called `name` resize with the window.
pub(super) fn present(parent: &impl IsA<gtk::Widget>, name: &str) {
    let page = adw::PreferencesPage::new();

    let intro = adw::PreferencesGroup::new();
    intro.set_description(Some(&i18n_f(
        "{name} shares the screen of its signed-in desktop, which keeps the size it has there, so the picture is scaled to fit the window. GNOME Remote Desktop can give the session a screen of its own instead, which follows the window’s size.",
        &[("name", &name)],
    )));
    page.add(&intro);

    let steps = adw::PreferencesGroup::new();
    steps.set_title(&i18n("On the Remote Computer"));
    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    body.append(&paragraph(&i18n(
        "Signed in as the user whose desktop is shared, run:",
    )));
    body.append(&command_box(EXTEND));
    body.append(&paragraph(&i18n(
        "Then restart the server, and connect again:",
    )));
    body.append(&command_box(RESTART));
    steps.add(&body);
    page.add(&steps);

    let after = adw::PreferencesGroup::new();
    after.set_title(&i18n("What Changes"));
    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    body.append(&paragraph(&i18n(
        "The session then shows a new, empty screen beside the computer’s own, not what is on that screen. Windows already open stay on the computer’s screen until moved.",
    )));
    body.append(&paragraph(&i18n(
        "To share the computer’s own screen again, run:",
    )));
    body.append(&command_box(MIRROR));
    after.add(&body);
    page.add(&after);

    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.set_content(Some(&page));
    adw::Dialog::builder()
        .title(i18n("Resize Desktop Sharing"))
        .content_width(560)
        .content_height(600)
        .child(&view)
        .build()
        .present(Some(parent));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_desktop_sharing_gets_the_advice() {
        assert!(applies(Some(Server::GnomeDesktop)));
        for server in [Server::GnomeLogin, Server::Krdp, Server::Xrdp] {
            assert!(!applies(Some(server)));
        }
        assert!(!applies(None));
    }
}
