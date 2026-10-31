//! The sidebar's connection list, rebuilt whole when the connections, the
//! search or the settings change. A list of a few dozen rows rebuilds faster
//! than it can be kept in step.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use relm4::prelude::*;

use super::{AppModel, AppMsg};
use crate::config::SortOrder;
use crate::connection::{Connection, split_address};
use crate::i18n::{i18n, i18n_f};

const ROW_PREFIX: &str = "conn:";

/// Selecting a row shows its connection, a double click connects to it,
/// and Delete asks to delete it.
pub(super) fn connect_list(
    list: &gtk::ListBox,
    quiet: &Rc<Cell<bool>>,
    sender: &ComponentSender<AppModel>,
) {
    let (input, quiet) = (sender.input_sender().clone(), quiet.clone());
    list.connect_row_selected(move |_, row| {
        if quiet.get() {
            return;
        }
        if let Some(id) = row.and_then(row_id) {
            input.emit(AppMsg::Select(id));
        }
    });
    // A double click connects, as it opens a file in Files.
    let clicks = gtk::GestureClick::new();
    let input = sender.input_sender().clone();
    clicks.connect_pressed(move |_, n_press, _, _| {
        if n_press == 2 {
            input.emit(AppMsg::ConnectSelected);
        }
    });
    list.add_controller(clicks);
    let keys = gtk::EventControllerKey::new();
    let input = sender.input_sender().clone();
    keys.connect_key_pressed(move |_, key, _, mods| {
        if key == gtk::gdk::Key::Delete && mods.is_empty() {
            input.emit(AppMsg::DeleteSelected);
            return gtk::glib::Propagation::Stop;
        }
        gtk::glib::Propagation::Proceed
    });
    list.add_controller(keys);
}

fn row_id(row: &gtk::ListBoxRow) -> Option<String> {
    row.widget_name()
        .strip_prefix(ROW_PREFIX)
        .map(str::to_owned)
}

/// The connection of the first row the search left, if any.
pub(super) fn first_visible(list: &gtk::ListBox) -> Option<String> {
    let mut i = 0;
    while let Some(row) = list.row_at_index(i) {
        if let Some(id) = row_id(&row) {
            return Some(id);
        }
        i += 1;
    }
    None
}

impl AppModel {
    /// Mark the selected connection's row, without that counting as the
    /// user picking it.
    pub(super) fn highlight_selected(&self) {
        let wanted = self.selected.as_deref();
        let mut i = 0;
        while let Some(row) = self.list.row_at_index(i) {
            if wanted.is_some() && row_id(&row).as_deref() == wanted {
                if !row.is_selected() {
                    self.quiet.set(true);
                    self.list.select_row(Some(&row));
                    self.quiet.set(false);
                }
                return;
            }
            i += 1;
        }
        self.quiet.set(true);
        self.list.unselect_all();
        self.quiet.set(false);
    }

    pub(super) fn rebuild_list(&self) {
        self.quiet.set(true);
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }

        let mut shown: Vec<&Connection> = self
            .connections
            .iter()
            .filter(|c| self.query.is_empty() || c.matches(&self.query))
            .collect();
        let by_name = |a: &&Connection, b: &&Connection| {
            a.title().to_lowercase().cmp(&b.title().to_lowercase())
        };
        match self.settings.sort {
            SortOrder::Name => shown.sort_by(by_name),
            SortOrder::RecentlyUsed => {
                shown.sort_by(|a, b| b.last_used.cmp(&a.last_used).then_with(|| by_name(a, b)))
            }
        }
        if self.settings.show_groups {
            // Ungrouped first, then each group under its name, in order.
            // The sort is stable, so each group keeps the order above.
            shown.sort_by(|a, b| {
                (!a.group.is_empty(), a.group.to_lowercase())
                    .cmp(&(!b.group.is_empty(), b.group.to_lowercase()))
            });
        }

        let mut group: Option<&str> = None;
        for conn in &shown {
            if self.settings.show_groups && !conn.group.is_empty() && group != Some(&conn.group) {
                group = Some(&conn.group);
                self.list.append(&heading(&conn.group));
            }
            let open = self.sessions.contains_key(&conn.id);
            let row = connection_row(conn, self.settings.show_hosts, open);
            self.list.append(&row);
            if self.selected.as_deref() == Some(conn.id.as_str()) {
                self.list.select_row(Some(&row));
            }
        }

        let hint = if !shown.is_empty() || self.query.is_empty() {
            None
        } else if let Some((host, _)) = split_address(&self.query) {
            Some(i18n_f(
                "Press Enter to add {address}",
                &[("address", &host)],
            ))
        } else {
            Some(i18n("No Results"))
        };
        self.hint.set_visible(hint.is_some());
        self.hint.set_label(hint.as_deref().unwrap_or_default());
        self.quiet.set(false);
    }
}

fn heading(text: &str) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_selectable(false);
    row.set_activatable(false);
    row.set_can_focus(false);
    row.add_css_class("sidebar-heading");
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.add_css_class("caption-heading");
    label.add_css_class("dim-label");
    row.set_child(Some(&label));
    row
}

/// A connection's row; `open` when a session to it is open.
fn connection_row(conn: &Connection, show_host: bool, open: bool) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_widget_name(&format!("{ROW_PREFIX}{}", conn.id));
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    line.add_css_class("nav-row");
    line.append(&adw::Avatar::new(32, Some(conn.title()), true));

    let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
    text.set_valign(gtk::Align::Center);
    text.set_hexpand(true);
    let name = gtk::Label::new(Some(conn.title()));
    name.set_xalign(0.0);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    name.add_css_class("row-name");
    text.append(&name);
    if show_host && !conn.name.trim().is_empty() {
        let host = gtk::Label::new(Some(&conn.address()));
        host.set_xalign(0.0);
        host.set_ellipsize(gtk::pango::EllipsizeMode::End);
        host.add_css_class("caption");
        host.add_css_class("dim-label");
        text.append(&host);
    }
    line.append(&text);
    if open {
        let dot = gtk::Image::from_icon_name("media-record-symbolic");
        dot.set_pixel_size(8);
        dot.add_css_class("session-dot");
        dot.set_tooltip_text(Some(&i18n("Connected")));
        line.append(&dot);
    }
    row.set_child(Some(&line));
    row.set_tooltip_text(Some(&conn.address()));
    row
}
