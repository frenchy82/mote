//! The Scan Network dialog: the RDP servers on the local networks, as they
//! answer, each a button away from being a connection.

use adw::prelude::*;
use relm4::prelude::*;

use crate::connection::DEFAULT_PORT;
use crate::i18n::{i18n, i18n_f};
use crate::rdp::server::Server;
use crate::scan::{self, Event, Found, Network};

pub(crate) struct Scan {
    /// The host and port of each connection there is.
    saved: Vec<(String, u16)>,
    networks: Vec<Network>,
    /// What answered, in order of address, beside its row in `list`.
    entries: Vec<Entry>,
    list: gtk::ListBox,
    scanning: bool,
    progress: f64,
}

struct Entry {
    found: Found,
    row: adw::ActionRow,
    add: gtk::Button,
    added: gtk::Label,
}

#[derive(Debug)]
pub(crate) enum ScanMsg {
    Again,
    Add(std::net::SocketAddrV4),
}

#[derive(Debug)]
pub(crate) enum ScanOutput {
    Add(Box<Found>),
    Closed,
}

#[relm4::component(pub(crate))]
impl Component for Scan {
    /// The host and port of each connection there is.
    type Init = Vec<(String, u16)>;
    type Input = ScanMsg;
    type Output = ScanOutput;
    type CommandOutput = Event;

    view! {
        adw::Dialog {
            set_title: &i18n("Scan Network"),
            set_content_width: 460,
            set_content_height: 520,
            connect_closed[sender] => move |_| {
                let _ = sender.output(ScanOutput::Closed);
            },

            #[wrap(Some)]
            set_child = &adw::ToolbarView {
                add_top_bar = &adw::HeaderBar {
                    pack_start = &gtk::Button {
                        set_icon_name: "view-refresh-symbolic",
                        set_tooltip_text: Some(&i18n("Scan Again")),
                        update_property: &[gtk::accessible::Property::Label(&i18n("Scan Again"))],
                        #[watch]
                        set_sensitive: !model.scanning,
                        connect_clicked => ScanMsg::Again,
                    },
                },
                add_top_bar = &gtk::ProgressBar {
                    add_css_class: "osd",
                    #[watch]
                    set_visible: model.scanning,
                    #[watch]
                    set_fraction: model.progress,
                },

                #[wrap(Some)]
                set_content = &gtk::Stack {
                    add_named[Some("scanning")] = &adw::StatusPage {
                        set_paintable: Some(&adw::SpinnerPaintable::new(Some(&root))),
                        set_title: &i18n("Scanning…"),
                        #[watch]
                        set_description: Some(&model.ranges()),
                    },
                    add_named[Some("none")] = &adw::StatusPage {
                        set_icon_name: Some("network-wired-symbolic"),
                        set_title: &i18n("No Computers Found"),
                        #[watch]
                        set_description: Some(&i18n_f(
                            "Nothing on {networks} answered on port {first} or {second}.",
                            &[
                                ("networks", &model.ranges()),
                                ("first", &scan::PORTS[0]),
                                ("second", &scan::PORTS[1]),
                            ],
                        )),
                    },
                    add_named[Some("no-network")] = &adw::StatusPage {
                        set_icon_name: Some("network-offline-symbolic"),
                        set_title: &i18n("No Local Network"),
                        set_description: Some(&i18n("This computer has no address on a private network.")),
                    },
                    add_named[Some("found")] = &adw::PreferencesPage {
                        add = &adw::PreferencesGroup {
                            set_title: &i18n("Computers"),
                            #[watch]
                            set_description: Some(&model.ranges()),
                            #[local_ref]
                            add = list -> gtk::ListBox {
                                add_css_class: "boxed-list",
                                set_selection_mode: gtk::SelectionMode::None,
                            },
                        },
                    },
                    // After the pages: naming one before it exists only
                    // earns a warning.
                    #[watch]
                    set_visible_child_name: model.page(),
                },
            },
        }
    }

    fn init(
        saved: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let mut model = Scan {
            saved,
            networks: Vec::new(),
            entries: Vec::new(),
            list: gtk::ListBox::new(),
            scanning: false,
            progress: 0.0,
        };
        let list = &model.list.clone();
        let widgets = view_output!();
        model.start(&sender);
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, _: &Self::Root) {
        match msg {
            ScanMsg::Again => {
                for entry in self.entries.drain(..) {
                    self.list.remove(&entry.row);
                }
                self.start(&sender);
            }
            ScanMsg::Add(address) => {
                let Some(entry) = self.entries.iter_mut().find(|e| e.found.address == address)
                else {
                    return;
                };
                self.saved
                    .push((entry.found.host.clone(), entry.found.address.port()));
                entry.found.saved = true;
                entry.show_added();
                let _ = sender.output(ScanOutput::Add(Box::new(entry.found.clone())));
            }
        }
    }

    fn update_cmd(
        &mut self,
        event: Self::CommandOutput,
        sender: ComponentSender<Self>,
        _: &Self::Root,
    ) {
        match event {
            Event::Networks(networks) => {
                self.scanning = !networks.is_empty();
                self.networks = networks;
            }
            Event::Progress(progress) => self.progress = progress,
            Event::Found(found) => self.show(*found, &sender),
            Event::Done => self.scanning = false,
        }
    }
}

