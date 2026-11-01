//! A connection's picture: a robot from DiceBear's Pixelbot (CC0), the one
//! its id picks or one chosen in the editor, or an image the user chose,
//! cut to a square with rounded corners. Robots are drawn rather than kept as images: a grid of
//! rounded squares, sharp at any size and scale, and nothing held in memory
//! per connection.

mod picker;
#[rustfmt::skip]
mod pixelbot;

pub(crate) use picker::{Choice, picker};

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;

use gtk::glib::subclass::prelude::*;
use gtk::prelude::*;
use gtk::subclass::widget::{WidgetClassExt, WidgetImpl};
use gtk::{gdk, gio, glib, graphene, gsk};

use crate::connection::Connection;

/// The largest an image chosen for a connection is kept at, in pixels: twice
/// the largest avatar, for a 2× screen.
const PICTURE_SIZE: u32 = 192;

/// How round a picture's corners are, as a share of its side.
const ROUNDING: f32 = 0.2;

/// Pixelbot's grid: 12 cells to a side, each a rounded square, on a canvas
/// of 100.
const STEP: f32 = 100.0 / 12.0;
const MARGIN: f32 = 0.42;
const CELL: f32 = 7.5;
const CORNER: f32 = 1.9;

/// One robot: its background, the color its eyes and mouth glow, and which
/// eyes and mouth.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Robot {
    background: u8,
    glow: u8,
    eyes: u8,
    mouth: u8,
}

impl Robot {
    /// The robot a seed stands for: the same seed, the same robot, so only
    /// the seed is kept.
    pub(crate) fn from_seed(seed: &str) -> Self {
        // FNV-1a, then SplitMix64's finish to spread nearby seeds apart.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in seed.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        hash = (hash ^ (hash >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        hash = (hash ^ (hash >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        hash ^= hash >> 31;
        let mut pick = |n: usize| {
            let i = (hash % n as u64) as u8;
            hash /= n as u64;
            i
        };
        Self {
            background: pick(pixelbot::BACKGROUNDS.len()),
            glow: pick(pixelbot::GLOWS.len()),
            eyes: pick(pixelbot::EYES.len()),
            mouth: pick(pixelbot::MOUTHS.len()),
        }
    }

    /// The robot as a render node, `size` pixels to a side.
    fn node(self, size: f32) -> Option<gsk::RenderNode> {
        let scale = size / 100.0;
        let cell = |(col, row): (u8, u8)| {
            gsk::RoundedRect::from_rect(
                graphene::Rect::new(
                    (MARGIN + f32::from(col) * STEP) * scale,
                    (MARGIN + f32::from(row) * STEP) * scale,
                    CELL * scale,
                    CELL * scale,
                ),
                CORNER * scale,
            )
        };
        let snapshot = gtk::Snapshot::new();
        snapshot.append_color(
            &color(pixelbot::BACKGROUNDS[usize::from(self.background)], 1.0),
            &graphene::Rect::new(0.0, 0.0, size, size),
        );
        // Every cell, dim; then the eyes and mouth, lit, over them.
        let dim = gsk::PathBuilder::new();
        for row in 0..12 {
            for col in 0..12 {
                dim.add_rounded_rect(&cell((col, row)));
            }
        }
        snapshot.append_fill(&dim.to_path(), gsk::FillRule::Winding, &color(0, 0.4));
        let lit = gsk::PathBuilder::new();
        let eyes = pixelbot::EYES[usize::from(self.eyes)];
        let mouth = pixelbot::MOUTHS[usize::from(self.mouth)];
        for &place in eyes.iter().chain(mouth) {
            lit.add_rounded_rect(&cell(place));
        }
        snapshot.append_fill(
            &lit.to_path(),
            gsk::FillRule::Winding,
            &color(pixelbot::GLOWS[usize::from(self.glow)], 1.0),
        );
        snapshot.to_node()
    }
}

