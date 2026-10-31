//! The main window.

use adw::prelude::*;
use gtk::gio;
use relm4::prelude::*;

use crate::i18n::i18n;
use crate::{APP_ID, APP_NAME, VERSION};

pub(crate) struct AppModel;

#[relm4::component(pub(crate))]
impl SimpleComponent for AppModel {
    type Init = ();
    type Input = ();
    type Output = ();

    view! {
        adw::ApplicationWindow {
            set_title: Some(APP_NAME),
            set_default_size: (960, 640),

            #[wrap(Some)]
            set_content = &adw::ToolbarView {
                add_top_bar = &adw::HeaderBar {
                    pack_end = &gtk::MenuButton {
                        set_icon_name: "open-menu-symbolic",
                        set_tooltip_text: Some(&i18n("Main Menu")),
                        set_primary: true,
                        set_menu_model: Some(&main_menu),
                    },
                },

                #[wrap(Some)]
                set_content = &adw::StatusPage {
                    set_icon_name: Some("network-server-symbolic"),
                    set_title: &i18n("No Connections"),
                },
            },
        }
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let main_menu = gio::Menu::new();
        main_menu.append(Some(&i18n("_About Mote")), Some("win.about"));

        let about = gio::ActionEntry::builder("about")
            .activate(|win: &adw::ApplicationWindow, _, _| present_about(win))
            .build();
        root.add_action_entries([about]);

        let app = relm4::main_adw_application();
        let quit = gio::ActionEntry::builder("quit")
            .activate(|app: &adw::Application, _, _| app.quit())
            .build();
        app.add_action_entries([quit]);
        app.set_accels_for_action("app.quit", &["<Control>q"]);
        app.set_accels_for_action("window.close", &["<Control>w"]);

        let model = AppModel;
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }
}

fn present_about(parent: &adw::ApplicationWindow) {
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
