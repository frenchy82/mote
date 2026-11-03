//! Starting a session: the desktop asked for, the account, and the way
//! to the computer.

use adw::prelude::*;
use gtk::glib;
use relm4::prelude::*;

use super::{Page, Session, SessionCmd, SessionInput, clipboard, keyboard, window_of};
use crate::i18n::i18n_f;
use crate::rdp;

impl Session {
    pub(super) fn connect(&mut self, sender: &ComponentSender<Self>) {
        self.clipboard = None;
        if let Some(handle) = self.handle.take() {
            handle.send(rdp::Command::Close);
            self.finished();
        }
        self.page = Page::Connecting;
        self.connecting.set_title(&i18n_f(
            "Connecting to {name}…",
            &[("name", &self.connection.title())],
        ));
        // The desktop is asked for at the page's size, so wait for there to
        // be one.
        let Some((width, height, scale)) = self.desktop_size() else {
            // Polled rather than tied to the page's frame clock, which
            // doesn't run while another connection is showing.
            let input = sender.input_sender().clone();
            let stack = self.stack.downgrade();
            glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                let Some(stack) = stack.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                let sized = stack.width() > 0 || window_of(&stack).is_some_and(|w| w.width() > 0);
                if !sized {
                    return glib::ControlFlow::Continue;
                }
                input.emit(SessionInput::Connect);
                glib::ControlFlow::Break
            });
            return;
        };
        self.display.set_asked(width, height);
        let target = rdp::Target {
            host: self.connection.host.clone(),
            port: self.connection.port,
            username: self.username.clone(),
            password: self.password.to_string(),
            domain: self.domain.clone(),
            width,
            height,
            scale,
            quality: self.connection.quality,
            network: self.connection.network,
            clipboard: self.connection.clipboard,
            clipboard_files: self.connection.clipboard_files,
            sound: self.connection.sound,
            microphone: self.connection.microphone,
            folder: self.connection.shared_folder(),
            via: self.via(),
            wake: self.connection.wake_address(),
            reconnect: self.reconnect,
            keyboard_layout: keyboard::layout_id(self.keyboard_layout, &self.display),
        };
        let (handle, mut events) = rdp::start(target);
        self.begun = true;
        let input = handle.clone();
        self.display
            .set_input(move |events| input.send(rdp::Command::Input(events)));
        if self.connection.clipboard {
            self.clipboard = Some(clipboard::Sharing::new(
                &self.display,
                handle.clone(),
                self.connection.clipboard_files,
            ));
        }
        self.handle = Some(handle);
        self.attempt += 1;
        let attempt = self.attempt;
        sender.command(move |out, shutdown| {
            shutdown
                .register(async move {
                    while let Some(event) = events.recv().await {
                        if out.send(SessionCmd::Event(attempt, event)).is_err() {
                            break;
                        }
                    }
                })
                .drop_on_shutdown()
        });
    }

    /// The way to the computer: through the SSH server or the gateway, if
    /// the connection has one.
    fn via(&self) -> rdp::Via {
        let c = &self.connection;
        if let Some((host, port)) = c.ssh_server() {
            let username = if c.ssh_username.is_empty() {
                glib::user_name().to_string_lossy().into_owned()
            } else {
                c.ssh_username.clone()
            };
            return rdp::Via::Ssh(rdp::Hop {
                host,
                port,
                username,
                password: self.ssh_password.to_string(),
            });
        }
        if let Some((host, port)) = c.gateway_server() {
            // Without an account of its own, the gateway takes the one
            // signing in to the computer, as Windows' client does.
            let (username, password) = if c.gateway_username.is_empty() {
                let account = if self.domain.is_empty() {
                    self.username.clone()
                } else {
                    format!("{}\\{}", self.domain, self.username)
                };
                (account, self.password.to_string())
            } else {
                (
                    c.gateway_username.clone(),
                    self.gateway_password.to_string(),
                )
            };
            return rdp::Via::Gateway(rdp::Hop {
                host,
                port,
                username,
                password,
            });
        }
        rdp::Via::Direct
    }
}