impl Scan {
    fn start(&mut self, sender: &ComponentSender<Self>) {
        self.scanning = true;
        self.progress = 0.0;
        let saved = self.saved.clone();
        sender.command(move |out, shutdown| {
            shutdown
                .register(async move {
                    scan::run(saved, |event| {
                        let _ = out.send(event);
                    })
                    .await;
                })
                .drop_on_shutdown()
        });
    }

    fn page(&self) -> &'static str {
        if !self.entries.is_empty() {
            "found"
        } else if self.scanning {
            "scanning"
        } else if self.networks.is_empty() {
            "no-network"
        } else {
            "none"
        }
    }

    /// The ranges being scanned, for the page to say how far it looks.
    fn ranges(&self) -> String {
        let ranges: Vec<String> = self.networks.iter().map(Network::to_string).collect();
        ranges.join(", ")
    }

    /// Add a row in its place by address, so that the list doesn't depend
    /// on who answers first.
    fn show(&mut self, found: Found, sender: &ComponentSender<Self>) {
        let at = self
            .entries
            .partition_point(|e| e.found.address < found.address);
        let entry = Entry::new(found, sender);
        self.list.insert(&entry.row, at as i32);
        self.entries.insert(at, entry);
    }
}

impl Entry {
    fn new(found: Found, sender: &ComponentSender<Scan>) -> Self {
        let title = title(&found);
        let row = adw::ActionRow::new();
        // Names from the network, so they aren't to be taken for markup.
        row.set_use_markup(false);
        row.set_title(&title);
        row.set_subtitle(&subtitle(&found));

        let add = gtk::Button::with_label(&i18n("Add"));
        add.set_valign(gtk::Align::Center);
        add.update_property(&[gtk::accessible::Property::Label(&i18n_f(
            "Add {name}",
            &[("name", &title)],
        ))]);
        let (input, address) = (sender.input_sender().clone(), found.address);
        add.connect_clicked(move |_| input.emit(ScanMsg::Add(address)));
        row.add_suffix(&add);

        let added = gtk::Label::new(Some(&i18n("Added")));
        added.add_css_class("dim-label");
        row.add_suffix(&added);

        let entry = Self {
            found,
            row,
            add,
            added,
        };
        if entry.found.saved {
            entry.show_added();
        } else {
            entry.added.set_visible(false);
            entry.row.set_activatable_widget(Some(&entry.add));
        }
        entry
    }

    fn show_added(&self) {
        let had_focus = self.add.has_focus();
        self.add.set_visible(false);
        self.added.set_visible(true);
        self.row.set_activatable_widget(gtk::Widget::NONE);
        self.row.set_activatable(false);
        // The button is gone; the keyboard stays where it was in the list.
        if had_focus {
            self.row.grab_focus();
        }
    }
}

fn title(found: &Found) -> String {
    if found.name.is_empty() {
        found.host.clone()
    } else {
        found.name.clone()
    }
}

/// The address, unless the title is that already, and the server if its
/// certificate says which.
fn subtitle(found: &Found) -> String {
    let address = found.address;
    let at = if address.port() == DEFAULT_PORT {
        address.ip().to_string()
    } else {
        address.to_string()
    };
    let parts = [
        (title(found) != at).then_some(at),
        found.server.map(server_name),
    ];
    let parts: Vec<String> = parts.into_iter().flatten().collect();
    parts.join(" · ")
}

/// A certificate tells GNOME's server from the others, but not its Desktop
/// Sharing from its Remote Login: both make it the same way.
fn server_name(server: Server) -> String {
    match server {
        Server::GnomeDesktop | Server::GnomeLogin => i18n("GNOME Remote Desktop"),
        Server::Krdp => "KRdp".to_owned(),
        Server::Xrdp => "xrdp".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(address: &str, host: &str, name: &str, server: Option<Server>) -> Found {
        Found {
            address: address.parse().unwrap(),
            host: host.into(),
            name: name.into(),
            server,
            saved: false,
        }
    }

    #[test]
    fn rows_say_the_address_once_and_the_port_when_unusual() {
        let by_name = found("10.0.1.33:3389", "vm.lan", "", Some(Server::Xrdp));
        assert_eq!(title(&by_name), "vm.lan");
        assert_eq!(subtitle(&by_name), "10.0.1.33 · xrdp");

        let bare = found("10.0.1.33:3389", "10.0.1.33", "", None);
        assert_eq!(title(&bare), "10.0.1.33");
        assert_eq!(subtitle(&bare), "");

        let other_port = found("10.0.1.33:3390", "10.0.1.33", "", Some(Server::Krdp));
        assert_eq!(subtitle(&other_port), "10.0.1.33:3390 · KRdp");

        let named = found("10.0.1.33:3389", "10.0.1.33", "build01", None);
        assert_eq!(title(&named), "build01");
        assert_eq!(subtitle(&named), "10.0.1.33");
    }
}
