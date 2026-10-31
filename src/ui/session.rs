//! A session: asks for what the connection lacks, connects, shows the
//! remote desktop, and says plainly how it ended. It lives in the main
//! window's content pane, or in a window of its own once detached, and
//! brings its own header either way.

mod display;
mod pages;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};
use relm4::prelude::*;
use tokio::sync::oneshot;

use crate::connection::Connection;
use crate::i18n::{i18n, i18n_f};
use crate::rdp::{self, Ending, Event, Failure, Trust, Unreachable};
use crate::{config, secrets};
use display::Display;

pub(crate) struct Session {
    connection: Connection,
    page: Page,
    /// The account to sign in with: the profile's, or what was typed.
    username: String,
    domain: String,
    password: String,
    /// Save the typed account once a session starts.
    remember: bool,
    handle: Option<rdp::Handle>,
    /// Counts connections, so a late event of an earlier one is ignored.
    attempt: u32,
    connected_once: bool,
    /// In a window of its own rather than the main window.
    detached: bool,
    /// Whether the window holding it is fullscreen, shared with the
    /// pointer watch that shows the header at the screen's top edge.
    fullscreen: Rc<Cell<bool>>,
    /// The window whose fullscreen state is followed.
    watched: Option<gtk::Window>,
    display: Display,
    sign_in: pages::SignIn,
    ended: adw::StatusPage,
    stack: gtk::Stack,
    toasts: adw::ToastOverlay,
    menu: gio::Menu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    SignIn,
    Connecting,
    Desktop,
    Ended,
}

impl Page {
    fn name(self) -> &'static str {
        match self {
            Self::SignIn => "sign-in",
            Self::Connecting => "connecting",
            Self::Desktop => "desktop",
            Self::Ended => "ended",
        }
    }
}

#[derive(Debug)]
pub(crate) enum SessionInput {
    /// Connect with what the sign-in page holds.
    SignIn,
    Reconnect,
    /// End the session and close it.
    Disconnect,
    ToggleFullscreen,
    /// Ask to move into a window of its own, or back.
    ToggleDetached,
    /// Where it is now, as the main window placed it.
    SetDetached(bool),
    /// Its window was mapped, perhaps a different one than before.
    Mapped,
    FullscreenChanged(bool),
}

#[derive(Debug)]
pub(crate) enum SessionOutput {
    /// A session started; the connection was used just now.
    Connected(String),
    /// The account typed at sign-in worked, and is to be kept.
    Remember {
        id: String,
        username: String,
        domain: String,
        password: String,
    },
    Closed(String),
    ToggleSidebar,
    Detach(String),
    Attach(String),
}

pub(crate) enum SessionCmd {
    Password(Result<Option<String>, String>),
    Event(u32, Event),
}

impl std::fmt::Debug for SessionCmd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Password(_) => "Password",
            Self::Event(..) => "Event",
        })
    }
}

#[relm4::component(pub(crate))]
impl Component for Session {
    type Init = Connection;
    type Input = SessionInput;
    type Output = SessionOutput;
    type CommandOutput = SessionCmd;

    view! {
        adw::ToolbarView {
            connect_map => SessionInput::Mapped,

            add_top_bar = &adw::HeaderBar {
                #[wrap(Some)]
                set_title_widget = &adw::WindowTitle {
                    set_title: model.connection.title(),
                    set_subtitle: &model.connection.address(),
                },
                pack_start = &gtk::Button {
                    set_icon_name: "sidebar-show-symbolic",
                    set_tooltip_text: Some(&i18n("Toggle Sidebar")),
                    connect_clicked[sender] => move |_| {
                        let _ = sender.output(SessionOutput::ToggleSidebar);
                    },
                    #[watch]
                    set_visible: !model.detached && !model.fullscreen.get(),
                },
                pack_end = &gtk::MenuButton {
                    set_icon_name: "view-more-symbolic",
                    set_tooltip_text: Some(&i18n("Session Menu")),
                    #[watch]
                    set_menu_model: Some(&model.menu),
                },
                pack_end = &gtk::Button {
                    #[watch]
                    set_icon_name: if model.fullscreen.get() {
                        "view-restore-symbolic"
                    } else {
                        "view-fullscreen-symbolic"
                    },
                    #[watch]
                    set_tooltip_text: Some(&if model.fullscreen.get() {
                        i18n("Leave Fullscreen")
                    } else {
                        i18n("Fullscreen")
                    }),
                    connect_clicked => SessionInput::ToggleFullscreen,
                },
                pack_end = &gtk::Button {
                    set_icon_name: "window-new-symbolic",
                    set_tooltip_text: Some(&i18n("Open in New Window")),
                    connect_clicked => SessionInput::ToggleDetached,
                    #[watch]
                    set_visible: !model.detached,
                },
            },

            #[local_ref]
            #[wrap(Some)]
            set_content = toasts -> adw::ToastOverlay {
                #[local_ref]
                #[wrap(Some)]
                set_child = stack -> gtk::Stack {
                    set_transition_type: gtk::StackTransitionType::Crossfade,
                    add_named: (sign_in_page, Some("sign-in")),
                    add_named: (&pages::connecting(&model.connection), Some("connecting")),
                    add_named: (display, Some("desktop")),
                    add_named: (ended, Some("ended")),
                    #[watch]
                    set_visible_child_name: model.page.name(),
                },
            },
        }
    }

