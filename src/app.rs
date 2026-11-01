//! The main window, laid out like Hylki's: a sidebar with the app's name and
//! main menu above it, New Connection and the connections beneath, and the
//! chosen connection beside it.

mod actions;
mod details;
mod files;
mod sessions;
mod showcase;
mod sidebar;

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use relm4::prelude::*;

use crate::config::{self, ColorScheme, Settings};
use crate::connection::{Connection, split_address};
use crate::i18n::{i18n, i18n_f};
use crate::secrets::{self, Secret};
use crate::ui::editor::{Editor, EditorInit, EditorOutput};
use crate::ui::preferences::{PrefChange, Preferences};
use crate::ui::session::{Session, SessionOutput};
use crate::{APP_ID, APP_NAME};

pub(crate) struct AppModel {
    settings: Settings,
    connections: Vec<Connection>,
    selected: Option<String>,
    /// The sidebar search, lower case.
    query: String,
    list: gtk::ListBox,
    hint: gtk::Label,
    search: gtk::SearchEntry,
    search_bar: gtk::SearchBar,
    details: adw::Bin,
    toasts: adw::ToastOverlay,
    split: adw::OverlaySplitView,
    /// Set while the list is rebuilt, so its selection changes aren't the
    /// user's.
    quiet: Rc<Cell<bool>>,
    editor: Option<Controller<Editor>>,
    prefs: Option<Controller<Preferences>>,
    /// The content pane: the chosen connection, or a session's page.
    panes: gtk::Stack,
    /// Open sessions, by connection.
    sessions: HashMap<String, Controller<Session>>,
    /// The connections whose session is connected right now.
    live: HashSet<String>,
    /// The windows of the sessions moved out of this one.
    detached: HashMap<String, adw::ApplicationWindow>,
}

#[derive(Debug)]
pub(crate) enum AppMsg {
    /// `host` fills the address in, from the search field.
    NewConnection(Option<String>),
    Select(String),
    Search(String),
    /// Enter in the search field.
    SearchActivated,
    FocusSearch,
    EditSelected,
    ConnectSelected,
    Session(SessionOutput),
    /// End a session, as closing its window does.
    CloseSession(String),
    DuplicateSelected,
    DeleteSelected,
    Delete(String),
    Saved {
        connection: Box<Connection>,
        /// The passwords changed in the editor; an empty one is forgotten.
        passwords: Vec<(Secret, String)>,
    },
    EditorClosed,
    ImportRdp,
    ImportRdpFiles(Vec<PathBuf>),
    ImportRemmina,
    ExportSelected,
    ExportTo(String, PathBuf),
    OpenSettings,
    Pref(PrefChange),
    About,
    Shortcuts,
    ShowcaseSettingsPage(String),
    ShowcaseSession(String),
    ShowcaseCapture(String),
}

#[derive(Debug)]
pub(crate) enum AppCmd {
    Keyring(Result<(), String>),
}

#[relm4::component(pub(crate))]
impl Component for AppModel {
    type Init = ();
    type Input = AppMsg;
    type Output = ();
    type CommandOutput = AppCmd;

