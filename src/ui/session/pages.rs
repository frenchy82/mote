//! The session window's pages around the desktop, and the certificate
//! question.

use adw::prelude::*;
use relm4::prelude::*;

use super::{Session, SessionInput};
use crate::connection::{Connection, Scaling};
use crate::i18n::{i18n, i18n_f};

/// What the sign-in page holds.
pub(super) struct Account {
    pub(super) username: String,
    pub(super) domain: String,
    pub(super) password: String,
    pub(super) remember: bool,
}

/// Asks for the account a connection has no saved password for, or for
/// another after the server refused one.
pub(super) struct SignIn {
    pub(super) page: adw::StatusPage,
    username: adw::EntryRow,
    password: adw::PasswordEntryRow,
    domain: adw::EntryRow,
    remember: adw::SwitchRow,
}

impl SignIn {
    pub(super) fn new(connection: &Connection, sender: &ComponentSender<Session>) -> Self {
        let page = adw::StatusPage::new();
        page.set_icon_name(Some("dialog-password-symbolic"));
        page.set_title(&i18n("Sign In"));

        let username = adw::EntryRow::new();
        username.set_title(&i18n("User Name"));
        username.set_text(&connection.username);
        let password = adw::PasswordEntryRow::new();
        password.set_title(&i18n("Password"));
        let domain = adw::EntryRow::new();
        domain.set_title(&i18n("Domain"));
        domain.set_text(&connection.domain);
        let remember = adw::SwitchRow::new();
        remember.set_title(&i18n("Remember Password"));
        remember.set_active(true);

        let group = adw::PreferencesGroup::new();
        for row in [
            username.upcast_ref::<gtk::Widget>(),
            password.upcast_ref(),
            domain.upcast_ref(),
            remember.upcast_ref(),
        ] {
            group.add(row);
        }
        for row in [&username, password.upcast_ref(), &domain] {
            let input = sender.input_sender().clone();
            row.connect_entry_activated(move |_| input.emit(SessionInput::SignIn));
        }

        let connect = gtk::Button::with_label(&i18n("Connect"));
        connect.add_css_class("pill");
        connect.add_css_class("suggested-action");
        connect.set_halign(gtk::Align::Center);
        let input = sender.input_sender().clone();
        connect.connect_clicked(move |_| input.emit(SessionInput::SignIn));

        let content = gtk::Box::new(gtk::Orientation::Vertical, 24);
        content.append(&group);
        content.append(&connect);
        let clamp = adw::Clamp::new();
        clamp.set_maximum_size(400);
        clamp.set_child(Some(&content));
        page.set_child(Some(&clamp));

        Self {
            page,
            username,
            password,
            domain,
            remember,
        }
    }

    pub(super) fn account(&self) -> Account {
        Account {
            username: self.username.text().trim().to_owned(),
            domain: self.domain.text().trim().to_owned(),
            password: self.password.text().to_string(),
            remember: self.remember.is_active(),
        }
    }

    /// Put the cursor where typing should start.
    pub(super) fn focus(&self) {
        if self.username.text().trim().is_empty() {
            self.username.grab_focus();
        } else {
            self.password.grab_focus();
        }
    }

    /// The server turned the account down: say so, and ask again.
    pub(super) fn refused(&self, message: &str) {
        self.page
            .set_description(Some(&gtk::glib::markup_escape_text(message)));
        self.password.set_text("");
        self.focus();
    }
}

/// The session menu. Detached, it offers the way back.
pub(super) fn menu(detached: bool) -> gtk::gio::Menu {
    let menu = gtk::gio::Menu::new();
    let place = gtk::gio::Menu::new();
    place.append(
        Some(&if detached {
            i18n("Return to Main Window")
        } else {
            i18n("Open in New Window")
        }),
        Some("session.move"),
    );
    menu.append_section(None, &place);
    let scaling = gtk::gio::Menu::new();
    for mode in Scaling::ALL {
        let item = gtk::gio::MenuItem::new(Some(&scaling_label(mode)), None);
        item.set_action_and_target_value(Some("session.scaling"), Some(&mode.key().to_variant()));
        scaling.append_item(&item);
    }
    menu.append_section(None, &scaling);
    let end = gtk::gio::Menu::new();
    end.append(Some(&i18n("Disconnect")), Some("session.disconnect"));
    menu.append_section(None, &end);
    menu
}

/// The scaling modes as the menu and the editor name them.
pub(crate) fn scaling_label(mode: Scaling) -> String {
    match mode {
        Scaling::Resize => i18n("Resize to Window"),
        Scaling::Fit => i18n("Scale to Fit"),
        Scaling::Actual => i18n("Actual Size"),
    }
}

pub(super) fn connecting(connection: &Connection) -> adw::StatusPage {
    let page = adw::StatusPage::new();
    page.set_paintable(Some(&adw::SpinnerPaintable::new(Some(&page))));
    page.set_title(&i18n_f(
        "Connecting to {name}…",
        &[("name", &connection.title())],
    ));
    page
}

/// Shown when a session ends without the window closing; the reason goes
/// in its title and description.
pub(super) fn ended(sender: &ComponentSender<Session>) -> adw::StatusPage {
    let page = adw::StatusPage::new();
    page.set_icon_name(Some("network-offline-symbolic"));
    let reconnect = gtk::Button::with_label(&i18n("Reconnect"));
    reconnect.add_css_class("pill");
    reconnect.add_css_class("suggested-action");
    reconnect.set_halign(gtk::Align::Center);
    let input = sender.input_sender().clone();
    reconnect.connect_clicked(move |_| input.emit(SessionInput::Reconnect));
    page.set_child(Some(&reconnect));
    page
}

/// Ask whether to trust a server's certificate: one never seen before, or
/// one that differs from the certificate trusted last time. The answer is
/// the response "trust" or "cancel".
pub(super) fn certificate_dialog(name: &str, fingerprint: &str, changed: bool) -> adw::AlertDialog {
    let (heading, body) = if changed {
        (
            i18n("The Computer’s Identity Has Changed"),
            i18n_f(
                "{name} presented a different certificate from the one trusted before. Someone may be intercepting the connection, or the computer was set up again.",
                &[("name", &name)],
            ),
        )
    } else {
        (
            i18n_f("Trust {name}?", &[("name", &name)]),
            i18n(
                "Check that this fingerprint matches the one in the computer’s Remote Desktop settings.",
            ),
        )
    };
    let dialog = adw::AlertDialog::new(Some(&heading), None);
    dialog.set_body_use_markup(true);
    dialog.set_body(&format!(
        "{}\n\n<tt>{}</tt>",
        gtk::glib::markup_escape_text(&body),
        gtk::glib::markup_escape_text(&in_lines(fingerprint)),
    ));
    dialog.add_response("cancel", &i18n("Cancel"));
    if changed {
        dialog.add_response("trust", &i18n("Connect Anyway"));
        dialog.set_response_appearance("trust", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
    } else {
        dialog.add_response("trust", &i18n("Trust and Connect"));
        dialog.set_response_appearance("trust", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("trust"));
    }
    dialog.set_close_response("cancel");
    dialog
}

/// A SHA-256 fingerprint is 32 pairs; 8 to a line fits a dialog.
fn in_lines(fingerprint: &str) -> String {
    let pairs: Vec<&str> = fingerprint.split(':').collect();
    pairs
        .chunks(8)
        .map(|line| line.join(":"))
        .collect::<Vec<_>>()
        .join("\n")
}
