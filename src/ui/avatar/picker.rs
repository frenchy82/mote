//! The editor's picture: the connection's avatar, with a button that offers
//! other robots, more of them on request, and an image of the user's own.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use super::{Avatar, import_picture};
use crate::i18n::{i18n, i18n_f};

/// How many robots the popover offers at a time.
const OFFERED: usize = 12;

/// What the editor holds while it is open: the robot's seed (empty for the
/// one the connection's id picks) and the image's name (empty for none),
/// with the images copied in meanwhile, to forget those not kept.
#[derive(Default)]
pub(crate) struct Choice {
    pub(crate) robot: String,
    pub(crate) picture: String,
    pub(crate) imported: Vec<String>,
}

/// The avatar, large, with a button at its corner to change it. `id` is
/// the connection's, whose robot is the first offered.
pub(crate) fn picker(id: &str, choice: &Rc<RefCell<Choice>>) -> gtk::Widget {
    let avatar = Avatar::new(96);
    let show = {
        let (avatar, choice, id) = (avatar.clone(), choice.clone(), id.to_owned());
        Rc::new(move || {
            let choice = choice.borrow();
            let seed = if choice.robot.is_empty() {
                &id
            } else {
                &choice.robot
            };
            avatar.show(seed, &choice.picture);
        })
    };
    show();

    let robots = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(6)
        .max_children_per_line(6)
        .column_spacing(6)
        .row_spacing(6)
        .build();
    let button = gtk::MenuButton::builder()
        .icon_name("document-edit-symbolic")
        .tooltip_text(i18n("Change Account Avatar"))
        .halign(gtk::Align::End)
        .valign(gtk::Align::End)
        .css_classes(["circular", "avatar-edit"])
        .build();
    button.update_property(&[gtk::accessible::Property::Label(&i18n("Change Account Avatar"))]);

    let offer = {
        let (robots, choice, show, button, id) = (
            robots.clone(),
            choice.clone(),
            show.clone(),
            button.clone(),
            id.to_owned(),
        );
        Rc::new(move || {
            robots.remove_all();
            // The connection's own robot first, then others at random.
            let seeds = std::iter::once(String::new())
                .chain((1..OFFERED).map(|_| glib::uuid_string_random().to_string()));
            for (number, seed) in seeds.enumerate() {
                let face = Avatar::new(40);
                face.show(if seed.is_empty() { &id } else { &seed }, "");
                let pick = gtk::Button::builder()
                    .child(&face)
                    .css_classes(["flat", "robot-choice"])
                    .build();
                let name = i18n_f("Robot {number}", &[("number", &(number + 1))]);
                pick.set_tooltip_text(Some(&name));
                pick.update_property(&[gtk::accessible::Property::Label(&name)]);
                let (choice, show, button) = (choice.clone(), show.clone(), button.clone());
                pick.connect_clicked(move |_| {
                    {
                        let mut choice = choice.borrow_mut();
                        choice.robot = seed.clone();
                        choice.picture.clear();
                    }
                    show();
                    button.popdown();
                });
                robots.insert(&pick, -1);
            }
        })
    };
    offer();

    let more = gtk::Button::builder()
        .icon_name("view-refresh-symbolic")
        .tooltip_text(i18n("More Robots"))
        .build();
    more.update_property(&[gtk::accessible::Property::Label(&i18n("More Robots"))]);
    more.connect_clicked({
        let offer = offer.clone();
        move |_| offer()
    });
    let image = gtk::Button::with_label(&i18n("Choose Image…"));
    image.set_hexpand(true);
    image.connect_clicked({
        let (choice, show, button) = (choice.clone(), show.clone(), button.clone());
        move |widget| {
            button.popdown();
            choose_image(widget, choice.clone(), show.clone());
        }
    });
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    actions.append(&image);
    actions.append(&more);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_top(6);
    content.set_margin_bottom(6);
    content.set_margin_start(6);
    content.set_margin_end(6);
    content.append(&robots);
    content.append(&actions);
    let popover = gtk::Popover::new();
    popover.set_child(Some(&content));
    button.set_popover(Some(&popover));

    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&avatar));
    overlay.add_overlay(&button);
    overlay.set_halign(gtk::Align::Center);
    overlay.upcast()
}

/// Ask for an image, and keep a copy of it as the connection's picture.
fn choose_image(widget: &gtk::Button, choice: Rc<RefCell<Choice>>, show: Rc<dyn Fn()>) {
    let images = gtk::FileFilter::new();
    images.set_name(Some(&i18n("Images")));
    images.add_mime_type("image/*");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&images);
    let dialog = gtk::FileDialog::builder()
        .title(i18n("Choose Image"))
        .modal(true)
        .filters(&filters)
        .default_filter(&images)
        .build();
    let window = widget.root().and_downcast::<gtk::Window>();
    dialog.open(window.as_ref(), None::<&gio::Cancellable>, move |chosen| {
        let Some(path) = chosen.ok().and_then(|file| file.path()) else {
            return;
        };
        glib::spawn_future_local(async move {
            match import_picture(path).await {
                Ok(name) => {
                    {
                        let mut choice = choice.borrow_mut();
                        choice.imported.push(name.clone());
                        choice.picture = name;
                    }
                    show();
                }
                Err(e) => tracing::warn!("keeping the chosen picture: {e}"),
            }
        });
    });
}
