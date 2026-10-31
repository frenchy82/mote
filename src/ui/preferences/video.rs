//! The Video page: where H.264 comes from, and Cisco's OpenH264 to download
//! when the system has no decoder of its own.

use adw::prelude::*;
use gtk::{gio, glib};

use crate::i18n::{i18n, i18n_f};
use crate::rdp::h264::{self, Source, cisco};

pub(super) fn page() -> adw::PreferencesPage {
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::new();
    group.set_title("H.264");
    group.set_description(Some(&i18n("Changes apply to the next connection.")));

    let decoder = adw::ActionRow::new();
    decoder.set_title(&i18n("Decoder"));
    decoder.add_css_class("property");
    group.add(&decoder);

    let download = adw::ActionRow::new();
    download.set_title(&i18n("Cisco’s OpenH264"));
    let button = gtk::Button::new();
    button.set_valign(gtk::Align::Center);
    download.add_suffix(&button);
    download.set_visible(cisco::offered());
    group.add(&download);

    let rows = Rows {
        decoder,
        download,
        button: button.clone(),
    };
    button.connect_clicked(glib::clone!(
        #[strong]
        rows,
        move |_| rows.act()
    ));
    rows.refresh();

    page.add(&group);
    page
}

#[derive(Clone)]
struct Rows {
    decoder: adw::ActionRow,
    download: adw::ActionRow,
    button: gtk::Button,
}

impl Rows {
    /// Find the decoder again, off the main thread: finding one loads it.
    fn refresh(&self) {
        self.decoder.set_subtitle(&i18n("Checking…"));
        let downloaded = cisco::downloaded();
        self.download.set_subtitle(&if downloaded {
            i18n("Downloaded from Cisco, under Cisco’s patent license")
        } else {
            i18n("Downloaded from Cisco on request, under Cisco’s patent license")
        });
        self.button.set_label(&if downloaded {
            i18n("Remove")
        } else {
            i18n("Download")
        });
        self.button.set_sensitive(true);
        self.button.remove_css_class("suggested-action");
        let rows = self.clone();
        glib::spawn_future_local(async move {
            let source = gio::spawn_blocking(h264::source).await.ok().flatten();
            // The download is the way forward only without a decoder.
            if source.is_none() && !downloaded {
                rows.button.add_css_class("suggested-action");
            }
            rows.decoder.set_subtitle(&match source {
                Some(Source::Ffmpeg) => i18n("FFmpeg"),
                Some(Source::SystemOpenH264) => i18n("OpenH264, from this system"),
                Some(Source::CiscoOpenH264) => i18n("OpenH264, from Cisco"),
                None => i18n("None: servers send RemoteFX instead, which blurs small text"),
            });
        });
    }

    fn act(&self) {
        self.button.set_sensitive(false);
        let rows = self.clone();
        if cisco::downloaded() {
            if let Err(e) = cisco::remove() {
                tracing::warn!("removing OpenH264: {e}");
            }
            return rows.refresh();
        }
        self.download.set_subtitle(&i18n("Downloading…"));
        glib::spawn_future_local(async move {
            match gio::spawn_blocking(cisco::download).await {
                Ok(Ok(())) => rows.refresh(),
                Ok(Err(e)) => {
                    tracing::warn!("downloading OpenH264: {e}");
                    rows.refresh();
                    rows.download
                        .set_subtitle(&i18n_f("The download failed: {error}", &[("error", &e)]));
                }
                Err(_) => rows.refresh(),
            }
        });
    }
}
