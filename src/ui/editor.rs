//! The connection editor: a dialog for adding a connection or changing one.
//! Later sections add their settings to it as pages of their own.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use relm4::prelude::*;

use crate::connection::{Connection, Quality, Scaling, split_address};
use crate::i18n::i18n;
use crate::secrets;

pub(crate) struct EditorInit {
    pub(crate) connection: Connection,
    pub(crate) is_new: bool,
}

pub(crate) struct Editor {
    base: Connection,
    valid: bool,
    rows: Rows,
    /// Set by the user typing, never by the keyring filling the row in: an
    /// untouched password is not written back.
    password_edited: bool,
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
    scaling: adw::ComboRow,
    quality: adw::ComboRow,
}

#[derive(Debug)]
pub(crate) enum EditorMsg {
    HostChanged,
    PasswordEdited,
    Save,
    Cancel,
}

#[derive(Debug)]
pub(crate) enum EditorOutput {
    /// `password` is `None` when it wasn't touched, and empty to forget it.
    Saved {
        connection: Connection,
        password: Option<String>,
    },
    Closed,
}

#[derive(Debug)]
pub(crate) enum EditorCmd {
    PasswordLoaded(Result<Option<String>, String>),
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
            password: adw::PasswordEntryRow::new(),
            scaling: adw::ComboRow::new(),
            quality: adw::ComboRow::new(),
        };
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
        rows.port.set_title(&i18n("Port"));
        rows.port.set_value(f64::from(c.port));
        rows.password.set_title(&i18n("Password"));
        rows.password.set_activates_default(true);

        let input = sender.input_sender().clone();
        rows.host
            .connect_changed(move |_| input.emit(EditorMsg::HostChanged));
        let filling = Rc::new(Cell::new(false));
        let (input, quiet) = (sender.input_sender().clone(), filling.clone());
        rows.password.connect_changed(move |_| {
            if !quiet.get() {
                input.emit(EditorMsg::PasswordEdited);
            }
        });

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
        page.add(&display);

        if !init.is_new {
            let id = c.id.clone();
            sender.oneshot_command(async move {
                EditorCmd::PasswordLoaded(secrets::password(id).await)
            });
        }

        let model = Editor {
            valid: split_address(&c.host).is_some(),
            base: init.connection,
            rows,
            password_edited: false,
            filling,
        };
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
            EditorMsg::HostChanged => {
                self.valid = split_address(&self.rows.host.text()).is_some();
            }
            EditorMsg::PasswordEdited => self.password_edited = true,
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
                    ..self.base.clone()
                }
                .normalized();
                let password = self.password_edited.then(|| r.password.text().to_string());
                let _ = sender.output(EditorOutput::Saved {
                    connection,
                    password,
                });
                root.close();
            }
        }
    }

    fn update_cmd(&mut self, msg: Self::CommandOutput, _: ComponentSender<Self>, _: &Self::Root) {
        match msg {
            EditorCmd::PasswordLoaded(Ok(Some(password))) if !self.password_edited => {
                self.filling.set(true);
                self.rows.password.set_text(&password);
                self.filling.set(false);
            }
            EditorCmd::PasswordLoaded(Err(e)) => {
                tracing::warn!("reading the saved password: {e}");
            }
            EditorCmd::PasswordLoaded(_) => {}
        }
    }
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
