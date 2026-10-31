//! The window's actions, the menus that name them, and their shortcuts.

use adw::prelude::*;
use gtk::gio;
use relm4::prelude::*;

use super::{AppModel, AppMsg};
use crate::i18n::i18n;
use crate::{APP_ID, APP_NAME, VERSION};

/// `win.<name>` actions and the message each sends, with their shortcuts.
const ACTIONS: &[(&str, &[&str])] = &[
    ("new-connection", &["<Control>n"]),
    ("import-rdp", &["<Control>o"]),
    ("import-remmina", &[]),
    ("edit", &["<Control>e"]),
    ("duplicate", &[]),
    ("export-rdp", &[]),
    ("delete", &[]),
    ("search", &["<Control>f"]),
    ("settings", &["<Control>comma"]),
    ("about", &[]),
];

fn message(action: &str) -> AppMsg {
    match action {
        "new-connection" => AppMsg::NewConnection(None),
        "import-rdp" => AppMsg::ImportRdp,
        "import-remmina" => AppMsg::ImportRemmina,
        "edit" => AppMsg::EditSelected,
        "duplicate" => AppMsg::DuplicateSelected,
        "export-rdp" => AppMsg::ExportSelected,
        "delete" => AppMsg::DeleteSelected,
        "search" => AppMsg::FocusSearch,
        "settings" => AppMsg::OpenSettings,
        _ => AppMsg::About,
    }
}

pub(super) fn install(window: &adw::ApplicationWindow, sender: &ComponentSender<AppModel>) {
    let app = relm4::main_adw_application();
    for (name, accels) in ACTIONS {
        let input = sender.input_sender().clone();
        let action = gio::ActionEntry::builder(name)
            .activate(move |_: &adw::ApplicationWindow, _, _| input.emit(message(name)))
            .build();
        window.add_action_entries([action]);
        app.set_accels_for_action(&format!("win.{name}"), accels);
    }

    let quit = gio::ActionEntry::builder("quit")
        .activate(|app: &adw::Application, _, _| {
            // Closing the windows, not app.quit(): the main window saves
            // its size as it closes.
            for window in app.windows() {
                window.close();
            }
        })
        .build();
    app.add_action_entries([quit]);
    app.set_accels_for_action("app.quit", &["<Control>q"]);
    app.set_accels_for_action("window.close", &["<Control>w"]);
}

/// The burger menu above the sidebar, in Hylki's order: Settings first,
/// then what the app does, then About, and Quit on its own.
pub(super) fn main_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    let settings = gio::Menu::new();
    settings.append(Some(&i18n("Settings")), Some("win.settings"));
    menu.append_section(None, &settings);
    menu.append_section(None, &import_menu());
    let help = gio::Menu::new();
    help.append(Some(&i18n("About Mote")), Some("win.about"));
    menu.append_section(None, &help);
    let quit = gio::Menu::new();
    quit.append(Some(&i18n("Quit")), Some("app.quit"));
    menu.append_section(None, &quit);
    menu
}

/// The imports, for the main menu and New Connection's caret.
pub(super) fn import_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    menu.append(Some(&i18n("Import .rdp Files…")), Some("win.import-rdp"));
    menu.append(
        Some(&i18n("Import from Remmina")),
        Some("win.import-remmina"),
    );
    menu
}

/// The menu at the end of a connection's header.
pub(super) fn connection_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    let items = gio::Menu::new();
    items.append(Some(&i18n("Duplicate")), Some("win.duplicate"));
    items.append(Some(&i18n("Export as .rdp File…")), Some("win.export-rdp"));
    menu.append_section(None, &items);
    let delete = gio::Menu::new();
    delete.append(Some(&i18n("Delete…")), Some("win.delete"));
    menu.append_section(None, &delete);
    menu
}

pub(super) fn present_about(parent: &adw::ApplicationWindow) {
    adw::AboutDialog::builder()
        .application_name(APP_NAME)
        .application_icon(APP_ID)
        .developer_name("Hyprlab")
        .version(VERSION)
        .license_type(gtk::License::Agpl30)
        .website("https://github.com/hyprlab/mote")
        .issue_url("https://github.com/hyprlab/mote/issues")
        .build()
        .present(Some(parent));
}
