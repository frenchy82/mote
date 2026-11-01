//! The remote desktop on screen: frames drawn as one texture, placed as the
//! scaling mode says, and the keyboard, pointer and wheel sent back as input.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use gtk::glib::subclass::prelude::*;
use gtk::prelude::*;
use gtk::subclass::widget::{WidgetImpl, WidgetImplExt};
use gtk::{gdk, glib, graphene, gsk};
use ironrdp_graphics::pointer::DecodedPointer;
use ironrdp_input::{Database, MouseButton, MousePosition, Operation, Scancode, WheelRotations};
use ironrdp_pdu::input::fast_path::FastPathInputEvent;

use crate::connection::Scaling;
use crate::rdp::{self, keymap};

type InputSink = Rc<dyn Fn(Vec<FastPathInputEvent>)>;
type SizeSink = Rc<dyn Fn()>;
type GrabSink = Rc<dyn Fn()>;

mod imp {
    use super::*;

    #[derive(Default)]
    pub(crate) struct Display {
        pub(super) texture: RefCell<Option<gdk::Texture>>,
        pub(super) input: RefCell<Option<InputSink>>,
        /// Told when the widget's size or the screen's scale changes.
        pub(super) on_size: RefCell<Option<SizeSink>>,
        pub(super) mode: Cell<Scaling>,
        /// The server's pointer, kept to draw it again at a new scale.
        pub(super) pointer: RefCell<Option<Arc<DecodedPointer>>>,
        /// The surface whose scale is followed, and the handler doing it.
        pub(super) watched: RefCell<Option<(gdk::Surface, glib::SignalHandlerId)>>,
        /// What is held down on the server's side, so a key or button
        /// still down when focus leaves can be let go of.
        pub(super) state: RefCell<Database>,
        /// Scrolling not yet sent: wheel units come whole.
        pub(super) scroll: Cell<(f64, f64)>,
        /// Shown but not yet painted: when its oldest change was decoded.
        pub(super) unpainted: Cell<Option<std::time::Instant>>,
        /// The frame clock painted by, and the handler watching it, to time
        /// frames to the screen.
        pub(super) clock: RefCell<Option<(gdk::FrameClock, glib::SignalHandlerId)>>,
        pub(super) stats: RefCell<Option<crate::stats::Stats>>,
        /// Keyboard and pointer stay here; the desktop is only shown.
        pub(super) view_only: Cell<bool>,
        /// Where this monitor's part of a desktop spread over several
        /// starts, in desktop pixels; the whole desktop when `None`.
        pub(super) viewport: Cell<Option<(i32, i32)>>,
        /// Hold the desktop's own shortcuts back while focused.
        pub(super) grab: Cell<bool>,
        /// Told when the desktop's shortcuts are held back.
        pub(super) on_grabbed: RefCell<Option<GrabSink>>,
        /// The window's surface, and the handler watching its shortcuts.
        pub(super) inhibit: RefCell<Option<(gdk::Surface, glib::SignalHandlerId)>>,
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
        /// At Actual Size the desktop's size, so that a scrolled window can
        /// scroll it; otherwise nothing, to take whatever room there is.
        fn measure(&self, orientation: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            let widget = self.obj();
            let texture = self.texture.borrow();
            match (self.mode.get(), texture.as_ref()) {
                (Scaling::Actual, Some(texture)) if self.viewport.get().is_none() => {
                    let pixels = match orientation {
                        gtk::Orientation::Horizontal => texture.width(),
                        _ => texture.height(),
                    };
                    let size = (f64::from(pixels) / device_scale(&*widget)).ceil() as i32;
                    (size, size, -1, -1)
                }
                _ => (0, 0, -1, -1),
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(width, height, baseline);
            self.obj().size_changed();
        }

        fn realize(&self) {
            self.parent_realize();
            let widget = self.obj();
            let Some(surface) = widget.native().and_then(|n| n.surface()) else {
                return;
            };
            let handler = surface.connect_scale_notify(glib::clone!(
                #[weak]
                widget,
                move |_| {
                    // The window moved to a screen with another scale.
                    widget.queue_resize();
                    widget.size_changed();
                }
            ));
            if let Some(toplevel) = surface.downcast_ref::<gdk::Toplevel>() {
                let handler = toplevel.connect_shortcuts_inhibited_notify(glib::clone!(
                    #[weak]
                    widget,
                    move |toplevel| {
                        tracing::info!(
                            "this computer's shortcuts {}",
                            if toplevel.is_shortcuts_inhibited() {
                                "are held back"
                            } else {
                                "work again"
                            }
                        );
                        if toplevel.is_shortcuts_inhibited() {
                            let sink = widget.imp().on_grabbed.borrow().clone();
                            if let Some(sink) = sink {
                                sink();
                            }
                        }
                    }
                ));
                self.inhibit.replace(Some((surface.clone(), handler)));
            }
            self.watched.replace(Some((surface, handler)));
            if let Some(clock) = widget.frame_clock() {
                let handler = clock.connect_after_paint(glib::clone!(
                    #[weak]
                    widget,
                    move |_| widget.painted()
                ));
                self.clock.replace(Some((clock, handler)));
            }
        }

        fn unrealize(&self) {
            if let Some((surface, handler)) = self.watched.take() {
                surface.disconnect(handler);
            }
            if let Some((surface, handler)) = self.inhibit.take() {
                surface.disconnect(handler);
            }
            if let Some((clock, handler)) = self.clock.take() {
                clock.disconnect(handler);
            }
            self.parent_unrealize();
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let widget = self.obj();
            let (w, h) = (widget.width() as f32, widget.height() as f32);
            snapshot.append_color(&gdk::RGBA::BLACK, &graphene::Rect::new(0.0, 0.0, w, h));
            if let Some(texture) = self.texture.borrow().as_ref() {
                let place = Placement::new(&widget, texture);
                let bounds = graphene::Rect::new(
                    place.x as f32,
                    place.y as f32,
                    texture.width() as f32 * place.scale as f32,
                    texture.height() as f32 * place.scale as f32,
                );
                snapshot.append_scaled_texture(texture, place.filter, &bounds);
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

/// The screen's scale where the widget is, fractional ones included.
pub(super) fn device_scale(widget: &impl IsA<gtk::Widget>) -> f64 {
    widget
        .native()
        .and_then(|n| n.surface())
        .map(|s| s.scale())
        .filter(|s| *s > 0.0)
        .unwrap_or_else(|| f64::from(widget.scale_factor()))
}

impl Display {
    /// Where input goes: to the session.
    pub(crate) fn set_input(&self, sink: impl Fn(Vec<FastPathInputEvent>) + 'static) {
        self.imp().input.replace(Some(Rc::new(sink)));
    }

    /// Hold the desktop's own shortcuts back while the desktop has focus,
    /// so Super, Alt+Tab and the rest go to the session. GNOME asks the
    /// user the first time, and gives them back on Super+Esc.
    pub(crate) fn set_grab(&self, grab: bool) {
        self.imp().grab.set(grab);
        if self.has_focus() {
            self.hold_shortcuts(grab);
        }
    }

    /// Told when the desktop's shortcuts are held back.
    pub(crate) fn set_on_grabbed(&self, sink: impl Fn() + 'static) {
        self.imp().on_grabbed.replace(Some(Rc::new(sink)));
    }

    fn hold_shortcuts(&self, hold: bool) {
        let Some(toplevel) = self
            .native()
            .and_then(|n| n.surface())
            .and_downcast::<gdk::Toplevel>()
        else {
            return;
        };
        if hold {
            toplevel.inhibit_system_shortcuts(None::<&gdk::Event>);
        } else {
            toplevel.restore_system_shortcuts();
        }
    }

    pub(crate) fn view_only(&self) -> bool {
        self.imp().view_only.get()
    }

    /// Show the desktop without sending it keys or the pointer.
    pub(crate) fn set_view_only(&self, view_only: bool) {
        self.imp().view_only.set(view_only);
        if view_only {
            let events = self.imp().state.borrow_mut().release_all();
            self.send(events);
        }
    }

    /// Press the keys in order and let them go in reverse: a combination
    /// this computer would otherwise take for itself.
    pub(crate) fn send_keys(&self, codes: &[u16]) {
        if self.imp().view_only.get() {
            return;
        }
        let codes: Vec<Scancode> = codes.iter().map(|&c| Scancode::from_u16(c)).collect();
        let operations = codes
            .iter()
            .map(|&c| Operation::KeyPressed(c))
            .chain(codes.iter().rev().map(|&c| Operation::KeyReleased(c)));
        let events = self.imp().state.borrow_mut().apply(operations);
        self.send(events);
    }

    /// The desktop as last shown.
    pub(crate) fn texture(&self) -> Option<gdk::Texture> {
        self.imp().texture.borrow().clone()
    }

    /// Show only the part of the desktop from `start` on, one pixel to one:
    /// this monitor's part of a desktop spread over several.
    pub(crate) fn set_viewport(&self, start: Option<(i32, i32)>) {
        self.imp().viewport.set(start);
        self.queue_resize();
        self.queue_draw();
        self.refresh_cursor();
    }

    /// Show the desktop another display decoded: the same picture, here on
    /// another monitor.
    pub(crate) fn show_texture(&self, texture: gdk::Texture) {
        let resized = self
            .imp()
            .texture
            .replace(Some(texture.clone()))
            .is_none_or(|old| old.width() != texture.width() || old.height() != texture.height());
        if resized {
            self.refresh_cursor();
        }
        self.queue_draw();
    }

    /// The window painted: time how long the newest frame took to get here.
    fn painted(&self) {
        let Some(made) = self.imp().unpainted.take() else {
            return;
        };
        if let Some(stats) = self.imp().stats.borrow_mut().as_mut() {
            stats.time("to screen", made);
            stats.count("painted", 1);
        }
    }

    /// Report what has been measured, at the end of a session.
    pub(crate) fn report(&self) {
        if let Some(stats) = self.imp().stats.borrow_mut().as_mut() {
            stats.report();
        }
    }

    /// Told when the room for the desktop, or the screen's scale, changes.
    pub(crate) fn set_on_size(&self, sink: impl Fn() + 'static) {
        self.imp().on_size.replace(Some(Rc::new(sink)));
    }

    pub(crate) fn set_mode(&self, mode: Scaling) {
        self.imp().mode.set(mode);
        self.queue_resize();
        self.queue_draw();
        self.refresh_cursor();
    }

    fn size_changed(&self) {
        self.refresh_cursor();
        let sink = self.imp().on_size.borrow().clone();
        if let Some(sink) = sink {
            sink();
        }
    }

    /// Show a frame. Only its damaged parts are uploaded again: the old
    /// texture is handed to GDK as the one the new one updates.
    pub(crate) fn show_frame(&self, frame: rdp::Frame) {
        let start = std::time::Instant::now();
        // A frame replaced before it was painted: the older change counts.
        let unpainted = self.imp().unpainted.get().unwrap_or(frame.made);
        self.imp().unpainted.set(Some(unpainted.min(frame.made)));

        let (width, height) = (i32::from(frame.width), i32::from(frame.height));
        let previous = self.imp().texture.borrow().clone();
        let resized = previous
            .as_ref()
            .is_none_or(|t| t.width() != width || t.height() != height);
        let bytes = glib::Bytes::from_owned(frame.pixels);
        let mut builder = gdk::MemoryTextureBuilder::new()
            .set_bytes(Some(&bytes))
            .set_width(width)
            .set_height(height)
            .set_stride(frame.stride)
            .set_format(gdk::MemoryFormat::B8g8r8x8);
        if let Some(previous) = previous.filter(|_| !resized) {
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
        self.imp()
            .stats
            .borrow_mut()
            .get_or_insert_with(|| crate::stats::Stats::new("screen"))
            .time("texture", start);
        if resized {
            // A new desktop size: a new natural size at Actual Size, and a
            // new scale for the pointer.
            self.queue_resize();
            self.refresh_cursor();
        }
        self.queue_draw();
    }

    pub(crate) fn set_remote_cursor(&self, cursor: rdp::Cursor) {
        let cursor = match cursor {
            rdp::Cursor::Default => {
                self.imp().pointer.replace(None);
                None
            }
            rdp::Cursor::Hidden => {
                self.imp().pointer.replace(None);
                gdk::Cursor::from_name("none", None)
            }
            rdp::Cursor::Bitmap(pointer) => {
                self.imp().pointer.replace(Some(pointer));
                return self.refresh_cursor();
            }
        };
        self.set_cursor(cursor.as_ref());
    }

    /// Draw the server's pointer at the size the desktop is drawn at, so it
    /// is as sharp as the screen allows.
    fn refresh_cursor(&self) {
        let Some(pointer) = self.imp().pointer.borrow().clone() else {
            return;
        };
        let (w, h) = (i32::from(pointer.width), i32::from(pointer.height));
        if w == 0 || h == 0 {
            self.set_cursor(gdk::Cursor::from_name("none", None).as_ref());
            return;
        }
        let scale = self
            .imp()
            .texture
            .borrow()
            .as_ref()
            .map_or(1.0 / device_scale(self), |t| Placement::new(self, t).scale);
        let texture = gdk::MemoryTexture::new(
            w,
            h,
            gdk::MemoryFormat::R8g8b8a8,
            &glib::Bytes::from(&pointer.bitmap_data),
            w as usize * 4,
        );
        let logical = |pixels: u16| ((f64::from(pixels) * scale).round() as i32).max(1);
        let size = (logical(pointer.width), logical(pointer.height));
        let hotspot = (
            (f64::from(pointer.hotspot_x) * scale).round() as i32,
            (f64::from(pointer.hotspot_y) * scale).round() as i32,
        );
        let cursor = gdk::Cursor::from_callback(
            move |_, _, _, width, height, hotspot_x, hotspot_y| {
                (*width, *height) = size;
                (*hotspot_x, *hotspot_y) = hotspot;
                texture.clone().upcast()
            },
            None,
        );
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
        let place = Placement::new(self, &texture);
        let max_x = f64::from(texture.width() - 1);
        let max_y = f64::from(texture.height() - 1);
        Some(MousePosition {
            x: ((x - place.x) / place.scale).clamp(0.0, max_x) as u16,
            y: ((y - place.y) / place.scale).clamp(0.0, max_y) as u16,
        })
    }
}

/// Where the desktop sits in the widget, and at what scale: widget pixels
/// to a desktop pixel.
struct Placement {
    x: f64,
    y: f64,
    scale: f64,
    filter: gsk::ScalingFilter,
}

impl Placement {
    fn new(widget: &Display, texture: &gdk::Texture) -> Self {
        let device = device_scale(widget);
        // One monitor's part of a desktop spread over several: one pixel to
        // one, from where its part starts.
        if let Some((x, y)) = widget.imp().viewport.get() {
            return Self {
                x: -f64::from(x) / device,
                y: -f64::from(y) / device,
                scale: 1.0 / device,
                filter: gsk::ScalingFilter::Nearest,
            };
        }
        let mode = widget.imp().mode.get();
        let (w, h) = (f64::from(widget.width()), f64::from(widget.height()));
        let (tw, th) = (f64::from(texture.width()), f64::from(texture.height()));
        let fit = (w / tw).min(h / th);
        // One desktop pixel to one screen pixel.
        let actual = 1.0 / device;
        let scale = match mode {
            Scaling::Fit => fit,
            // Until the server catches up with a resize, shrink to fit a
            // desktop that is too large, but never enlarge one.
            Scaling::Resize => actual.min(fit),
            Scaling::Actual => actual,
        };
        // Centered, and on whole screen pixels, so 1:1 stays sharp.
        let align =
            |room: f64, used: f64| (((room - used) / 2.0).max(0.0) * device).floor() / device;
        let sharp = (scale * device - 1.0).abs() < 1e-6;
        Self {
            x: align(w, tw * scale),
            y: align(h, th * scale),
            scale,
            filter: if sharp {
                gsk::ScalingFilter::Nearest
            } else {
                gsk::ScalingFilter::Linear
            },
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
            if display.imp().view_only.get() {
                // Kept from this computer's shortcuts all the same.
                return glib::Propagation::Stop;
            }
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
            if let Some(code) = scancode(keycode).filter(|_| !display.imp().view_only.get()) {
                display.apply(Operation::KeyReleased(code));
            }
        }
    ));
    display.add_controller(keys);

    let focus = gtk::EventControllerFocus::new();
    focus.connect_enter(glib::clone!(
        #[weak]
        display,
        move |_| {
            display.sync_locks();
            if display.imp().grab.get() {
                display.hold_shortcuts(true);
            }
        }
    ));
    // Keys let go of elsewhere would stay down on the server.
    focus.connect_leave(glib::clone!(
        #[weak]
        display,
        move |_| {
            let events = display.imp().state.borrow_mut().release_all();
            display.send(events);
            display.hold_shortcuts(false);
        }
    ));
    display.add_controller(focus);

    let motion = gtk::EventControllerMotion::new();
    motion.connect_motion(glib::clone!(
        #[weak]
        display,
        move |_, x, y| {
            if display.imp().view_only.get() {
                return;
            }
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
            }
            if display.imp().view_only.get() {
                return glib::Propagation::Stop;
            }
            if pressed {
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
            if !display.imp().view_only.get() {
                display.scroll(controller.unit(), dx, dy);
            }
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
        if self.imp().view_only.get() {
            return;
        }
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
