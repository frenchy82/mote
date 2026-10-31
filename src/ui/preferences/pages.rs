//! The Settings categories' pages. Every row reports its change at once;
//! the main window applies and saves it.
//!
//! Combo and spin rows are set once, here, and never bound to a watched
//! value: Hylki's Settings window hung GTK's list item manager that way.

use adw::prelude::*;
use relm4::prelude::*;

use super::{PrefChange, Preferences};
use crate::config::{ColorScheme, Settings, SortOrder};
use crate::i18n::i18n;

pub(super) fn build(
    settings: &Settings,
    sender: &ComponentSender<Preferences>,
) -> Vec<(&'static str, adw::PreferencesPage)> {
    vec![
        ("defaults", defaults(settings, sender)),
        ("appearance", appearance(settings, sender)),
        ("sidebar", sidebar(settings, sender)),
        ("video", super::video::page()),
    ]
}

fn page_with(groups: &[&adw::PreferencesGroup]) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::new();
    for group in groups {
        page.add(*group);
    }
    page
}

fn defaults(settings: &Settings, sender: &ComponentSender<Preferences>) -> adw::PreferencesPage {
    let group = adw::PreferencesGroup::new();
    group.set_title(&i18n("New Connections"));
    group.set_description(Some(&i18n(
        "What a connection starts with when it is added.",
    )));

    let port = adw::SpinRow::with_range(1.0, 65535.0, 1.0);
    port.set_title(&i18n("Port"));
    port.set_value(f64::from(settings.default_port));
    let out = sender.output_sender().clone();
    port.connect_value_notify(move |r| out.emit(PrefChange::DefaultPort(r.value() as u16)));
    group.add(&port);

    let user = text_row(&i18n("User Name"), &settings.default_username);
    let out = sender.output_sender().clone();
    user.connect_changed(move |r| {
        out.emit(PrefChange::DefaultUsername(r.text().trim().to_owned()))
    });
    group.add(&user);

    let domain = text_row(&i18n("Domain"), &settings.default_domain);
    let out = sender.output_sender().clone();
    domain
        .connect_changed(move |r| out.emit(PrefChange::DefaultDomain(r.text().trim().to_owned())));
    group.add(&domain);

    page_with(&[&group])
}

fn appearance(settings: &Settings, sender: &ComponentSender<Preferences>) -> adw::PreferencesPage {
    let group = adw::PreferencesGroup::new();
    let row = adw::ActionRow::new();
    row.set_title(&i18n("Style"));

    let toggles = adw::ToggleGroup::new();
    toggles.set_valign(gtk::Align::Center);
    let choices = [
        ("system", i18n("System"), ColorScheme::System),
        ("light", i18n("Light"), ColorScheme::Light),
        ("dark", i18n("Dark"), ColorScheme::Dark),
    ];
    for (name, label, _) in &choices {
        toggles.add(adw::Toggle::builder().name(*name).label(label).build());
    }
    let current = choices
        .iter()
        .find(|(_, _, s)| *s == settings.color_scheme)
        .map_or("system", |(n, _, _)| *n);
    toggles.set_active_name(Some(current));
    let out = sender.output_sender().clone();
    toggles.connect_active_name_notify(move |t| {
        let picked = t.active_name();
        if let Some((_, _, scheme)) = choices
            .iter()
            .find(|(n, _, _)| Some(*n) == picked.as_deref())
        {
            out.emit(PrefChange::ColorScheme(*scheme));
        }
    });
    row.add_suffix(&toggles);
    group.add(&row);
    page_with(&[&group])
}

fn sidebar(settings: &Settings, sender: &ComponentSender<Preferences>) -> adw::PreferencesPage {
    let group = adw::PreferencesGroup::new();
    group.set_title(&i18n("Connections"));

    let sort = adw::ComboRow::new();
    sort.set_title(&i18n("Sort By"));
    let orders = [SortOrder::Name, SortOrder::RecentlyUsed];
    sort.set_model(Some(&gtk::StringList::new(&[
        &i18n("Name"),
        &i18n("Recently Used"),
    ])));
    sort.set_selected(orders.iter().position(|o| *o == settings.sort).unwrap_or(0) as u32);
    let out = sender.output_sender().clone();
    sort.connect_selected_notify(move |r| {
        if let Some(order) = orders.get(r.selected() as usize) {
            out.emit(PrefChange::Sort(*order));
        }
    });
    group.add(&sort);

    let groups = adw::SwitchRow::new();
    groups.set_title(&i18n("Show Groups"));
    groups.set_subtitle(&i18n("Connections in a group are listed under its name."));
    groups.set_active(settings.show_groups);
    let out = sender.output_sender().clone();
    groups.connect_active_notify(move |r| out.emit(PrefChange::ShowGroups(r.is_active())));
    group.add(&groups);

    let hosts = adw::SwitchRow::new();
    hosts.set_title(&i18n("Show Addresses"));
    hosts.set_active(settings.show_hosts);
    let out = sender.output_sender().clone();
    hosts.connect_active_notify(move |r| out.emit(PrefChange::ShowHosts(r.is_active())));
    group.add(&hosts);

    page_with(&[&group])
}

fn text_row(title: &str, text: &str) -> adw::EntryRow {
    let row = adw::EntryRow::new();
    row.set_title(title);
    row.set_text(text);
    row
}
