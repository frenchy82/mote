//! The connection editor: a dialog for adding a connection or changing one.
//! Later sections add their settings to it as pages of their own.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use relm4::prelude::*;

use crate::connection::{Connection, Network, Quality, Scaling, parse_mac, split_address};
use crate::i18n::i18n;
use crate::secrets::{self, Secret};

pub(crate) struct EditorInit {
    pub(crate) connection: Connection,
    pub(crate) is_new: bool,
}

pub(crate) struct Editor {
    base: Connection,
    valid: bool,
    rows: Rows,
    /// The passwords the user typed in, never the keyring filling a row
    /// in: an untouched password is not written back.
    edited: Vec<Secret>,
    filling: Rc<Cell<bool>>,
}

struct Rows {
    name: adw::EntryRow,
    host: adw::EntryRow,
    port: adw::SpinRow,
    group: adw::EntryRow,
    username: adw::EntryRow,
    domain: adw::EntryRow,
    password: adw::PasswordEntryRow,
    mac: adw::EntryRow,
    ssh: adw::ExpanderRow,
    ssh_host: adw::EntryRow,
    ssh_username: adw::EntryRow,
    ssh_password: adw::PasswordEntryRow,
    gateway: adw::ExpanderRow,
    gateway_host: adw::EntryRow,
    gateway_username: adw::EntryRow,
    gateway_password: adw::PasswordEntryRow,
    scaling: adw::ComboRow,
    quality: adw::ComboRow,
    view_only: adw::SwitchRow,
    network: adw::ComboRow,
    clipboard: adw::SwitchRow,
    sound: adw::SwitchRow,
    microphone: adw::SwitchRow,
    folder: adw::ActionRow,
    command_before: adw::EntryRow,
    command_after: adw::EntryRow,
    /// The folder chosen in `folder`, empty for none.
    folder_path: Rc<RefCell<String>>,
}

#[derive(Debug)]
pub(crate) enum EditorMsg {
    /// Something that decides whether the connection can be saved.
    Changed,
    PasswordEdited(Secret),
    Save,
    Cancel,
}

#[derive(Debug)]
pub(crate) enum EditorOutput {
    /// `passwords` holds the ones changed; an empty one is forgotten.
    Saved {
        connection: Box<Connection>,
        passwords: Vec<(Secret, String)>,
    },
    Closed,
}

#[derive(Debug)]
pub(crate) enum EditorCmd {
    PasswordLoaded(Secret, Result<Option<String>, String>),
}

#[relm4::component(pub(crate))]
impl Component for Editor {
    type Init = EditorInit;
    type Input = EditorMsg;
    type Output = EditorOutput;
    type CommandOutput = EditorCmd;

