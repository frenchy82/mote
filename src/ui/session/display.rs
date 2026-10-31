//! The remote desktop on screen: frames drawn as one texture, scaled to fit
//! the widget, and the keyboard, pointer and wheel sent back as input.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib::subclass::prelude::*;
use gtk::prelude::*;
use gtk::subclass::widget::WidgetImpl;
use gtk::{gdk, glib, graphene, gsk};
use ironrdp_input::{Database, MouseButton, MousePosition, Operation, Scancode, WheelRotations};
use ironrdp_pdu::input::fast_path::FastPathInputEvent;

use crate::rdp::{self, keymap};

type InputSink = Rc<dyn Fn(Vec<FastPathInputEvent>)>;

mod imp {
    use super::*;

    #[derive(Default)]
    pub(crate) struct Display {
        pub(super) texture: RefCell<Option<gdk::Texture>>,
        pub(super) input: RefCell<Option<InputSink>>,
        /// What is held down on the server's side, so a key or button
        /// still down when focus leaves can be let go of.
        pub(super) state: RefCell<Database>,
        /// Scrolling not yet sent: wheel units come whole.
        pub(super) scroll: Cell<(f64, f64)>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Display {
        const NAME: &'static str = "MoteDisplay";
        type Type = super::Display;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Display {
        fn constructed(&self) {
            self.parent_constructed();
            let widget = self.obj();
            widget.set_focusable(true);
            widget.set_hexpand(true);
            widget.set_vexpand(true);
            widget.set_overflow(gtk::Overflow::Hidden);
            super::connect_input(&widget);
        }
    }

    impl WidgetImpl for Display {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let widget = self.obj();
            let (w, h) = (widget.width() as f32, widget.height() as f32);
            snapshot.append_color(&gdk::RGBA::BLACK, &graphene::Rect::new(0.0, 0.0, w, h));
            if let Some(texture) = self.texture.borrow().as_ref() {
                let fit = Fit::new(&*widget, texture);
                let bounds = graphene::Rect::new(
                    fit.x as f32,
                    fit.y as f32,
                    texture.width() as f32 * fit.scale as f32,
                    texture.height() as f32 * fit.scale as f32,
                );
                snapshot.append_scaled_texture(texture, gsk::ScalingFilter::Linear, &bounds);
            }
        }
    }
}

glib::wrapper! {
    pub(crate) struct Display(ObjectSubclass<imp::Display>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for Display {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl Display {
    /// Where input goes: to the session.
    pub(crate) fn set_input(&self, sink: impl Fn(Vec<FastPathInputEvent>) + 'static) {
        self.imp().input.replace(Some(Rc::new(sink)));
    }

    /// Show a frame. Only its damaged parts are uploaded again: the old
    /// texture is handed to GDK as the one the new one updates.
    pub(crate) fn show_frame(&self, frame: rdp::Frame) {
        let (width, height) = (i32::from(frame.width), i32::from(frame.height));
        let previous = self.imp().texture.borrow().clone();
        let bytes = glib::Bytes::from_owned(frame.pixels);
        let mut builder = gdk::MemoryTextureBuilder::new()
            .set_bytes(Some(&bytes))
            .set_width(width)
            .set_height(height)
            .set_stride(frame.stride)
            .set_format(gdk::MemoryFormat::B8g8r8x8);
        if let Some(previous) = previous.filter(|t| t.width() == width && t.height() == height) {
            let rects: Vec<_> = frame
                .damage
                .iter()
                .map(|r| gtk::cairo::RectangleInt::new(r.x, r.y, r.width, r.height))
                .collect();
            builder = builder
                .set_update_texture(Some(&previous))
                .set_update_region(Some(&gtk::cairo::Region::create_rectangles(&rects)));
        }
        self.imp().texture.replace(Some(builder.build()));
        self.queue_draw();
    }

    pub(crate) fn set_remote_cursor(&self, cursor: rdp::Cursor) {
        let cursor = match cursor {
            rdp::Cursor::Default => None,
            rdp::Cursor::Hidden => gdk::Cursor::from_name("none", None),
            rdp::Cursor::Bitmap(pointer) => {
                let (w, h) = (i32::from(pointer.width), i32::from(pointer.height));
                if w == 0 || h == 0 {
                    gdk::Cursor::from_name("none", None)
                } else {
                    let texture = gdk::MemoryTexture::new(
                        w,
                        h,
                        gdk::MemoryFormat::R8g8b8a8,
                        &glib::Bytes::from(&pointer.bitmap_data),
                        w as usize * 4,
                    );
                    Some(gdk::Cursor::from_texture(
                        &texture,
                        i32::from(pointer.hotspot_x),
                        i32::from(pointer.hotspot_y),
                        None,
                    ))
                }
            }
        };
        self.set_cursor(cursor.as_ref());
    }

    fn send(&self, events: impl IntoIterator<Item = FastPathInputEvent>) {
        let events: Vec<_> = events.into_iter().collect();
        if events.is_empty() {
            return;
        }
        if let Some(sink) = self.imp().input.borrow().clone() {
            sink(events);
        }
    }

    fn apply(&self, operation: Operation) {
        let events = self.imp().state.borrow_mut().apply([operation]);
        self.send(events);
    }

    /// The desktop position under a point of the widget.
    fn remote_point(&self, x: f64, y: f64) -> Option<MousePosition> {
        let texture = self.imp().texture.borrow().clone()?;
        let fit = Fit::new(self, &texture);
        let max_x = f64::from(texture.width() - 1);
        let max_y = f64::from(texture.height() - 1);
        Some(MousePosition {
            x: ((x - fit.x) / fit.scale).clamp(0.0, max_x) as u16,
            y: ((y - fit.y) / fit.scale).clamp(0.0, max_y) as u16,
        })
    }
}

/// How the desktop sits in the widget: scaled to fit, centered.
struct Fit {
    x: f64,
    y: f64,
    scale: f64,
}

impl Fit {
    fn new(widget: &impl IsA<gtk::Widget>, texture: &gdk::Texture) -> Self {
        let (w, h) = (f64::from(widget.width()), f64::from(widget.height()));
        let (tw, th) = (f64::from(texture.width()), f64::from(texture.height()));
        let scale = (w / tw).min(h / th);
        Self {
            x: (w - tw * scale) / 2.0,
            y: (h - th * scale) / 2.0,
            scale,
        }
    }
}

fn connect_input(display: &Display) {
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed(glib::clone!(
        #[weak]
        display,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, _, keycode, _| {
            if let Some(code) = scancode(keycode) {
                // The server repeats a held key itself.
                if !display.imp().state.borrow().is_key_pressed(code) {
                    display.apply(Operation::KeyPressed(code));
                }
            }
            // Every key is the session's, shortcuts included.
            glib::Propagation::Stop
        }
    ));
    keys.connect_key_released(glib::clone!(
        #[weak]
        display,
        move |_, _, keycode, _| {
            if let Some(code) = scancode(keycode) {
                display.apply(Operation::KeyReleased(code));
            }
        }
    ));
    display.add_controller(keys);

    let focus = gtk::EventControllerFocus::new();
    focus.connect_enter(glib::clone!(
        #[weak]
        display,
        move |_| display.sync_locks()
    ));
    // Keys let go of elsewhere would stay down on the server.
    focus.connect_leave(glib::clone!(
        #[weak]
        display,
        move |_| {
            let events = display.imp().state.borrow_mut().release_all();
            display.send(events);
        }
    ));
    display.add_controller(focus);

    let motion = gtk::EventControllerMotion::new();
    motion.connect_motion(glib::clone!(
        #[weak]
        display,
        move |_, x, y| {
            if let Some(position) = display.remote_point(x, y) {
                display.apply(Operation::MouseMove(position));
            }
        }
    ));
    display.add_controller(motion);

    // Buttons from the raw events: a click gesture follows one button at a
    // time, and a drag can hold two.
    let buttons = gtk::EventControllerLegacy::new();
    buttons.connect_event(glib::clone!(
        #[weak]
        display,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, event| {
            let pressed = match event.event_type() {
                gdk::EventType::ButtonPress => true,
                gdk::EventType::ButtonRelease => false,
                _ => return glib::Propagation::Proceed,
            };
            let Some(button) = event
                .downcast_ref::<gdk::ButtonEvent>()
                .and_then(|e| mouse_button(e.button()))
            else {
                return glib::Propagation::Proceed;
            };
            if pressed {
                display.grab_focus();
                display.apply(Operation::MouseButtonPressed(button));
            } else {
                display.apply(Operation::MouseButtonReleased(button));
            }
            glib::Propagation::Stop
        }
    ));
    display.add_controller(buttons);

    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
    scroll.connect_scroll(glib::clone!(
        #[weak]
        display,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |controller, dx, dy| {
            display.scroll(controller.unit(), dx, dy);
            glib::Propagation::Stop
        }
    ));
    display.add_controller(scroll);
}

impl Display {
    /// Send scrolling as wheel units, 120 to a notch. A touchpad's pixels
    /// count as GNOME counts them, 10 to a notch, and what doesn't make a
    /// whole unit waits for the next event.
    fn scroll(&self, unit: gdk::ScrollUnit, dx: f64, dy: f64) {
        let per = if unit == gdk::ScrollUnit::Wheel {
            120.0
        } else {
            12.0
        };
        let (mut ax, mut ay) = self.imp().scroll.get();
        // Down is negative on the wire, right positive.
        ax += dx * per;
        ay -= dy * per;
        let mut operations = Vec::new();
        for (amount, vertical) in [(&mut ay, true), (&mut ax, false)] {
            let whole = amount.trunc();
            if whole != 0.0 {
                *amount -= whole;
                operations.push(Operation::WheelRotations(WheelRotations {
                    is_vertical: vertical,
                    rotation_units: whole.clamp(-255.0, 255.0) as i16,
                }));
            }
        }
        self.imp().scroll.set((ax, ay));
        if !operations.is_empty() {
            let events = self.imp().state.borrow_mut().apply(operations);
            self.send(events);
        }
    }

    /// Match the server's Caps, Num and Scroll Lock to this keyboard's.
    fn sync_locks(&self) {
        let Some(keyboard) = self.display().default_seat().and_then(|s| s.keyboard()) else {
            return;
        };
        self.send([ironrdp_input::synchronize_event(
            keyboard.is_scroll_locked(),
            keyboard.is_num_locked(),
            keyboard.is_caps_locked(),
            false,
        )]);
    }
}

/// GTK's hardware keycode is the evdev code plus 8.
fn scancode(keycode: u32) -> Option<Scancode> {
    keymap::scancode(keycode.checked_sub(8)?).map(Scancode::from_u16)
}

fn mouse_button(button: u32) -> Option<MouseButton> {
    Some(match button {
        gdk::BUTTON_PRIMARY => MouseButton::Left,
        gdk::BUTTON_MIDDLE => MouseButton::Middle,
        gdk::BUTTON_SECONDARY => MouseButton::Right,
        8 => MouseButton::X1,
        9 => MouseButton::X2,
        _ => return None,
    })
}
