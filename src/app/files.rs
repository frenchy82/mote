//! Imports, export, and the question before a delete.

use std::path::{Path, PathBuf};

use adw::prelude::*;
use gtk::gio;
use relm4::prelude::*;

use super::{AppModel, AppMsg};
use crate::config;
use crate::connection::Connection;
use crate::i18n::{i18n, i18n_f, ni18n_f};
use crate::import;

fn rdp_filter() -> gio::ListStore {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some(&i18n("Remote Desktop Files")));
    filter.add_suffix("rdp");
    filter.add_mime_type("application/x-rdp");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    filters
}

pub(super) fn choose_rdp_files(
    parent: &adw::ApplicationWindow,
    sender: &ComponentSender<AppModel>,
) {
    let dialog = gtk::FileDialog::builder()
        .title(i18n("Import .rdp Files"))
        .filters(&rdp_filter())
        .build();
    let input = sender.input_sender().clone();
    dialog.open_multiple(Some(parent), gio::Cancellable::NONE, move |result| {
        let Ok(files) = result else { return };
        let paths: Vec<PathBuf> = (0..files.n_items())
            .filter_map(|i| files.item(i).and_downcast::<gio::File>()?.path())
            .collect();
        input.emit(AppMsg::ImportRdpFiles(paths));
    });
}

pub(super) fn choose_export_path(
    conn: &Connection,
    parent: &adw::ApplicationWindow,
    sender: &ComponentSender<AppModel>,
) {
    let dialog = gtk::FileDialog::builder()
        .title(i18n("Export as .rdp File"))
        .initial_name(format!("{}.rdp", conn.title().replace('/', "-")))
        .filters(&rdp_filter())
        .build();
    let (input, id) = (sender.input_sender().clone(), conn.id.clone());
    dialog.save(Some(parent), gio::Cancellable::NONE, move |result| {
        if let Some(path) = result.ok().and_then(|f| f.path()) {
            input.emit(AppMsg::ExportTo(id, path));
        }
    });
}

pub(super) fn confirm_delete(
    conn: &Connection,
    parent: &adw::ApplicationWindow,
    sender: &ComponentSender<AppModel>,
) {
    let dialog = adw::AlertDialog::new(
        Some(&i18n_f("Delete {name}?", &[("name", &conn.title())])),
        Some(&i18n("Its saved password is deleted too.")),
    );
    dialog.add_response("cancel", &i18n("Cancel"));
    dialog.add_response("delete", &i18n("Delete"));
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    let (input, id) = (sender.input_sender().clone(), conn.id.clone());
    dialog.connect_response(Some("delete"), move |_, _| {
        input.emit(AppMsg::Delete(id.clone()));
    });
    dialog.present(Some(parent));
}

impl AppModel {
    pub(super) fn import_rdp_files(&mut self, paths: Vec<PathBuf>) {
        let mut found = Vec::new();
        for path in &paths {
            let name = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            match std::fs::read(path)
                .ok()
                .and_then(|b| import::rdp::parse(&b, &name))
            {
                Some(conn) => found.push(conn),
                None => self.toast(&i18n_f(
                    "{file} is not a connection file",
                    &[("file", &file_name(path))],
                )),
            }
        }
        if !found.is_empty() {
            self.finish_import(found);
        }
    }

    pub(super) fn import_remmina(&mut self) {
        let (found, any_dir) = import::remmina::find();
        if !any_dir || found.is_empty() {
            self.toast(&i18n("No Remmina RDP connections found"));
            return;
        }
        self.finish_import(found);
    }

    fn finish_import(&mut self, found: Vec<Connection>) {
        let last = found.last().map(|c| c.id.clone());
        let added = import::merge(&mut self.connections, found);
        if added == 0 {
            self.toast(&i18n("Those connections are already here"));
            return;
        }
        config::save_connections(&self.connections);
        self.toast(&ni18n_f(
            "Imported {n} connection",
            "Imported {n} connections",
            added as u32,
            &[("n", &added)],
        ));
        if added == 1 {
            self.select(last);
        }
        self.rebuild_list();
    }

    pub(super) fn export(&self, id: &str, path: &Path) {
        let Some(conn) = self.connections.iter().find(|c| c.id == id) else {
            return;
        };
        if let Err(e) = std::fs::write(path, import::rdp::export(conn)) {
            tracing::warn!("exporting to {}: {e}", path.display());
            self.toast(&i18n_f(
                "{file} could not be saved",
                &[("file", &file_name(path))],
            ));
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}