    fn init(
        connection: Connection,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let sign_in = pages::SignIn::new(&connection, &sender);
        let ended = pages::ended(&sender);
        let model = Session {
            username: connection.username.clone(),
            domain: connection.domain.clone(),
            password: String::new(),
            remember: false,
            page: Page::Connecting,
            handle: None,
            attempt: 0,
            connected_once: false,
            detached: false,
            fullscreen: Rc::new(Cell::new(false)),
            watched: None,
            display: Display::default(),
            sign_in,
            ended,
            stack: gtk::Stack::new(),
            toasts: adw::ToastOverlay::new(),
            menu: pages::menu(false),
            connection,
        };
        install_actions(&root, &sender);
        reveal_at_top_edge(&root, &model.fullscreen);
        let (stack, toasts, display, ended) = (
            &model.stack.clone(),
            &model.toasts.clone(),
            &model.display.clone(),
            &model.ended.clone(),
        );
        let sign_in_page = &model.sign_in.page.clone();
        let widgets = view_output!();

        if model.connection.username.trim().is_empty() {
            let mut model = model;
            model.page = Page::SignIn;
            model.sign_in.focus();
            return ComponentParts { model, widgets };
        }
        let id = model.connection.id.clone();
        sender.oneshot_command(async move { SessionCmd::Password(secrets::password(id).await) });
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match msg {
            SessionInput::SignIn => {
                let account = self.sign_in.account();
                if account.username.trim().is_empty() {
                    self.sign_in.focus();
                    return;
                }
                self.username = account.username;
                self.domain = account.domain;
                self.password = account.password;
                self.remember = account.remember;
                self.connect(&sender);
            }
            SessionInput::Reconnect => self.connect(&sender),
            SessionInput::Disconnect => {
                if let Some(handle) = self.handle.take() {
                    handle.send(rdp::Command::Close);
                }
                self.leave_fullscreen(root);
                let _ = sender.output(SessionOutput::Closed(self.connection.id.clone()));
            }
            SessionInput::ToggleFullscreen => {
                if let Some(window) = window_of(root) {
                    if self.fullscreen.get() {
                        window.unfullscreen();
                    } else {
                        window.fullscreen();
                        self.display.grab_focus();
                    }
                }
            }
            SessionInput::ToggleDetached => {
                // A window leaving fullscreen first comes back as it was.
                self.leave_fullscreen(root);
                let id = self.connection.id.clone();
                let _ = sender.output(if self.detached {
                    SessionOutput::Attach(id)
                } else {
                    SessionOutput::Detach(id)
                });
            }
            SessionInput::SetDetached(detached) => {
                self.detached = detached;
                self.menu = pages::menu(detached);
                if self.page == Page::Desktop {
                    self.display.grab_focus();
                }
            }
            SessionInput::Mapped => self.watch_window(root, &sender),
            SessionInput::FullscreenChanged(fullscreen) => {
                self.fullscreen.set(fullscreen);
                root.set_reveal_top_bars(!fullscreen);
                // Shown at the top edge, the header covers the desktop
                // rather than pushing it down.
                root.set_extend_content_to_top_edge(fullscreen);
                root.set_top_bar_style(if fullscreen {
                    adw::ToolbarStyle::Raised
                } else {
                    adw::ToolbarStyle::Flat
                });
            }
        }
    }

