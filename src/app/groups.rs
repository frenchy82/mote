//! Putting connections into groups from the sidebar, and renaming or
//! dissolving a group from its heading.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use relm4::prelude::*;

use super::{AppModel, AppMsg};
use crate::config;
use crate::i18n::i18n;
use crate::ui::group;

/// What the sidebar's menus offer: the groups in use, and each
/// connection's. Kept by `rebuild_list`.
#[derive(Default)]
pub(super) struct GroupIndex {
    pub(super) names: Vec<String>,
    pub(super) of: HashMap<String, String>,
}

pub(super) type SharedGroups = Rc<RefCell<GroupIndex>>;

impl AppModel {
    pub(super) fn index_groups(&self) {
        let mut index = self.groups.borrow_mut();
        index.names = group::list(&self.connections);
        index.of = self
            .connections
            .iter()
            .map(|c| (c.id.clone(), c.group.clone()))
            .collect();
    }

    /// Put connection `id` in `name`, or in no group for an empty one.
    pub(super) fn set_group(&mut self, id: &str, name: &str) {
        let name = group::canonical(name, &group::list(&self.connections));
        let Some(conn) = self.connections.iter_mut().find(|c| c.id == id) else {
            return;
        };
        if conn.group == name {
            return;
        }
        conn.group = name;
        config::save_connections(&self.connections);
        self.rebuild_list();
    }

    /// Give every connection in group `old` the group `new`: a rename, a
    /// merge into a group already called that, or, for an empty one, no
    /// group.
    pub(super) fn regroup(&mut self, old: &str, new: &str) {
        let new = group::canonical(new, &group::list(&self.connections));
        let mut changed = false;
        for conn in self.connections.iter_mut().filter(|c| c.group == old) {
            conn.group = new.clone();
            changed = true;
        }
        if changed {
            config::save_connections(&self.connections);
            self.rebuild_list();
        }
    }

    /// Ask for a new group's name, then put connection `id` in it.
    pub(super) fn ask_new_group(
        &self,
        id: String,
        root: &adw::ApplicationWindow,
        sender: &ComponentSender<Self>,
    ) {
        let input = sender.input_sender().clone();
        group::ask_name(root, &i18n("New Group"), &i18n("Add"), "", move |name| {
            if let Some(group) = name {
                input.emit(AppMsg::SetGroup {
                    id: id.clone(),
                    group,
                });
            }
        });
    }

    pub(super) fn ask_rename_group(
        &self,
        old: String,
        root: &adw::ApplicationWindow,
        sender: &ComponentSender<Self>,
    ) {
        let input = sender.input_sender().clone();
        let initial = old.clone();
        group::ask_name(
            root,
            &i18n("Rename Group"),
            &i18n("Rename"),
            &initial,
            move |name| {
                if let Some(new) = name {
                    input.emit(AppMsg::Regroup {
                        old: old.clone(),
                        new,
                    });
                }
            },
        );
    }
}
