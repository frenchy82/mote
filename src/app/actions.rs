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
    ("connect", &["<Control>Return"]),
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
        "connect" => AppMsg::ConnectSelected,
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
    // The shortcuts belong to this window and run after the focused widget
    // has had the key, never as the application's: GTK tries those first,
    // so they would take keys from a session's desktop, which keeps every
    // key it gets.
    let shortcuts = gtk::ShortcutController::new();
    let add = |accel: &str, action: &str| {
        shortcuts.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string(accel),
            Some(gtk::NamedAction::new(action)),
        ));
    };
    for (name, accels) in ACTIONS {
        let input = sender.input_sender().clone();
        let action = gio::ActionEntry::builder(name)
            .activate(move |_: &adw::ApplicationWindow, _, _| input.emit(message(name)))
            .build();
        window.add_action_entries([action]);
        for accel in *accels {
            add(accel, &format!("win.{name}"));
        }
    }
    add("<Control>q", "app.quit");
    add("<Control>w", "window.close");
    window.add_controller(shortcuts);

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
}

/// A menu item with its shortcut shown beside it. The menu can't find the
/// shortcut itself, since it isn't the application's.
fn item(label: &str, action: &str, accel: Option<&str>) -> gio::MenuItem {
    let item = gio::MenuItem::new(Some(label), Some(action));
    if let Some(accel) = accel {
        item.set_attribute_value("accel", Some(&accel.to_variant()));
    }
    item
}

/// The burger menu above the sidebar, in Hylki's order: Settings first,
/// then what the app does, then About, and Quit on its own.
pub(super) fn main_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    let settings = gio::Menu::new();
    settings.append_item(&item(
        &i18n("Settings"),
        "win.settings",
        Some("<Control>comma"),
    ));
    menu.append_section(None, &settings);
    menu.append_section(None, &import_menu());
    let help = gio::Menu::new();
    help.append(Some(&i18n("About Mote")), Some("win.about"));
    menu.append_section(None, &help);
    let quit = gio::Menu::new();
    quit.append_item(&item(&i18n("Quit"), "app.quit", Some("<Control>q")));
    menu.append_section(None, &quit);
    menu
}

/// The imports, in the main menu.
pub(super) fn import_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    menu.append_item(&item(
        &i18n("Import .rdp Files…"),
        "win.import-rdp",
        Some("<Control>o"),
    ));
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