    fn update_cmd(
        &mut self,
        msg: Self::CommandOutput,
        sender: ComponentSender<Self>,
        root: &Self::Root,
    ) {
        match msg {
            SessionCmd::Password(Ok(Some(password))) => {
                self.password = password;
                self.connect(&sender);
            }
            SessionCmd::Password(result) => {
                if let Err(e) = result {
                    tracing::warn!("reading the saved password: {e}");
                }
                self.page = Page::SignIn;
                self.sign_in.focus();
            }
            SessionCmd::Event(attempt, event) if attempt == self.attempt => {
                self.event(event, &sender, root);
            }
            SessionCmd::Event(..) => {}
        }
    }
}

impl Session {
    /// Follow the fullscreen state of the window it is in, which changes
    /// when it moves between the main window and one of its own.
    fn watch_window(&mut self, root: &adw::ToolbarView, sender: &ComponentSender<Self>) {
        let Some(window) = window_of(root) else {
            return;
        };
        if self.watched.as_ref() == Some(&window) {
            return;
        }
        let input = sender.input_sender().clone();
        window.connect_fullscreened_notify(move |window| {
            input.emit(SessionInput::FullscreenChanged(window.is_fullscreen()));
        });
        sender.input(SessionInput::FullscreenChanged(window.is_fullscreen()));
        self.watched = Some(window);
    }

    fn leave_fullscreen(&self, root: &adw::ToolbarView) {
        if self.fullscreen.get()
            && let Some(window) = window_of(root)
        {
            window.unfullscreen();
        }
    }