    view! {
        adw::Dialog {
            set_title: &title,
            set_content_width: 520,
            connect_closed[sender] => move |_| {
                let _ = sender.output(EditorOutput::Closed);
            },

            #[wrap(Some)]
            set_child = &adw::ToolbarView {
                add_top_bar = &adw::HeaderBar {
                    set_show_start_title_buttons: false,
                    set_show_end_title_buttons: false,
                    pack_start = &gtk::Button {
                        set_label: &i18n("Cancel"),
                        connect_clicked => EditorMsg::Cancel,
                    },
                    #[name = "save"]
                    pack_end = &gtk::Button {
                        set_label: &save_label,
                        add_css_class: "suggested-action",
                        #[watch]
                        set_sensitive: model.valid,
                        connect_clicked => EditorMsg::Save,
                    },
                },
                set_content: Some(&page),
            },
        }
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let c = &init.connection;
        let (title, save_label) = if init.is_new {
            (i18n("New Connection"), i18n("Add"))
        } else {
            (i18n("Edit Connection"), i18n("Save"))
        };
        let rows = Rows {
            name: entry(&i18n("Name"), &c.name),
            host: entry(&i18n("Address"), &c.host),
            port: adw::SpinRow::with_range(1.0, 65535.0, 1.0),
            group: entry(&i18n("Group"), &c.group),
            username: entry(&i18n("User Name"), &c.username),
            domain: entry(&i18n("Domain"), &c.domain),
            password: password_entry(&i18n("Password")),
            mac: entry(&i18n("Wake-on-LAN MAC Address"), &c.mac_address),
            ssh: adw::ExpanderRow::new(),
            ssh_host: entry(&i18n("Address"), &c.ssh_host),
            ssh_username: entry(&i18n("User Name"), &c.ssh_username),
            ssh_password: password_entry(&i18n("Password")),
            gateway: adw::ExpanderRow::new(),
            gateway_host: entry(&i18n("Address"), &c.gateway_host),
            gateway_username: entry(&i18n("User Name"), &c.gateway_username),
            gateway_password: password_entry(&i18n("Password")),
            scaling: adw::ComboRow::new(),
            quality: adw::ComboRow::new(),
            view_only: adw::SwitchRow::new(),
            network: adw::ComboRow::new(),
            clipboard: adw::SwitchRow::new(),
            sound: adw::SwitchRow::new(),
            microphone: adw::SwitchRow::new(),
            folder: adw::ActionRow::new(),
            command_before: entry(&i18n("Before Connecting"), &c.command_before),
            command_after: entry(&i18n("After Disconnecting"), &c.command_after),
            folder_path: Rc::new(RefCell::new(c.folder.clone())),
        };
        folder_row(&rows.folder, &rows.folder_path);
        let labels: Vec<String> = Scaling::ALL
            .into_iter()
            .map(crate::ui::session::scaling_label)
            .collect();
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        rows.scaling.set_title(&i18n("Scaling"));
        rows.scaling.set_model(Some(&gtk::StringList::new(&labels)));
        rows.scaling.set_selected(
            Scaling::ALL
                .iter()
                .position(|s| *s == c.scaling)
                .unwrap_or_default() as u32,
        );
        let labels: Vec<String> = Quality::ALL.into_iter().map(quality_label).collect();
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        rows.quality.set_title(&i18n("Codec"));
        rows.quality.set_model(Some(&gtk::StringList::new(&labels)));
        rows.quality.connect_selected_notify(|row| {
            let quality = Quality::ALL.get(row.selected() as usize).copied();
            row.set_subtitle(&quality.map(quality_cost).unwrap_or_default());
        });
        rows.quality.set_selected(
            Quality::ALL
                .iter()
                .position(|q| *q == c.quality)
                .unwrap_or_default() as u32,
        );
        rows.quality.notify("selected");
        let labels: Vec<String> = Network::ALL.into_iter().map(network_label).collect();
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        rows.network.set_title(&i18n("Connection Type"));
        rows.network.set_subtitle(&i18n("Only xrdp acts on it"));
        rows.network.set_model(Some(&gtk::StringList::new(&labels)));
        rows.network.set_selected(
            Network::ALL
                .iter()
                .position(|n| *n == c.network)
                .unwrap_or_default() as u32,
        );
        rows.view_only.set_title(&i18n("View Only"));
        rows.view_only.set_active(c.view_only);
        rows.clipboard.set_title(&i18n("Clipboard"));
        rows.clipboard
            .set_subtitle(&i18n("Text only with KRdp; xrdp leaves out folders"));
        rows.clipboard.set_active(c.clipboard);
        // Both depend on the server the same way.
        let audio_limit = i18n("Not with KRdp; xrdp needs its PipeWire module");
        rows.sound.set_title(&i18n("Sound"));
        rows.sound.set_subtitle(&audio_limit);
        rows.sound.set_active(c.sound);
        rows.microphone.set_title(&i18n("Microphone"));
        rows.microphone.set_subtitle(&audio_limit);
        rows.microphone.set_active(c.microphone);
        rows.port.set_title(&i18n("Port"));
        rows.port.set_value(f64::from(c.port));
        rows.ssh.set_title(&i18n("SSH Tunnel"));
        rows.ssh
            .set_subtitle(&i18n("Tries the SSH agent’s keys before the password"));
        rows.ssh.set_show_enable_switch(true);
        rows.ssh.set_enable_expansion(c.ssh_tunnel);
        rows.ssh.set_expanded(false);
        for row in [
            rows.ssh_host.upcast_ref::<gtk::Widget>(),
            rows.ssh_username.upcast_ref(),
            rows.ssh_password.upcast_ref(),
        ] {
            rows.ssh.add_row(row);
        }
        rows.gateway.set_title(&i18n("RD Gateway"));
        rows.gateway.set_subtitle(&i18n(
            "Without an account here, signs in with the one above",
        ));
        rows.gateway.set_show_enable_switch(true);
        rows.gateway.set_enable_expansion(c.gateway);
        rows.gateway.set_expanded(false);
        for row in [
            rows.gateway_host.upcast_ref::<gtk::Widget>(),
            rows.gateway_username.upcast_ref(),
            rows.gateway_password.upcast_ref(),
        ] {
            rows.gateway.add_row(row);
        }
        // One or the other: a gateway can't be reached through the tunnel.
        let other = rows.gateway.clone();
        let username = rows.ssh_username.clone();
        rows.ssh.connect_enable_expansion_notify(move |ssh| {
            if ssh.enables_expansion() {
                other.set_enable_expansion(false);
                if username.text().trim().is_empty() {
                    username.set_text(&gtk::glib::user_name().to_string_lossy());
                }
            }
        });
        let other = rows.ssh.clone();
        rows.gateway
            .connect_enable_expansion_notify(move |gateway| {
                if gateway.enables_expansion() {
                    other.set_enable_expansion(false);
                }
            });

        for row in [&rows.host, &rows.ssh_host, &rows.gateway_host, &rows.mac] {
            let input = sender.input_sender().clone();
            row.connect_changed(move |_| input.emit(EditorMsg::Changed));
        }
        for row in [&rows.ssh, &rows.gateway] {
            let input = sender.input_sender().clone();
            row.connect_enable_expansion_notify(move |_| input.emit(EditorMsg::Changed));
        }
        let filling = Rc::new(Cell::new(false));
        for (row, kind) in [
            (&rows.password, Secret::Account),
            (&rows.ssh_password, Secret::Ssh),
            (&rows.gateway_password, Secret::Gateway),
        ] {
            let (input, quiet) = (sender.input_sender().clone(), filling.clone());
            row.connect_changed(move |_| {
                if !quiet.get() {
                    input.emit(EditorMsg::PasswordEdited(kind));
                }
            });
        }

        let page = adw::PreferencesPage::new();
        let target = adw::PreferencesGroup::new();
        for row in [
            rows.name.upcast_ref::<gtk::Widget>(),
            rows.host.upcast_ref(),
            rows.port.upcast_ref(),
            rows.group.upcast_ref(),
        ] {
            target.add(row);
        }
        page.add(&target);
        let account = adw::PreferencesGroup::new();
        account.set_title(&i18n("Account"));
        account.set_description(Some(&i18n(
            "Leave the password empty to be asked for it when connecting.",
        )));
        for row in [
            rows.username.upcast_ref::<gtk::Widget>(),
            rows.password.upcast_ref(),
            rows.domain.upcast_ref(),
        ] {
            account.add(row);
        }
        page.add(&account);
        let display = adw::PreferencesGroup::new();
        display.set_title(&i18n("Display"));
        display.add(&rows.scaling);
        display.add(&rows.quality);
        display.add(&rows.view_only);
        page.add(&display);
        let network = adw::PreferencesGroup::new();
        network.set_title(&i18n("Network"));
        network.add(&rows.network);
        network.add(&rows.mac);
        network.add(&rows.ssh);
        network.add(&rows.gateway);
        page.add(&network);
        let sharing = adw::PreferencesGroup::new();
        sharing.set_title(&i18n("Sharing"));
        sharing.add(&rows.clipboard);
        sharing.add(&rows.sound);
        sharing.add(&rows.microphone);
        sharing.add(&rows.folder);
        page.add(&sharing);
        let commands = adw::PreferencesGroup::new();
        commands.set_title(&i18n("Commands"));
        commands.set_description(Some(&i18n(
            "Run by sh, with the connection in $MOTE_HOST, $MOTE_PORT, $MOTE_USER and $MOTE_NAME. A failed command before connecting stops the connection.",
        )));
        commands.add(&rows.command_before);
        commands.add(&rows.command_after);
        page.add(&commands);

        if !init.is_new {
            for kind in Secret::ALL {
                let id = c.id.clone();
                sender.oneshot_command(async move {
                    EditorCmd::PasswordLoaded(kind, secrets::password(id, kind).await)
                });
            }
        }

        let mut model = Editor {
            valid: false,
            base: init.connection,
            rows,
            edited: Vec::new(),
            filling,
        };
        model.check();
        let widgets = view_output!();
        root.set_default_widget(Some(&widgets.save));
        if init.is_new && !model.base.host.is_empty() {
            model.rows.name.grab_focus();
        } else {
            model.rows.host.grab_focus();
        }
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match msg {
            EditorMsg::Changed => self.check(),
            EditorMsg::PasswordEdited(kind) => {
                if !self.edited.contains(&kind) {
                    self.edited.push(kind);
                }
            }
            EditorMsg::Cancel => {
                root.close();
            }
            EditorMsg::Save => {
                if !self.valid {
                    return;
                }
                let r = &self.rows;
                // A port typed but not yet committed by leaving the field.
                r.port.update();
                let connection = Connection {
                    name: r.name.text().into(),
                    host: r.host.text().into(),
                    port: r.port.value() as u16,
                    group: r.group.text().into(),
                    username: r.username.text().into(),
                    domain: r.domain.text().into(),
                    scaling: Scaling::ALL
                        .get(r.scaling.selected() as usize)
                        .copied()
                        .unwrap_or_default(),
                    quality: Quality::ALL
                        .get(r.quality.selected() as usize)
                        .copied()
                        .unwrap_or_default(),
                    network: Network::ALL
                        .get(r.network.selected() as usize)
                        .copied()
                        .unwrap_or_default(),
                    view_only: r.view_only.is_active(),
                    command_before: r.command_before.text().into(),
                    command_after: r.command_after.text().into(),
                    clipboard: r.clipboard.is_active(),
                    sound: r.sound.is_active(),
                    microphone: r.microphone.is_active(),
                    folder: r.folder_path.borrow().clone(),
                    ssh_tunnel: r.ssh.enables_expansion(),
                    ssh_host: r.ssh_host.text().into(),
                    ssh_username: r.ssh_username.text().into(),
                    gateway: r.gateway.enables_expansion(),
                    gateway_host: r.gateway_host.text().into(),
                    gateway_username: r.gateway_username.text().into(),
                    mac_address: r.mac.text().into(),
                    ..self.base.clone()
                }
                .normalized();
                let passwords = self
                    .edited
                    .iter()
                    .map(|&kind| (kind, self.password_row(kind).text().to_string()))
                    .collect();
                let _ = sender.output(EditorOutput::Saved {
                    connection: Box::new(connection),
                    passwords,
                });
                root.close();
            }
        }
    }