    view! {
        adw::ApplicationWindow {
            set_title: Some(APP_NAME),
            set_icon_name: Some(APP_ID),
            set_default_size: (window.width, window.height),
            set_maximized: window.maximized,
            // Only the size is kept: on Wayland the compositor places windows.
            connect_close_request => move |w| {
                save_window_state(w);
                gtk::glib::Propagation::Proceed
            },

            #[local_ref]
            toasts -> adw::ToastOverlay {
                #[local_ref]
                #[wrap(Some)]
                set_child = split -> adw::OverlaySplitView {
                    set_min_sidebar_width: 220.0,
                    set_max_sidebar_width: 300.0,
                    set_sidebar_width_fraction: 0.25,

                    #[wrap(Some)]
                    set_sidebar = &adw::ToolbarView {
                        add_top_bar = &adw::HeaderBar {
                            add_css_class: "flat",
                            #[wrap(Some)]
                            set_title_widget = &gtk::Label {
                                set_label: APP_NAME,
                                add_css_class: "app-title",
                            },
                            pack_start = &gtk::Button {
                                set_icon_name: "list-add-symbolic",
                                set_tooltip_text: Some(&i18n("New Connection")),
                                update_property: &[gtk::accessible::Property::Label(&i18n("New Connection"))],
                                set_action_name: Some("win.new-connection"),
                            },
                            pack_end = &gtk::MenuButton {
                                set_icon_name: "open-menu-symbolic",
                                set_tooltip_text: Some(&i18n("Main Menu")),
                                update_property: &[gtk::accessible::Property::Label(&i18n("Main Menu"))],
                                set_primary: true,
                                add_css_class: "flat",
                                set_menu_model: Some(&main_menu),
                            },
                            // pack_end packs right to left: just left of the menu.
                            #[name = "search_button"]
                            pack_end = &gtk::ToggleButton {
                                set_icon_name: "system-search-symbolic",
                                set_tooltip_text: Some(&i18n("Search")),
                                update_property: &[gtk::accessible::Property::Label(&i18n("Search"))],
                            },
                        },

                        // Hidden until the search button or Ctrl+F; Escape
                        // closes it and clears the search.
                        #[local_ref]
                        add_top_bar = search_bar -> gtk::SearchBar {
                            #[local_ref]
                            #[wrap(Some)]
                            set_child = search -> gtk::SearchEntry {
                                set_hexpand: true,
                                set_placeholder_text: Some(&i18n("Search or Enter an Address")),
                                connect_search_changed[sender] => move |e| {
                                    sender.input(AppMsg::Search(e.text().into()));
                                },
                                connect_activate => AppMsg::SearchActivated,
                            },
                        },

                        #[wrap(Some)]
                        set_content = &gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,

                            gtk::ScrolledWindow {
                                set_vexpand: true,
                                set_hscrollbar_policy: gtk::PolicyType::Never,

                                #[wrap(Some)]
                                set_child = &gtk::Box {
                                    set_orientation: gtk::Orientation::Vertical,
                                    #[local_ref]
                                    list -> gtk::ListBox {},
                                    #[local_ref]
                                    hint -> gtk::Label {},
                                },
                            },
                        },
                    },

                    // The chosen connection, or the session open to it.
                    #[local_ref]
                    #[wrap(Some)]
                    set_content = panes -> gtk::Stack {
                    add_named[Some("browse")] = &adw::ToolbarView {
                        add_top_bar = &adw::HeaderBar {
                            add_css_class: "flat",
                            // The connection names itself in the page below.
                            #[wrap(Some)]
                            set_title_widget = &gtk::Label {},
                            pack_start = &gtk::Button {
                                set_icon_name: "sidebar-show-symbolic",
                                set_tooltip_text: Some(&i18n("Toggle Sidebar")),
                                update_property: &[gtk::accessible::Property::Label(&i18n("Toggle Sidebar"))],
                                connect_clicked[split = split.clone()] => move |_| {
                                    split.set_show_sidebar(!split.shows_sidebar());
                                },
                            },
                            pack_end = &gtk::MenuButton {
                                set_icon_name: "view-more-symbolic",
                                set_tooltip_text: Some(&i18n("Connection Menu")),
                                update_property: &[gtk::accessible::Property::Label(&i18n("Connection Menu"))],
                                set_menu_model: Some(&connection_menu),
                                #[watch]
                                set_visible: model.selected.is_some(),
                            },
                            pack_end = &gtk::Button {
                                set_icon_name: "document-edit-symbolic",
                                set_tooltip_text: Some(&i18n("Edit Connection")),
                                update_property: &[gtk::accessible::Property::Label(&i18n("Edit Connection"))],
                                set_action_name: Some("win.edit"),
                                #[watch]
                                set_visible: model.selected.is_some(),
                            },
                        },

                        #[wrap(Some)]
                        set_content = &gtk::Stack {
                            add_named[Some("empty")] = &adw::StatusPage {
                                set_icon_name: Some("co.hyprlab.Mote-symbolic"),
                                set_title: &i18n("No Connections"),
                                set_description: Some(&i18n("Add a connection, or import one from an .rdp file or from Remmina.")),
                                #[wrap(Some)]
                                set_child = &gtk::Button {
                                    set_label: &i18n("New Connection"),
                                    set_halign: gtk::Align::Center,
                                    add_css_class: "pill",
                                    add_css_class: "suggested-action",
                                    set_action_name: Some("win.new-connection"),
                                },
                            },
                            add_named[Some("none")] = &adw::StatusPage {
                                set_icon_name: Some("co.hyprlab.Mote-symbolic"),
                                set_title: &i18n("No Connection Selected"),
                            },
                            #[local_ref]
                            add_named[Some("details")] = details -> adw::Bin {},

                            // After the pages: naming one before it exists
                            // only earns a warning.
                            #[watch]
                            set_visible_child_name: model.content_page(),
                        },
                    },
                    },
                },
            },
        }
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        load_css();
        // Files pasted from a remote desktop in an earlier run have been
        // copied wherever they were pasted by now.
        std::thread::spawn(|| {
            let _ = std::fs::remove_dir_all(crate::ui::session::fetched_files());
        });
        // Only X11 reads a window's own icon; Wayland goes by the app ID.
        gtk::Window::set_default_icon_name(crate::APP_ID);
        let settings = config::load_settings();
        apply_color_scheme(settings.color_scheme);
        let window = settings.window.clone();

