//! Mote: a GNOME-native remote desktop client for Linux machines running RDP.

// A binary crate exports nothing, so a bare `pub` only hides how far an item
// really reaches; `pub(crate)` or narrower says it.
#![warn(unreachable_pub)]

mod app;
mod config;
mod connection;
mod i18n;
mod import;
mod paths;
mod rdp;
mod secrets;
mod stats;
mod ui;

use tracing_subscriber::EnvFilter;

const APP_ID: &str = "co.hyprlab.Mote";

/// The user-visible application name. A name, so never translated.
const APP_NAME: &str = "Mote";

/// The user-visible version: the crate version verbatim.
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    i18n::init();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("MOTE_LOG").unwrap_or_else(|_| EnvFilter::new("mote=info")),
        )
        .init();
    tracing::info!("{APP_NAME} {VERSION}");
    register_resources();
    paths::move_from_sandbox();

    let app = relm4::RelmApp::new(APP_ID);
    // relm4 sets the ID on an application it has already built, too late
    // for GLib to derive the resource base path GTK finds the icon under.
    {
        use gtk::gio::prelude::ApplicationExt;
        relm4::main_adw_application().set_resource_base_path(Some("/co/hyprlab/Mote"));
    }
    app.run::<app::AppModel>(());
}

/// Register the app icon built into the binary. GTK looks under the
/// application's resource base path, in its `icons` directory, as part of
/// hicolor: About and the window find the icon even
/// when Mote runs from `target/` and nothing is installed.
fn register_resources() {
    use gtk::{gio, glib};
    let bytes =
        glib::Bytes::from_static(include_bytes!(concat!(env!("OUT_DIR"), "/mote.gresource")));
    match gio::Resource::from_data(&bytes) {
        Ok(resource) => gio::resources_register(&resource),
        Err(e) => tracing::error!("failed to register the app icon: {e}"),
    }
}
