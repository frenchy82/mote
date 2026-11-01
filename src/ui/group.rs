//! Groups: the names connections are listed under. A group exists while a
//! connection is in it. Names are picked from those already in use, so a
//! group is never split by a typing slip, and a name typed for a new one
//! that differs from an existing one only in case is that one.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;

use crate::connection::Connection;
use crate::i18n::i18n;

/// The groups in use, each once, in the sidebar's order.
pub(crate) fn list(connections: &[Connection]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for conn in connections {
        let name = conn.group.trim();
        if !name.is_empty()
            && !names
                .iter()
                .any(|n| n.to_lowercase() == name.to_lowercase())
        {
            names.push(name.to_owned());
        }
    }
    names.sort_by_key(|n| n.to_lowercase());
    names
}

/// `name` as stored: trimmed, and spelled as the group in `groups` it
/// matches regardless of case.
pub(crate) fn canonical(name: &str, groups: &[String]) -> String {
    let name = name.trim();
    groups
        .iter()
        .find(|g| g.to_lowercase() == name.to_lowercase())
        .cloned()
        .unwrap_or_else(|| name.to_owned())
}

/// Ask for a group's name, then call `done` with it, or with `None` if the
/// dialog was cancelled.
pub(crate) fn ask_name(
    parent: &impl IsA<gtk::Widget>,
    title: &str,
    confirm: &str,
    initial: &str,
    done: impl Fn(Option<String>) + 'static,
) {
    let dialog = adw::AlertDialog::new(Some(title), None);
    let entry = gtk::Entry::new();
    entry.set_text(initial);
    // Enter answers with the default response, while it is enabled.
    entry.set_activates_default(true);
    entry.set_placeholder_text(Some(&i18n("Name")));
    entry.update_property(&[gtk::accessible::Property::Label(&i18n("Name"))]);
    dialog.set_extra_child(Some(&entry));
    dialog.add_responses(&[("cancel", &i18n("Cancel")), ("ok", confirm)]);
    dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("ok"));
    dialog.set_close_response("cancel");
    let filled = |entry: &gtk::Entry| !entry.text().trim().is_empty();
    dialog.set_response_enabled("ok", filled(&entry));
    entry.connect_changed({
        let dialog = dialog.clone();
        move |entry| dialog.set_response_enabled("ok", filled(entry))
    });
    dialog.connect_response(None, move |_, response| {
        done((response == "ok").then(|| entry.text().trim().to_owned()));
    });
    dialog.present(Some(parent));
}

/// The editor's Group row: None, the groups in use, and New Group…, which
/// asks for a name.
#[derive(Clone)]
pub(crate) struct GroupRow {
    pub(crate) row: adw::ComboRow,
    names: Rc<RefCell<Vec<String>>>,
    /// The choice before New Group…, to go back to if it is cancelled.
    before: Rc<Cell<u32>>,
    /// Set while the row's choices are replaced, which changes its selection.
    quiet: Rc<Cell<bool>>,
}

impl GroupRow {
    pub(crate) fn new(current: &str, groups: Vec<String>) -> Self {
        let row = adw::ComboRow::new();
        row.set_title(&i18n("Group"));
        let this = Self {
            row,
            names: Rc::new(RefCell::new(groups)),
            before: Rc::new(Cell::new(0)),
            quiet: Rc::new(Cell::new(false)),
        };
        let current = canonical(current, &this.names.borrow());
        if !current.is_empty() && !this.names.borrow().contains(&current) {
            this.names.borrow_mut().push(current.clone());
        }
        this.fill(&current);
        let this2 = this.clone();
        this.row.connect_selected_notify(move |_| this2.picked());
        this
    }

    /// The chosen group, empty for none.
    pub(crate) fn group(&self) -> String {
        match self.row.selected() as usize {
            0 => String::new(),
            i => self.names.borrow().get(i - 1).cloned().unwrap_or_default(),
        }
    }

    /// Offer the names, sorted, and choose `chosen`.
    fn fill(&self, chosen: &str) {
        self.names.borrow_mut().sort_by_key(|n| n.to_lowercase());
        let names = self.names.borrow();
        let mut labels = vec![i18n("None")];
        labels.extend(names.iter().cloned());
        labels.push(i18n("New Group…"));
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        let at = names.iter().position(|n| n == chosen).map_or(0, |i| i + 1) as u32;
        self.quiet.set(true);
        self.row.set_model(Some(&gtk::StringList::new(&labels)));
        self.row.set_selected(at);
        self.quiet.set(false);
        self.before.set(at);
    }

    fn picked(&self) {
        if self.quiet.get() {
            return;
        }
        let selected = self.row.selected();
        if (selected as usize) <= self.names.borrow().len() {
            self.before.set(selected);
            return;
        }
        let this = self.clone();
        ask_name(
            &self.row,
            &i18n("New Group"),
            &i18n("Add"),
            "",
            move |name| match name {
                Some(name) => {
                    let name = canonical(&name, &this.names.borrow());
                    if !this.names.borrow().contains(&name) {
                        this.names.borrow_mut().push(name.clone());
                    }
                    this.fill(&name);
                }
                None => {
                    this.quiet.set(true);
                    this.row.set_selected(this.before.get());
                    this.quiet.set(false);
                }
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_are_listed_once_and_matched_regardless_of_case() {
        let conn = |group: &str| Connection {
            group: group.into(),
            ..Connection::default()
        };
        let groups = list(&[conn("Lab"), conn("home"), conn("lab "), conn("")]);
        assert_eq!(groups, ["home", "Lab"]);
        assert_eq!(canonical(" LAB", &groups), "Lab");
        assert_eq!(canonical("Office", &groups), "Office");
    }
}
