//! A development hook, as Hylki has: with `MOTE_SHOWCASE=<file.png>` set,
//! the app stages a view and writes a picture of it, so a change to the
//! interface can be checked without anyone at the keyboard.
//!
//! - `MOTE_SHOWCASE_OPEN`: comma-separated steps before the capture:
//!   `first` selects the first connection, `editor` opens it in the editor
//!   (or a new one), `settings` or `settings:<page>` opens Settings and
//!   captures that window instead, `about` opens About, `search:<text>`
//!   opens the search with that text in it, and `connect:<name>` connects
//!   to the connection of that name and captures its session window.
//! - `MOTE_SHOWCASE_DELAY`: seconds before the capture, 3 by default.

use std::time::Duration;

use adw::prelude::*;
use relm4::prelude::*;

use super::{AppModel, AppMsg};
use crate::ui::preferences::PrefInput;

impl AppModel {
    pub(super) fn arm_showcase(&self, sender: &ComponentSender<Self>) {
        let Ok(path) = std::env::var("MOTE_SHOWCASE") else {
            return;
        };
        let steps = std::env::var("MOTE_SHOWCASE_OPEN").unwrap_or_default();
        for step in steps.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            match step.split_once(':').unwrap_or((step, "")) {
                ("first", _) => {
                    if let Some(id) = super::sidebar::first_visible(&self.list) {
                        sender.input(AppMsg::Select(id));
                    }
                }
                ("editor", _) => match super::sidebar::first_visible(&self.list) {
                    Some(_) if steps.contains("first") => sender.input(AppMsg::EditSelected),
                    _ => sender.input(AppMsg::NewConnection(None)),
                },
                ("settings", page) => {
                    sender.input(AppMsg::OpenSettings);
                    sender.input(AppMsg::ShowcaseSettingsPage(page.to_owned()));
                }
                ("about", _) => sender.input(AppMsg::About),
                ("connect", name) => {
                    if let Some(id) = self
                        .connections
                        .iter()
                        .find(|c| c.title() == name)
                        .map(|c| c.id.clone())
                    {
                        sender.input(AppMsg::Select(id));
                        sender.input(AppMsg::ConnectSelected);
                        // Nobody clicked Connect, so the compositor keeps
                        // the new window behind this one, where input
                        // can't reach it.
                        sender.input(AppMsg::ShowcaseMinimize);
                    }
                }
                ("search", text) => {
                    sender.input(AppMsg::FocusSearch);
                    self.search.set_text(text);
                }
                (other, _) => tracing::warn!("showcase: unknown step {other}"),
            }
        }
        let delay = std::env::var("MOTE_SHOWCASE_DELAY")
            .ok()
            .and_then(|d| d.parse().ok())
            .unwrap_or(3);
        let input = sender.input_sender().clone();
        gtk::glib::timeout_add_local_once(Duration::from_secs(delay), move || {
            input.emit(AppMsg::ShowcaseCapture(path));
        });
    }

    pub(super) fn showcase_settings_page(&self, page: &str) {
        if let Some(prefs) = &self.prefs
            && !page.is_empty()
        {
            prefs.emit(PrefInput::SelectPage(page.to_owned()));
        }
    }

    pub(super) fn showcase_capture(&self, path: &str, root: &adw::ApplicationWindow) {
        let session = self.sessions.values().next().map(|s| s.widget().clone());
        let window: gtk::Widget = match (&self.prefs, session) {
            (_, Some(session)) => session.upcast(),
            (Some(prefs), _) if prefs.widget().is_visible() => prefs.widget().clone().upcast(),
            _ => root.clone().upcast(),
        };
        match capture(&window, path) {
            Ok(()) => tracing::info!("showcase: wrote {path}"),
            Err(e) => tracing::error!("showcase: {e}"),
        }
        relm4::main_adw_application().quit();
    }
}

fn capture(widget: &gtk::Widget, path: &str) -> Result<(), String> {
    let scale = f64::from(widget.scale_factor());
    let (w, h) = (f64::from(widget.width()), f64::from(widget.height()));
    let paintable = gtk::WidgetPaintable::new(Some(widget));
    let snapshot = gtk::Snapshot::new();
    snapshot.scale(scale as f32, scale as f32);
    paintable.snapshot(&snapshot, w, h);
    let node = snapshot
        .to_node()
        .ok_or("nothing to render (window suspended?)")?;
    let renderer = widget
        .native()
        .and_then(|n| n.renderer())
        .ok_or("no renderer")?;
    let bounds = gtk::graphene::Rect::new(0.0, 0.0, (w * scale) as f32, (h * scale) as f32);
    renderer
        .render_texture(&node, Some(&bounds))
        .save_to_png(path)
        .map_err(|e| e.to_string())
}
