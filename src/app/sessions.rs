//! The open sessions, one window each, and what they report back.

use adw::prelude::*;
use relm4::prelude::*;

use super::{AppCmd, AppModel, AppMsg};
use crate::config;
use crate::secrets;
use crate::ui::session::{Session, SessionInput, SessionOutput};

impl AppModel {
    /// Open a session to the connection, or bring its window forward when
    /// one is already open.
    pub(super) fn connect(&mut self, id: &str, sender: &ComponentSender<Self>) {
        if let Some(session) = self.sessions.get(id) {
            session.emit(SessionInput::Present);
            return;
        }
        let Some(connection) = self.connections.iter().find(|c| c.id == id).cloned() else {
            return;
        };
        let session =
            Session::builder()
                .launch(connection)
                .forward(sender.input_sender(), |output| match output {
                    SessionOutput::Connected(id) => AppMsg::SessionConnected(id),
                    SessionOutput::Remember {
                        id,
                        username,
                        domain,
                        password,
                    } => AppMsg::SessionRemember {
                        id,
                        username,
                        domain,
                        password,
                    },
                    SessionOutput::Closed(id) => AppMsg::SessionClosed(id),
                });
        session.widget().present();
        self.sessions.insert(id.to_owned(), session);
    }

    pub(super) fn session_connected(&mut self, id: &str) {
        let Some(connection) = self.connections.iter_mut().find(|c| c.id == id) else {
            return;
        };
        connection.last_used = gtk::glib::real_time() / 1_000_000;
        config::save_connections(&self.connections);
        self.rebuild_list();
        if self.selected.as_deref() == Some(id) {
            self.select(Some(id.to_owned()));
        }
    }

    /// Keep the account typed at sign-in: the password in the keyring, the
    /// user name and domain in the profile.
    pub(super) fn remember(
        &mut self,
        id: &str,
        username: String,
        domain: String,
        password: String,
        sender: &ComponentSender<Self>,
    ) {
        let Some(connection) = self.connections.iter_mut().find(|c| c.id == id) else {
            return;
        };
        connection.username = username;
        connection.domain = domain;
        let label = secrets::label(connection.title());
        config::save_connections(&self.connections);
        let saved = id.to_owned();
        sender.oneshot_command(async move {
            AppCmd::Keyring(secrets::set_password(saved, label, password).await)
        });
        self.rebuild_list();
        if self.selected.as_deref() == Some(id) {
            self.select(Some(id.to_owned()));
        }
    }
}