fn color(rgb: u32, alpha: f32) -> gdk::RGBA {
    let channel = |shift: u32| ((rgb >> shift) & 0xff) as f32 / 255.0;
    gdk::RGBA::new(channel(16), channel(8), channel(0), alpha)
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub(crate) struct Avatar {
        pub(super) size: Cell<i32>,
        pub(super) robot: Cell<Robot>,
        pub(super) picture: RefCell<Option<gdk::Texture>>,
        /// The robot drawn last, and the size it was drawn at.
        pub(super) drawn: RefCell<Option<(f32, gsk::RenderNode)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Avatar {
        const NAME: &'static str = "MoteAvatar";
        type Type = super::Avatar;
        type ParentType = gtk::Widget;

        fn class_init(class: &mut Self::Class) {
            class.set_css_name("mote-avatar");
            class.set_accessible_role(gtk::AccessibleRole::Img);
        }
    }

    impl ObjectImpl for Avatar {}

    impl WidgetImpl for Avatar {
        fn measure(&self, _: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            let size = self.size.get();
            (size, size, -1, -1)
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let widget = self.obj();
            let size = self.size.get() as f32;
            // Centered in whatever room it is given.
            let x = ((widget.width() as f32 - size) / 2.0).max(0.0);
            let y = ((widget.height() as f32 - size) / 2.0).max(0.0);
            let shape =
                gsk::RoundedRect::from_rect(graphene::Rect::new(x, y, size, size), size * ROUNDING);
            snapshot.push_rounded_clip(&shape);
            if let Some(picture) = self.picture.borrow().as_ref() {
                // Filling the square, the middle kept.
                let (w, h) = (picture.width() as f32, picture.height() as f32);
                let fill = size / w.min(h);
                let bounds = graphene::Rect::new(
                    x + (size - w * fill) / 2.0,
                    y + (size - h * fill) / 2.0,
                    w * fill,
                    h * fill,
                );
                snapshot.append_scaled_texture(picture, gsk::ScalingFilter::Trilinear, &bounds);
            } else {
                let mut drawn = self.drawn.borrow_mut();
                if drawn.as_ref().is_none_or(|(at, _)| *at != size) {
                    *drawn = self.robot.get().node(size).map(|node| (size, node));
                }
                if let Some((_, node)) = drawn.as_ref() {
                    snapshot.translate(&graphene::Point::new(x, y));
                    snapshot.append_node(node);
                    snapshot.translate(&graphene::Point::new(-x, -y));
                }
            }
            snapshot.pop();
        }
    }
}

glib::wrapper! {
    pub(crate) struct Avatar(ObjectSubclass<imp::Avatar>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Avatar {
    pub(crate) fn new(size: i32) -> Self {
        let avatar: Self = glib::Object::new();
        avatar.imp().size.set(size);
        avatar.set_halign(gtk::Align::Center);
        avatar.set_valign(gtk::Align::Center);
        avatar
    }

    /// Show the connection's own: its image, or its robot.
    pub(crate) fn show_connection(&self, connection: &Connection) {
        self.show(&robot_seed(connection), &connection.picture);
    }

    /// Show the robot `seed` stands for, or the image kept as `picture`
    /// when there is one.
    pub(crate) fn show(&self, seed: &str, picture: &str) {
        self.imp().robot.set(Robot::from_seed(seed));
        self.imp().drawn.replace(None);
        self.imp().picture.replace(
            Some(picture)
                .filter(|p| !p.is_empty())
                .and_then(load_picture),
        );
        self.queue_draw();
    }
}

/// The seed of the connection's robot: the one chosen, or else its id.
pub(crate) fn robot_seed(connection: &Connection) -> String {
    if connection.robot.is_empty() {
        connection.id.clone()
    } else {
        connection.robot.clone()
    }
}

/// Where the images chosen for connections are kept.
fn pictures_dir() -> PathBuf {
    glib::user_data_dir().join("mote").join("pictures")
}

thread_local! {
    /// Images already read, by name: a list redrawn reads none twice.
    static PICTURES: RefCell<HashMap<String, gdk::Texture>> = RefCell::new(HashMap::new());
}

fn load_picture(name: &str) -> Option<gdk::Texture> {
    if let Some(texture) = PICTURES.with(|p| p.borrow().get(name).cloned()) {
        return Some(texture);
    }
    // A name is only ever a file in the pictures folder.
    if name.contains('/') {
        return None;
    }
    match gdk::Texture::from_filename(pictures_dir().join(name)) {
        Ok(texture) => {
            PICTURES.with(|p| p.borrow_mut().insert(name.to_owned(), texture.clone()));
            Some(texture)
        }
        Err(e) => {
            tracing::warn!("reading the picture {name}: {e}");
            None
        }
    }
}

/// Keep a copy of the image at `source` for a connection, cut to a square
/// and made no larger than it is shown: the original can then move or go.
/// Returns the copy's name.
pub(crate) async fn import_picture(source: PathBuf) -> Result<String, String> {
    gio::spawn_blocking(move || {
        let texture = gdk::Texture::from_filename(&source).map_err(|e| e.message().to_owned())?;
        let square = crop_square(&texture, PICTURE_SIZE);
        let dir = pictures_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let name = format!("{}.png", glib::uuid_string_random());
        square
            .save_to_png(dir.join(&name))
            .map_err(|e| e.to_string())?;
        Ok(name)
    })
    .await
    .map_err(|_| "the picture couldn't be read".to_owned())?
}

/// A copy of a kept image under a new name, for a duplicated connection.
pub(crate) fn copy_picture(name: &str) -> Option<String> {
    if name.is_empty() || name.contains('/') {
        return None;
    }
    let copy = format!("{}.png", glib::uuid_string_random());
    let dir = pictures_dir();
    match std::fs::copy(dir.join(name), dir.join(&copy)) {
        Ok(_) => Some(copy),
        Err(e) => {
            tracing::warn!("copying the picture {name}: {e}");
            None
        }
    }
}

/// Forget a kept image.
pub(crate) fn delete_picture(name: &str) {
    if name.is_empty() || name.contains('/') {
        return;
    }
    PICTURES.with(|p| p.borrow_mut().remove(name));
    if let Err(e) = std::fs::remove_file(pictures_dir().join(name))
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!("removing the picture {name}: {e}");
    }
}

/// The middle square of an image, scaled down to `limit` pixels a side if
/// it is larger, each pixel the average of those it covers.
fn crop_square(texture: &gdk::Texture, limit: u32) -> gdk::Texture {
    let (w, h) = (texture.width() as usize, texture.height() as usize);
    let mut downloader = gdk::TextureDownloader::new(texture);
    downloader.set_format(gdk::MemoryFormat::R8g8b8a8Premultiplied);
    let (bytes, stride) = downloader.download_bytes();
    let side = w.min(h);
    let (left, top) = ((w - side) / 2, (h - side) / 2);
    let out = side.min(limit as usize).max(1);
    let mut pixels = vec![0u8; out * out * 4];
    for y in 0..out {
        let (y0, y1) = (
            top + y * side / out,
            top + ((y + 1) * side / out).max(y * side / out + 1),
        );
        for x in 0..out {
            let (x0, x1) = (
                left + x * side / out,
                left + ((x + 1) * side / out).max(x * side / out + 1),
            );
            let mut sum = [0u32; 4];
            for sy in y0..y1.min(h) {
                for sx in x0..x1.min(w) {
                    let at = sy * stride + sx * 4;
                    for (c, total) in sum.iter_mut().enumerate() {
                        *total += u32::from(bytes[at + c]);
                    }
                }
            }
            let count = ((y1.min(h) - y0) * (x1.min(w) - x0)).max(1) as u32;
            let at = (y * out + x) * 4;
            for c in 0..4 {
                pixels[at + c] = (sum[c] / count) as u8;
            }
        }
    }
    gdk::MemoryTexture::new(
        out as i32,
        out as i32,
        gdk::MemoryFormat::R8g8b8a8Premultiplied,
        &glib::Bytes::from_owned(pixels),
        out * 4,
    )
    .upcast()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seed_always_gives_the_same_robot() {
        assert_eq!(
            Robot::from_seed("Test Server"),
            Robot::from_seed("Test Server")
        );
    }

    #[test]
    fn an_image_is_cut_to_its_middle_square() {
        // 300 by 200: red left third, blue middle, green right third.
        let (w, h) = (300usize, 200usize);
        let mut pixels = Vec::with_capacity(w * h * 4);
        for _ in 0..h {
            for x in 0..w {
                pixels.extend_from_slice(match x {
                    0..100 => &[255, 0, 0, 255],
                    100..200 => &[0, 0, 255, 255],
                    _ => &[0, 255, 0, 255],
                });
            }
        }
        let texture: gdk::Texture = gdk::MemoryTexture::new(
            w as i32,
            h as i32,
            gdk::MemoryFormat::R8g8b8a8Premultiplied,
            &glib::Bytes::from_owned(pixels),
            w * 4,
        )
        .upcast();
        let square = crop_square(&texture, 100);
        assert_eq!((square.width(), square.height()), (100, 100));
        let mut downloader = gdk::TextureDownloader::new(&square);
        downloader.set_format(gdk::MemoryFormat::R8g8b8a8Premultiplied);
        let (bytes, stride) = downloader.download_bytes();
        // The middle 200 columns, from 50 to 250: half red, half blue, a
        // quarter of it green at the right.
        let at = |x: usize| &bytes[50 * stride + x * 4..50 * stride + x * 4 + 3];
        assert_eq!(at(10), &[255, 0, 0]);
        assert_eq!(at(50), &[0, 0, 255]);
        assert_eq!(at(90), &[0, 255, 0]);
    }

    #[test]
    fn seeds_spread_over_every_part() {
        let robots: Vec<Robot> = (0..400)
            .map(|i| Robot::from_seed(&format!("seed {i}")))
            .collect();
        let distinct = |part: fn(&Robot) -> u8, n: usize| {
            let seen: std::collections::HashSet<u8> = robots.iter().map(part).collect();
            assert_eq!(seen.len(), n);
        };
        distinct(|r| r.background, pixelbot::BACKGROUNDS.len());
        distinct(|r| r.glow, pixelbot::GLOWS.len());
        distinct(|r| r.eyes, pixelbot::EYES.len());
        distinct(|r| r.mouth, pixelbot::MOUTHS.len());
    }
}