        let list = &gtk::ListBox::new();
        list.add_css_class("navigation-sidebar");
        let hint = &gtk::Label::new(None);
        hint.set_wrap(true);
        hint.set_visible(false);
        hint.add_css_class("dim-label");
        hint.add_css_class("sidebar-hint");
        let quiet = Rc::new(Cell::new(false));
        sidebar::connect_list(list, &quiet, &sender);

        let model = AppModel {
            settings,
            connections: config::load_connections(),
            selected: None,
            query: String::new(),
            list: list.clone(),
            hint: hint.clone(),
            search: gtk::SearchEntry::new(),
            search_bar: gtk::SearchBar::new(),
            details: adw::Bin::new(),
            toasts: adw::ToastOverlay::new(),
            split: adw::OverlaySplitView::new(),
            quiet,
            editor: None,
            prefs: None,
            panes: gtk::Stack::new(),
            sessions: HashMap::new(),
            live: HashSet::new(),
            detached: HashMap::new(),
        };
        let split = &model.split.clone();
        let panes = &model.panes.clone();
        let (search, search_bar, details, toasts) = (
            &model.search.clone(),
            &model.search_bar.clone(),
            &model.details.clone(),
            &model.toasts.clone(),
        );
        let main_menu = actions::main_menu();
        let connection_menu = actions::connection_menu();
        let widgets = view_output!();
        search_bar.connect_entry(search);
        search_bar
            .bind_property("search-mode-enabled", &widgets.search_button, "active")
            .bidirectional()
            .sync_create()
            .build();

        // Narrow, the sidebar slides over the connection instead.
        let narrow = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
            adw::BreakpointConditionLengthType::MaxWidth,
            640.0,
            adw::LengthUnit::Sp,
        ));
        narrow.add_setter(split, "collapsed", Some(&true.to_value()));
        root.add_breakpoint(narrow);

        // Fullscreen is for a session: the sidebar steps aside meanwhile.
        let sidebar_before = Cell::new(true);
        root.connect_fullscreened_notify(glib::clone!(
            #[weak]
            split,
            move |window| {
                if window.is_fullscreen() {
                    sidebar_before.set(split.shows_sidebar());
                    split.set_show_sidebar(false);
                } else {
                    split.set_show_sidebar(sidebar_before.get());
                }
            }
        ));
        actions::install(&root, &sender);
        model.rebuild_list();
        model.arm_showcase(&sender);
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match msg {
            AppMsg::NewConnection(host) => {
                let mut conn = self.settings.new_connection();
                if let Some(host) = host {
                    conn.host = host;
                }
                self.open_editor(conn, true, &sender, root);
            }
            AppMsg::Select(id) => self.select(Some(id)),
            AppMsg::Search(text) => {
                self.query = text.trim().to_lowercase();
                self.rebuild_list();
            }
            AppMsg::SearchActivated => self.search_activated(&sender),
            AppMsg::FocusSearch => {
                self.split.set_show_sidebar(true);
                self.search_bar.set_search_mode(true);
                self.search.grab_focus();
            }
            AppMsg::EditSelected => {
                if let Some(conn) = self.selected_connection().cloned() {
                    self.open_editor(conn, false, &sender, root);
                }
            }
            AppMsg::ConnectSelected => {
                if let Some(id) = self.selected.clone() {
                    self.connect(&id, &sender);
                }
            }
            AppMsg::Session(output) => self.session_output(output, &sender),
            AppMsg::CloseSession(id) => self.end_session(&id),
            AppMsg::DuplicateSelected => self.duplicate_selected(&sender),
            AppMsg::DeleteSelected => {
                if let Some(conn) = self.selected_connection() {
                    files::confirm_delete(conn, root, &sender);
                }
            }
            AppMsg::Delete(id) => {
                if let Some(gone) = self.connections.iter().find(|c| c.id == id) {
                    crate::ui::avatar::delete_picture(&gone.picture);
                }
                self.connections.retain(|c| c.id != id);
                config::save_connections(&self.connections);
                sender.oneshot_command(async move {
                    AppCmd::Keyring(secrets::delete_passwords(id).await)
                });
                self.select(None);
                self.rebuild_list();
            }
            AppMsg::Saved {
                connection,
                passwords,
            } => self.save_connection(*connection, passwords, &sender),
            AppMsg::EditorClosed => self.editor = None,
            AppMsg::ImportRdp => files::choose_rdp_files(root, &sender),
            AppMsg::ImportRdpFiles(paths) => self.import_rdp_files(paths),
            AppMsg::ImportRemmina => self.import_remmina(),
            AppMsg::ExportSelected => {
                if let Some(conn) = self.selected_connection() {
                    files::choose_export_path(conn, root, &sender);
                }
            }
            AppMsg::ExportTo(id, path) => self.export(&id, &path),
            AppMsg::OpenSettings => self.open_settings(&sender, root),
            AppMsg::Pref(change) => self.apply_pref(change),
            AppMsg::About => actions::present_about(root),
            AppMsg::Shortcuts => actions::present_shortcuts(root),
            AppMsg::ShowcaseSettingsPage(page) => self.showcase_settings_page(&page),
            AppMsg::ShowcaseSession(step) => self.showcase_session(&step),
            AppMsg::ShowcaseCapture(path) => self.showcase_capture(&path, root),
        }
    }

    fn update_cmd(&mut self, msg: Self::CommandOutput, _: ComponentSender<Self>, _: &Self::Root) {
        match msg {
            AppCmd::Keyring(Ok(())) => {}
            AppCmd::Keyring(Err(e)) => {
                tracing::warn!("keyring: {e}");
                self.toast(&i18n_f(
                    "The keyring could not be reached: {error}",
                    &[("error", &e)],
                ));
            }
        }
    }
}