    fn update_cmd(&mut self, msg: Self::CommandOutput, _: ComponentSender<Self>, _: &Self::Root) {
        match msg {
            EditorCmd::PasswordLoaded(kind, Ok(Some(password))) if !self.edited.contains(&kind) => {
                self.filling.set(true);
                self.password_row(kind).set_text(&password);
                self.filling.set(false);
            }
            EditorCmd::PasswordLoaded(_, Err(e)) => {
                tracing::warn!("reading the saved password: {e}");
            }
            EditorCmd::PasswordLoaded(..) => {}
        }
    }
}

impl Editor {
    fn password_row(&self, kind: Secret) -> &adw::PasswordEntryRow {
        match kind {
            Secret::Account => &self.rows.password,
            Secret::Ssh => &self.rows.ssh_password,
            Secret::Gateway => &self.rows.gateway_password,
        }
    }

    /// Whether what the rows hold can be saved, marking the rows that
    /// can't be.
    fn check(&mut self) {
        let r = &self.rows;
        let address = |row: &adw::EntryRow| split_address(&row.text()).is_some();
        let mac = r.mac.text();
        let checks = [
            (&r.host, address(&r.host) || r.host.text().trim().is_empty()),
            (
                &r.ssh_host,
                !r.ssh.enables_expansion() || address(&r.ssh_host),
            ),
            (
                &r.gateway_host,
                !r.gateway.enables_expansion() || address(&r.gateway_host),
            ),
            (&r.mac, mac.trim().is_empty() || parse_mac(&mac).is_some()),
        ];
        let mut valid = address(&r.host);
        for (row, ok) in checks {
            // An empty row is still being filled in, not wrong.
            if ok || row.text().trim().is_empty() {
                row.remove_css_class("error");
            } else {
                row.add_css_class("error");
            }
            valid &= ok;
        }
        self.valid = valid;
    }
}