    fn connect(&mut self, sender: &ComponentSender<Self>) {
        if let Some(handle) = self.handle.take() {
            handle.send(rdp::Command::Close);
        }
        self.page = Page::Connecting;
        // The desktop is asked for at the window's size, so wait for the
        // window to have one.
        if self.stack.width() == 0 {
            let input = sender.input_sender().clone();
            self.stack.add_tick_callback(move |stack, _| {
                if stack.width() == 0 {
                    return gtk::glib::ControlFlow::Continue;
                }
                input.emit(SessionInput::Reconnect);
                gtk::glib::ControlFlow::Break
            });
            return;
        }
        let (width, height) = self.desktop_size();
        let target = rdp::Target {
            host: self.connection.host.clone(),
            port: self.connection.port,
            username: self.username.clone(),
            password: self.password.clone(),
            domain: self.domain.clone(),
            width,
            height,
        };
        let (handle, mut events) = rdp::start(target);
        let input = handle.clone();
        self.display
            .set_input(move |events| input.send(rdp::Command::Input(events)));
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

    /// The size to ask the server for: what the window has room for.
    fn desktop_size(&self) -> (u16, u16) {
        let (w, h) = (self.stack.width(), self.stack.height());
        // Servers want an even width, and nothing smaller than 200.
        let w = (w.clamp(200, 8192) & !1) as u16;
        let h = h.clamp(200, 8192) as u16;
        (w, h)
    }

    fn event(&mut self, event: Event, sender: &ComponentSender<Self>, root: &adw::ToolbarView) {
        match event {
            Event::Certificate {
                host,
                port,
                fingerprint,
                reply,
            } => self.check_certificate(&host, port, fingerprint, reply, root),
            Event::Connected { width, height } => {
                tracing::info!("connected, {width}×{height}");
                self.page = Page::Desktop;
                self.display.grab_focus();
                if !self.connected_once {
                    self.connected_once = true;
                    let _ = sender.output(SessionOutput::Connected(self.connection.id.clone()));
                }
                if std::mem::take(&mut self.remember) {
                    let _ = sender.output(SessionOutput::Remember {
                        id: self.connection.id.clone(),
                        username: self.username.clone(),
                        domain: self.domain.clone(),
                        password: self.password.clone(),
                    });
                }
            }
            Event::Frame(frame) => {
                self.display.show_frame(frame);
                // After the frame is drawn: redrawing runs at a higher
                // priority than idle callbacks.
                if let Some(handle) = self.handle.clone() {
                    gtk::glib::idle_add_local_once(move || handle.send(rdp::Command::FrameDone));
                }
            }
            Event::Cursor(cursor) => self.display.set_remote_cursor(cursor),
            Event::Ended(ending) => {
                self.handle = None;
                self.ended(ending);
            }
        }
    }

    fn check_certificate(
        &self,
        host: &str,
        port: u16,
        fingerprint: String,
        reply: oneshot::Sender<Trust>,
        root: &adw::ToolbarView,
    ) {
        let known = config::trusted_fingerprint(host, port);
        if known.as_deref() == Some(fingerprint.as_str()) {
            let _ = reply.send(Trust::Known);
            return;
        }
        let reply = Rc::new(RefCell::new(Some(reply)));
        let dialog =
            pages::certificate_dialog(self.connection.title(), &fingerprint, known.is_some());
        let (host, port) = (host.to_owned(), port);
        dialog.connect_response(None, move |_, response| {
            let Some(reply) = reply.borrow_mut().take() else {
                return;
            };
            if response == "trust" {
                config::trust_certificate(&host, port, &fingerprint);
                let _ = reply.send(Trust::Accepted);
            } else {
                let _ = reply.send(Trust::Rejected);
            }
        });
        dialog.present(Some(root));
    }

    fn ended(&mut self, ending: Ending) {
        let was_connected = self.page == Page::Desktop;
        let (title, description) = match ending {
            // Closed from here: the window is on its way out.
            Ending::Closed => return,
            Ending::Failed(Failure::Refused) => {
                self.page = Page::SignIn;
                self.sign_in
                    .refused(&i18n("The user name or password was not accepted"));
                return;
            }
            Ending::Failed(Failure::Untrusted) => (
                i18n("Not Connected"),
                i18n("The computer’s identity was not trusted."),
            ),
            Ending::Failed(Failure::Unreachable(why)) => {
                let host = self.connection.host.as_str();
                let text = match why {
                    Unreachable::NotFound => {
                        i18n_f("{host} could not be found.", &[("host", &host)])
                    }
                    Unreachable::Refused => i18n_f(
                        "{host} is not accepting remote desktop connections.",
                        &[("host", &host)],
                    ),
                    Unreachable::NoAnswer => i18n_f("{host} did not answer.", &[("host", &host)]),
                    Unreachable::Closed => i18n_f(
                        "{host} closed the connection at once. Check that its remote desktop service is turned on and set up.",
                        &[("host", &host)],
                    ),
                    Unreachable::Other(error) => {
                        tracing::warn!("connecting: {error}");
                        i18n_f("{host} could not be reached.", &[("host", &host)])
                    }
                };
                (i18n("Could Not Connect"), text)
            }
            Ending::Failed(Failure::Lost(error)) => {
                tracing::warn!("connection lost: {error}");
                (
                    i18n("Disconnected"),
                    i18n("The connection to the computer was lost."),
                )
            }
            Ending::Failed(Failure::Protocol(error)) => {
                tracing::warn!("session failed: {error}");
                let title = if was_connected {
                    i18n("Disconnected")
                } else {
                    i18n("Could Not Connect")
                };
                (title, error)
            }
            Ending::ServerClosed(reason) => (
                i18n("Disconnected"),
                reason.unwrap_or_else(|| i18n("The computer ended the session.")),
            ),
        };
        self.ended.set_title(&title);
        self.ended
            .set_description(Some(&gtk::glib::markup_escape_text(&description)));
        self.page = Page::Ended;
    }
}

fn window_of(widget: &impl IsA<gtk::Widget>) -> Option<gtk::Window> {
    widget.root().and_downcast::<gtk::Window>()
}

/// The session menu's actions, under `session.`.
fn install_actions(root: &adw::ToolbarView, sender: &ComponentSender<Session>) {
    let group = gio::SimpleActionGroup::new();
    for (name, message) in [
        (
            "disconnect",
            (|| SessionInput::Disconnect) as fn() -> SessionInput,
        ),
        ("move", || SessionInput::ToggleDetached),
    ] {
        let action = gio::SimpleAction::new(name, None);
        let input = sender.input_sender().clone();
        action.connect_activate(move |_, _| input.emit(message()));
        group.add_action(&action);
    }
    root.insert_action_group("session", Some(&group));
}

/// Fullscreen, the header hides; the pointer at the top edge brings it back
/// until the pointer moves well clear of it.
fn reveal_at_top_edge(root: &adw::ToolbarView, fullscreen: &Rc<Cell<bool>>) {
    let motion = gtk::EventControllerMotion::new();
    // Before the desktop sees the pointer, so the header shows over it.
    motion.set_propagation_phase(gtk::PropagationPhase::Capture);
    let fullscreen = fullscreen.clone();
    motion.connect_motion(glib::clone!(
        #[weak]
        root,
        move |_, _, y| {
            if !fullscreen.get() {
                return;
            }
            if y <= 2.0 {
                root.set_reveal_top_bars(true);
            } else if root.reveals_top_bars() && y > f64::from(root.top_bar_height()) + 48.0 {
                root.set_reveal_top_bars(false);
            }
        }
    ));
    root.add_controller(motion);
}