impl AppModel {
    fn content_page(&self) -> &'static str {
        if self.connections.is_empty() {
            "empty"
        } else if self.selected.is_some() {
            "details"
        } else {
            "none"
        }
    }

    fn selected_connection(&self) -> Option<&Connection> {
        let id = self.selected.as_ref()?;
        self.connections.iter().find(|c| &c.id == id)
    }

    fn select(&mut self, id: Option<String>) {
        self.selected = id.filter(|id| self.connections.iter().any(|c| &c.id == id));
        self.refresh_details();
        // A connection with a session open here shows the session.
        let page = self
            .selected
            .as_deref()
            .and_then(|id| self.session_page(id))
            .unwrap_or_else(|| "browse".to_owned());
        self.panes.set_visible_child_name(&page);
        self.highlight_selected();
        // Narrow, the sidebar covers the connection; get it out of the way.
        if self.selected.is_some() && self.split.is_collapsed() {
            self.split.set_show_sidebar(false);
        }
    }

    fn refresh_details(&self) {
        match self.selected_connection() {
            Some(conn) => self.details.set_child(Some(&details::page(conn))),
            None => self.details.set_child(None::<&gtk::Widget>),
        }
    }

    /// Enter in the search field: the first match, or a new connection to
    /// what was typed when it is an address and nothing matches.
    fn search_activated(&mut self, sender: &ComponentSender<Self>) {
        if let Some(id) = sidebar::first_visible(&self.list) {
            self.select(Some(id));
            self.rebuild_list();
        } else if let Some((host, port)) = split_address(&self.search.text()) {
            let typed = match port {
                Some(port) => format!("{host}:{port}"),
                None => host,
            };
            sender.input(AppMsg::NewConnection(Some(typed)));
        }
    }

    fn open_editor(
        &mut self,
        connection: Connection,
        is_new: bool,
        sender: &ComponentSender<Self>,
        root: &adw::ApplicationWindow,
    ) {
        if self.editor.is_some() {
            return;
        }
        let editor = Editor::builder()
            .launch(EditorInit { connection, is_new })
            .forward(sender.input_sender(), |out| match out {
                EditorOutput::Saved {
                    connection,
                    passwords,
                } => AppMsg::Saved {
                    connection,
                    passwords,
                },
                EditorOutput::Closed => AppMsg::EditorClosed,
            });
        editor.widget().present(Some(root));
        self.editor = Some(editor);
    }

    fn save_connection(
        &mut self,
        connection: Connection,
        passwords: Vec<(Secret, String)>,
        sender: &ComponentSender<Self>,
    ) {
        let id = connection.id.clone();
        let title = connection.title().to_owned();
        match self.connections.iter_mut().find(|c| c.id == id) {
            Some(existing) => *existing = connection,
            None => self.connections.push(connection),
        }
        config::save_connections(&self.connections);
        for (kind, password) in passwords {
            let (id, title) = (id.clone(), title.clone());
            sender.oneshot_command(async move {
                AppCmd::Keyring(secrets::set_password(id, kind, title, password).await)
            });
        }
        self.select(Some(id));
        self.rebuild_list();
    }

    fn duplicate_selected(&mut self, sender: &ComponentSender<Self>) {
        let Some(original) = self.selected_connection() else {
            return;
        };
        let copy = Connection {
            id: crate::connection::new_id(),
            name: i18n_f("{name} (Copy)", &[("name", &original.title())]),
            last_used: 0,
            // Its own copy of the image, so deleting one keeps the other's.
            picture: crate::ui::avatar::copy_picture(&original.picture).unwrap_or_default(),
            ..original.clone()
        };
        let (from, to, title) = (original.id.clone(), copy.id.clone(), copy.name.clone());
        sender.oneshot_command(async move {
            AppCmd::Keyring(secrets::copy_passwords(from, to, title).await)
        });
        let id = copy.id.clone();
        self.connections.push(copy);
        config::save_connections(&self.connections);
        self.select(Some(id));
        self.rebuild_list();
    }

    fn open_settings(&mut self, sender: &ComponentSender<Self>, root: &adw::ApplicationWindow) {
        let prefs = self.prefs.get_or_insert_with(|| {
            Preferences::builder()
                .transient_for(root)
                .launch(self.settings.clone())
                .forward(sender.input_sender(), AppMsg::Pref)
        });
        prefs.widget().present();
    }

    fn apply_pref(&mut self, change: PrefChange) {
        let s = &mut self.settings;
        let relist = match change {
            PrefChange::ColorScheme(scheme) => {
                s.color_scheme = scheme;
                apply_color_scheme(scheme);
                false
            }
            PrefChange::Sort(order) => {
                s.sort = order;
                true
            }
            PrefChange::ShowGroups(on) => {
                s.show_groups = on;
                true
            }
            PrefChange::ShowHosts(on) => {
                s.show_hosts = on;
                true
            }
            PrefChange::DefaultPort(port) => {
                s.default_port = port;
                false
            }
            PrefChange::DefaultUsername(name) => {
                s.default_username = name;
                false
            }
            PrefChange::DefaultDomain(domain) => {
                s.default_domain = domain;
                false
            }
            PrefChange::DefaultScaling(scaling) => {
                s.default_scaling = scaling;
                false
            }
            PrefChange::DefaultQuality(quality) => {
                s.default_quality = quality;
                false
            }
            PrefChange::DefaultClipboard(on) => {
                s.default_clipboard = on;
                false
            }
            PrefChange::DefaultSound(on) => {
                s.default_sound = on;
                false
            }
            PrefChange::DefaultMicrophone(on) => {
                s.default_microphone = on;
                false
            }
            // Sessions take these when they start.
            PrefChange::KeyboardLayout(id) => {
                s.keyboard_layout = id;
                false
            }
            PrefChange::GrabKeyboard(on) => {
                s.grab_keyboard = on;
                false
            }
            PrefChange::Reconnect(on) => {
                s.reconnect = on;
                false
            }
        };
        // The window's size is saved by the window as it closes; keep what
        // is on disk rather than the size it opened at.
        s.window = config::load_settings().window;
        config::save_settings(s);
        if relist {
            self.rebuild_list();
        }
    }

    fn toast(&self, text: &str) {
        self.toasts.add_toast(adw::Toast::new(text));
    }
}

fn save_window_state(window: &adw::ApplicationWindow) {
    let mut settings = config::load_settings();
    let (width, height) = window.default_size();
    settings.window = config::WindowState {
        width,
        height,
        maximized: window.is_maximized(),
    };
    config::save_settings(&settings);
}

fn apply_color_scheme(scheme: ColorScheme) {
    adw::StyleManager::default().set_color_scheme(match scheme {
        ColorScheme::System => adw::ColorScheme::Default,
        ColorScheme::Light => adw::ColorScheme::ForceLight,
        ColorScheme::Dark => adw::ColorScheme::ForceDark,
    });
}

fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(include_str!("styles.css"));
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}