fn password_entry(title: &str) -> adw::PasswordEntryRow {
    let row = adw::PasswordEntryRow::new();
    row.set_title(title);
    row.set_activates_default(true);
    row
}

fn entry(title: &str, text: &str) -> adw::EntryRow {
    let row = adw::EntryRow::new();
    row.set_title(title);
    row.set_text(text);
    row.set_activates_default(true);
    row
}

fn quality_label(quality: Quality) -> String {
    match quality {
        Quality::Best => i18n("Automatic"),
        Quality::Avc420 => i18n("H.264 (AVC420)"),
        Quality::RemoteFx => i18n("RemoteFX"),
    }
}

/// What choosing it costs, for the row's subtitle.
fn quality_cost(quality: Quality) -> String {
    match quality {
        Quality::Best => String::new(),
        Quality::Avc420 => i18n("Half the color detail: colored text is softer"),
        Quality::RemoteFx => i18n("About twice the bandwidth; KRdp turns it down"),
    }
}

fn network_label(network: Network) -> String {
    match network {
        Network::Lan => i18n("Local Network"),
        Network::Wan => i18n("Wide Area Network"),
        Network::BroadbandHigh => i18n("Fast Broadband"),
        Network::Satellite => i18n("Satellite"),
        Network::BroadbandLow => i18n("Slow Broadband"),
        Network::Modem => i18n("Modem"),
    }
}

