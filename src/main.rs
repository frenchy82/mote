//! Mote: a GNOME-native remote desktop client for Linux machines running RDP.

// A binary crate exports nothing, so a bare `pub` only hides how far an item
// really reaches; `pub(crate)` or narrower says it.
#![warn(unreachable_pub)]

mod app;
mod i18n;

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

    relm4::RelmApp::new(APP_ID).run::<app::AppModel>(());
}