/// The shared folder's row: the folder, a button to choose one and one to
/// stop sharing it.
fn folder_row(row: &adw::ActionRow, path: &Rc<RefCell<String>>) {
    row.set_title(&i18n("Shared Folder"));
    let choose = gtk::Button::builder()
        .icon_name("folder-open-symbolic")
        .tooltip_text(i18n("Choose Folder"))
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    let clear = gtk::Button::builder()
        .icon_name("edit-clear-symbolic")
        .tooltip_text(i18n("Stop Sharing"))
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    row.add_suffix(&clear);
    row.add_suffix(&choose);
    row.set_activatable_widget(Some(&choose));

    let show = {
        let (row, clear) = (row.clone(), clear.clone());
        move |folder: &str| {
            clear.set_visible(!folder.is_empty());
            if folder.is_empty() {
                row.set_subtitle(&i18n("Only xrdp opens a shared folder"));
            } else {
                row.set_subtitle(&gtk::glib::markup_escape_text(&shown_path(folder)));
            }
        }
    };
    show(&path.borrow());
    let show = Rc::new(show);

    clear.connect_clicked({
        let (path, show) = (path.clone(), show.clone());
        move |_| {
            path.borrow_mut().clear();
            show("");
        }
    });
    choose.connect_clicked({
        let (path, show) = (path.clone(), show.clone());
        move |button| {
            let dialog = gtk::FileDialog::builder()
                .title(i18n("Shared Folder"))
                .modal(true)
                .build();
            let current = path.borrow().clone();
            if !current.is_empty() {
                dialog.set_initial_folder(Some(&gtk::gio::File::for_path(&current)));
            }
            let window = button.root().and_downcast::<gtk::Window>();
            let (path, show) = (path.clone(), show.clone());
            dialog.select_folder(
                window.as_ref(),
                None::<&gtk::gio::Cancellable>,
                move |chosen| {
                    let Some(folder) = chosen.ok().and_then(|f| f.path()) else {
                        return;
                    };
                    let folder = folder.to_string_lossy().into_owned();
                    show(&folder);
                    *path.borrow_mut() = folder;
                },
            );
        }
    });
}

/// A path as people read it, with the home folder as ~.
fn shown_path(path: &str) -> String {
    let home = gtk::glib::home_dir();
    match std::path::Path::new(path).strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.to_owned(),
    }
}
