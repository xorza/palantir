//! [`Harness`]: one window with no desktop, on a stepped clock, which a unit test, an app's UI
//! test and an agent drive alike. A host beside [`WinitHost`](crate::WinitHost) and
//! [`OffscreenHost`](crate::OffscreenHost), over the same core and window frames, so its frames
//! run the stamp, display, damage and screenshot path the desktop runs.
//!
//! # The one fact everything follows from
//!
//! A frame calls the record closure one, two or three times, each seeing different input:
//!
//! | pass | when it runs | what it sees |
//! |---|---|---|
//! | `Warmup` | the very first frame of a cold recorder | No input. Builds the cascade so pass A has something to hit-test. |
//! | `First`, or A | always, except on a paint-only frame | The real input, including one-frame edges (`clicked`, `drag.started()`). |
//! | `Second`, or B | pass A acted or asked a relayout | The edges are gone. At most one retry. |
//!
//! A paint-only frame runs no record pass. The harness keeps no copy of the pointer position or
//! the modifiers, which would part from the window's once an event goes in through
//! [`Harness::on_input`].
//!
//! # The rules
//!
//! 1. **The recorder starts warm**, so frame 1 runs no warmup pass.
//! 2. **Prime before reading.** A locator and a response read the last frame: an input assertion
//!    needs one frame before it, a stable arranged rect two ([`Harness::prime`]).
//! 3. **Read a response inside the record** ([`Harness::response_in`]).
//! 4. **Read the first record pass.** Pass B has no edges, so [`Harness::frame_value`] keeps
//!    pass A's value.
//! 5. **The closure's side effects run once per pass too.** Anything that adds up across frames
//!    reads pass A alone ([`Ui::record_pass`](crate::Ui::record_pass)) or is idempotent.
//! 6. **One clock, two doors.** [`Harness::advance`] and [`Harness::at`] move the time that
//!    stamps both frames and input.
//! 7. **A frozen clock makes every click one run of clicks**: the double-click interval is 500 ms
//!    and 5 px, so [`Harness::advance_past_double_click`] goes between separate clicks.
//!    [`AgentWindow::play`](crate::AgentWindow::play) waits it out by itself.
//! 8. **Animation time is not the frame clock.** An animation's step is the frame's delta clamped
//!    to 0.1 s, so a long wait moves it no further than one step; it moves through
//!    [`Harness::advance_frames`].
//! 9. **A frame can run no record pass.** A focused `TextEdit`'s caret blink keeps paint-only
//!    frames coming for 30 s, and [`Harness::frame_value`] panics on one.
//! 10. **The size is physical; pointer positions are logical** (`physical / scale`).
//! 11. **Keyboard input with nothing focused is dropped**, unless a shortcut subscription takes
//!     it. Focus first.
//! 12. **One change of a kind per frame.** A second press or release of one button, or a key
//!     press after the frame's command key, waits for the next frame, so [`Harness::click`] is a
//!     press frame then a release frame. [`Harness::frame`] runs the frames held input owes first
//!     and answers from the last; [`Harness::step`] runs one.
//!
//! The input methods are a test's: each queues input, which the next frame plays, and a locator
//! that names no one place panics. [`AgentWindow::play`](crate::AgentWindow::play), through [`Harness::session`], is a person's: it settles the window after
//! each step and returns an error an agent can read.

pub(crate) mod harness_builder;
pub(crate) mod harness_session;

use crate::common::time::MAX_ANIMATION_DT;
use crate::display::system_scale::SystemScale;
use crate::driver::error::PlayError;
use crate::driver::locator::Locator;
use crate::driver::play::{self, BUSY_SETTLE_WAITS};
use crate::gpu::device::gpu::Gpu;
use crate::gpu::device::gpu_wait::GpuWait;
use crate::gpu::surface::owned_target::OwnedTarget;
use crate::gpu::surface::render_target::TargetFormat;
use crate::host::harness::harness_builder::{HarnessBuilder, SettlePace, TARGET_LABEL};
use crate::host::harness::harness_session::HarnessSession;
use crate::host::headless_window::{HeadlessCanvas, HeadlessWindow};
use crate::host::host_device::HostDevice;
use crate::input::capture::DRAG_THRESHOLD;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::input_event::InputEvent;
use crate::primitives::input::interaction::input_delta::InputDelta;
use crate::primitives::input::interaction::response::Response;
use crate::primitives::input::keyboard::key::Key;
use crate::primitives::input::keyboard::modifiers::Modifiers;
use crate::primitives::input::pointer::PointerButton;
use crate::primitives::input::sense::Sense;
use crate::primitives::input::shortcut::{Shortcut, ShortcutMods};
use crate::primitives::input::zoom_factor::ZoomFactor;
use crate::primitives::reduction::Reduction;
use crate::theme::color_scheme::ColorScheme;
use crate::ui::Ui;
use crate::ui::app::{App, RecordPass};
use crate::ui::frame_report::FrameReport;
use crate::ui::inspected_widget::InspectedWidget;
use crate::ui::window_frames::present::{FrameSpec, TargetKey};
use crate::window::window_commands::WindowCommand;
use crate::window::window_token::WindowToken;
use glam::{UVec2, Vec2};
use std::time::{Duration, Instant};

/// The ambient font size this crate's unit tests frame at, pinned so the default text size can
/// move without their hand-computed text metrics moving with it, as the visual suite pins its
/// palette. A harness outside those tests keeps the stock theme, as a host does.
#[cfg(test)]
const FIXTURE_FONT_SIZE: f32 = 16.0;

/// One window with no desktop: its frames, its input and its clock, stepped by the caller. See
/// the module doc for the rules a frame follows.
///
/// ```
/// use palantir::{Button, Configure, Harness, Ui, WidgetId};
/// use palantir::glam::UVec2;
///
/// let mut h = Harness::new(UVec2::new(200, 100));
/// let mut clicks = 0;
/// let mut screen = |ui: &mut Ui| {
///     if Button::new().id(WidgetId::from_hash("inc")).label("click me").show(ui).clicked() {
///         clicks += 1;
///     }
/// };
/// h.frame(&mut screen);
/// h.click("click me");
/// h.frame(&mut screen);
/// assert_eq!(clicks, 1);
/// ```
#[derive(Debug)]
#[must_use]
pub struct Harness {
    window: HeadlessWindow,
    /// How far frames run, and the window's size, held by its texture where it paints.
    paint: HarnessPaint,
    /// Moves only on [`Self::advance`] and [`Self::at`], and the frames a [session](Self::session)'s settle and
    /// play run.
    time: Duration,
    /// Each button pressed and not released, with the press position; written only in
    /// [`Self::on_input`].
    held: [Option<PressOrigin>; PointerButton::COUNT],
    /// The time of the frame after a session's last press, which its next press waits out
    /// the double-click interval from.
    last_press: Option<Duration>,
    pace: SettlePace,
    /// Every change the frames made to a window, oldest first ([`log_window_command`]).
    window_commands: Vec<WindowCommand>,
}

/// How far a harness's frames run, with the window's size: kept by the texture of a window that
/// paints, beside the device it paints with, so the size, the paint and the device are one fact.
/// Refused: the depth, an optional target and the core's optional device stored apart, which
/// each check read a different one of.
#[derive(Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "one per harness, held in place: a box would cost a pointer chase and save nothing"
)]
enum HarnessPaint {
    /// Record through damage, and build no draw list.
    Record { size: UVec2 },
    /// Build the draw list too, and paint nothing.
    Compose { size: UVec2 },
    /// Paint into `target` on `device`: one target, kept between frames, so a still frame
    /// paints nothing.
    Gpu {
        target: OwnedTarget,
        device: HostDevice,
    },
}

impl HarnessPaint {
    const fn paints(&self) -> bool {
        matches!(self, Self::Gpu { .. })
    }

    /// The target the window's frames are laid out for: its texture where it paints, else an
    /// 8-bit sRGB one of its size that copies.
    fn target_key(&self) -> TargetKey {
        match self {
            Self::Record { size } | Self::Compose { size } => Self::unpainted(*size),
            Self::Gpu { target, .. } => TargetKey::of(target.as_target()),
        }
    }

    const fn unpainted(size: UVec2) -> TargetKey {
        TargetKey {
            physical: size,
            format: TargetFormat::RGBA8_SRGB,
            gives_copy: true,
        }
    }

    const fn target_mut(&mut self) -> Option<&mut OwnedTarget> {
        match self {
            Self::Record { .. } | Self::Compose { .. } => None,
            Self::Gpu { target, .. } => Some(target),
        }
    }
}

/// Where a held button went down; `None` when the pointer was off the window.
#[derive(Clone, Copy, Debug)]
struct PressOrigin {
    at: Option<Vec2>,
}

impl Harness {
    /// The token of a harness's one window, as [`App::record`] receives it.
    pub const WINDOW: WindowToken = WindowToken(0);

    /// A window of `size` physical pixels at scale 1 that records and builds no draw list,
    /// shaping with the bundled faces.
    pub fn new(size: UVec2) -> Self {
        Self::builder(size).build()
    }

    /// Start building a window of `size` physical pixels.
    pub const fn builder(size: UVec2) -> HarnessBuilder {
        HarnessBuilder::new(size)
    }

    fn open(window: HeadlessWindow, paint: HarnessPaint, pace: SettlePace) -> Self {
        let mut harness = Self {
            window,
            paint,
            time: Duration::ZERO,
            held: [None; PointerButton::COUNT],
            last_press: None,
            pace,
            window_commands: Vec::new(),
        };
        #[cfg(test)]
        {
            harness.ui_mut().theme_mut().text.font_size = FIXTURE_FONT_SIZE;
        }
        harness.sync_display();
        harness.mark_warm();
        harness
    }

    /// Run one frame of `record`, which runs once, twice or not at all. Runs the frames held
    /// input still owes first (rule 12).
    pub fn frame(&mut self, mut record: impl FnMut(&mut Ui)) -> FrameReport {
        self.frame_app(&mut record)
    }

    /// Run one frame of a whole [`App`], as a host does, after the frames held input owes.
    pub fn frame_app(&mut self, app: &mut impl App) -> FrameReport {
        self.deliver_held_input(app);
        self.drive(app)
    }

    /// Exactly one frame, delivering nothing held.
    pub fn step(&mut self, mut record: impl FnMut(&mut Ui)) -> FrameReport {
        self.drive(&mut record)
    }

    /// The value pass A of one frame of `record` returned.
    ///
    /// # Panics
    ///
    /// When the frame ran no record pass (rule 9).
    pub fn frame_value<R>(&mut self, mut record: impl FnMut(&mut Ui) -> R) -> R {
        self.deliver_held_input(&mut |ui: &mut Ui| {
            record(ui);
        });
        let mut first = None;
        self.step(|ui| {
            // `record` runs on every pass: skipping it would record an empty tree.
            let value = record(ui);
            if ui.record_pass() == RecordPass::First {
                first = Some(value);
            }
        });
        first.expect(
            "the frame ran no record pass: a paint animation was its only cause, as a focused \
             TextEdit's caret blink is. Feed an input or request a repaint first.",
        )
    }

    /// `id`'s response, read inside pass A: the one way to read a one-frame edge.
    pub fn response_in(&mut self, id: WidgetId, mut record: impl FnMut(&mut Ui)) -> Response {
        self.frame_value(move |ui| {
            record(ui);
            ui.response_for(id)
        })
    }

    /// `frames` frames of `record` whose reports go unread; two is the usual least.
    pub fn prime(&mut self, frames: u32, mut record: impl FnMut(&mut Ui)) {
        for _ in 0..frames {
            self.frame(&mut record);
        }
    }

    /// Move the clock forward by `dt`; input fed after it and the next frame carry the new time.
    /// An animation moves through [`Self::advance_frames`].
    pub fn advance(&mut self, dt: Duration) -> &mut Self {
        self.time += dt;
        self
    }

    /// Park the clock at `time`.
    pub const fn at(&mut self, time: Duration) -> &mut Self {
        self.time = time;
        self
    }

    /// `frames` frames of `record`, the clock moving `dt` before each: the way an animation
    /// moves.
    ///
    /// # Panics
    ///
    /// When `dt` is past the longest step an animation integrates, 0.1 s, which would end it
    /// short with no sign.
    pub fn advance_frames(&mut self, frames: u32, dt: Duration, mut record: impl FnMut(&mut Ui)) {
        Self::assert_frame_dt(dt);
        for _ in 0..frames {
            self.advance(dt);
            self.frame(&mut record);
        }
    }

    /// Move the clock past the double-click interval, so the next press starts a new run of
    /// clicks. Runs no frame.
    pub fn advance_past_double_click(&mut self) -> &mut Self {
        let interval = self.ui().double_click_interval();
        self.advance(interval + Duration::from_millis(1))
    }

    /// Give the window `size` physical pixels from the next frame on, which reads the change as a
    /// resize. At [`Depth::Gpu`](crate::Depth::Gpu) a new texture holds it, and the frame paints
    /// whole.
    ///
    /// # Panics
    ///
    /// At [`Depth::Gpu`](crate::Depth::Gpu), when either side is zero or past the device's
    /// largest 2D texture.
    pub fn resize(&mut self, size: UVec2) -> &mut Self {
        match &mut self.paint {
            HarnessPaint::Record { size: held } | HarnessPaint::Compose { size: held } => {
                *held = size;
            }
            HarnessPaint::Gpu { target, .. } => {
                if target.size() != size {
                    *target = target.resized(TARGET_LABEL, size);
                }
            }
        }
        self.sync_display();
        self
    }

    /// Report `scale` physical pixels per logical pixel from the next frame on, as a move to a
    /// monitor of another density does; the window keeps its physical size, which
    /// [`Self::resize`] moves. The app's own scale stays on top.
    pub fn set_system_scale(&mut self, scale: SystemScale) -> &mut Self {
        self.window.system_scale = scale;
        self.sync_display();
        self
    }

    /// Report the monitor's refresh rate, which repaint wakes coalesce to, from the next frame
    /// on; `None` reports none.
    pub fn set_refresh_millihertz(&mut self, millihertz: Option<u32>) -> &mut Self {
        self.window.refresh_millihertz = millihertz;
        self.sync_display();
        self
    }

    /// Report `scheme` as the system's, as the winit host reports the desktop's; `None`, the
    /// start, reports nothing, so [`Preference::System`](crate::Preference::System)
    /// shows the fallback. The next frame shows the resolved scheme.
    pub fn set_system_color_scheme(&mut self, scheme: Option<ColorScheme>) -> &mut Self {
        self.window
            .set_preferences(|preferences| preferences.color_scheme = scheme);
        self
    }

    /// Report the system as asking for less motion, or not, as the winit host reports the
    /// desktop's; `false` at the start. See [`Ui::is_motion_reduced`].
    pub fn set_system_motion_reduced(&mut self, reduced: bool) -> &mut Self {
        self.window
            .set_preferences(|preferences| preferences.motion = Reduction::of(reduced));
        self
    }

    /// Report the system as asking for less transparency, or not, as the winit host reports the
    /// desktop's; `false` at the start. See [`Ui::is_transparency_reduced`].
    pub fn set_system_transparency_reduced(&mut self, reduced: bool) -> &mut Self {
        self.window
            .set_preferences(|preferences| preferences.transparency = Reduction::of(reduced));
        self
    }

    /// The raw input door, for an event no method below sends.
    pub fn on_input(&mut self, event: InputEvent<'_>) -> InputDelta {
        match event {
            InputEvent::PointerPressed(button) => {
                assert!(
                    self.held[button.idx()].is_none(),
                    "{button:?} is already held; release it before pressing it again",
                );
                self.held[button.idx()] = Some(PressOrigin {
                    at: self.ui().input().peek_pointer_pos(),
                });
            }
            InputEvent::PointerReleased(button) => self.held[button.idx()] = None,
            _ => {}
        }
        self.window.frames.on_input(event, self.time)
    }

    /// Move the pointer to `at`.
    ///
    /// # Panics
    ///
    /// When `at` names no one place in the last frame.
    #[track_caller]
    pub fn hover(&mut self, at: impl Into<Locator>) -> InputDelta {
        let point = self.aim(&at.into());
        self.on_input(InputEvent::PointerMoved(point))
    }

    /// Move the pointer to `at` and press `button` there.
    ///
    /// # Panics
    ///
    /// As [`Self::hover`], and when `button` is held already.
    #[track_caller]
    pub fn press(&mut self, at: impl Into<Locator>, button: PointerButton) -> InputDelta {
        self.hover(at);
        self.on_input(InputEvent::PointerPressed(button))
    }

    /// Release `button` where the pointer is.
    pub fn release(&mut self, button: PointerButton) -> InputDelta {
        self.on_input(InputEvent::PointerReleased(button))
    }

    /// Press and release the left button at `at`: a press frame, then a release frame.
    ///
    /// # Panics
    ///
    /// As [`Self::press`].
    #[track_caller]
    pub fn click(&mut self, at: impl Into<Locator>) -> InputDelta {
        let pressed = self.press(at, PointerButton::Left);
        let released = self.release(PointerButton::Left);
        either(pressed, released)
    }

    /// Two left clicks at `at` as one run of clicks: a frame each for two presses and two releases.
    /// On a clock that holds still they are a double click; the second lands where the first did,
    /// with no second look at `at`.
    ///
    /// # Panics
    ///
    /// As [`Self::press`].
    #[track_caller]
    pub fn double_click(&mut self, at: impl Into<Locator>) -> InputDelta {
        let first = self.click(at);
        let pressed = self.on_input(InputEvent::PointerPressed(PointerButton::Left));
        let released = self.release(PointerButton::Left);
        either(either(first, pressed), released)
    }

    /// [`Self::click`] with the right button.
    ///
    /// # Panics
    ///
    /// As [`Self::press`].
    #[track_caller]
    pub fn right_click(&mut self, at: impl Into<Locator>) -> InputDelta {
        let pressed = self.press(at, PointerButton::Right);
        let released = self.release(PointerButton::Right);
        either(pressed, released)
    }

    /// Move the pointer to `at` with a button held.
    ///
    /// # Panics
    ///
    /// As [`Self::hover`], when no button is held or it went down off the window, and when the
    /// move from the press is shorter than the drag threshold, so no drag would start.
    #[track_caller]
    pub fn drag_to(&mut self, at: impl Into<Locator>) -> InputDelta {
        let point = self.aim(&at.into());
        let origin = self
            .held
            .iter()
            .find_map(|held| *held)
            .expect("drag_to needs a press first; no button is held")
            .at
            .expect("drag_to needs a press on the window; the pointer was off it");
        let travel = origin.distance(point);
        assert!(
            travel >= DRAG_THRESHOLD,
            "drag_to({point:?}) moves {travel} px from the press at {origin:?}, under the \
             {DRAG_THRESHOLD} px drag threshold; no drag would start",
        );
        self.on_input(InputEvent::PointerMoved(point))
    }

    /// Move the pointer to `at` and scroll by `delta` logical pixels, as a touchpad does;
    /// positive `y` scrolls content down.
    ///
    /// # Panics
    ///
    /// As [`Self::hover`].
    #[track_caller]
    pub fn scroll_pixels(&mut self, at: impl Into<Locator>, delta: Vec2) -> InputDelta {
        self.hover(at);
        self.on_input(InputEvent::ScrollPixels(delta))
    }

    /// Move the pointer to `at` and scroll by `delta` lines, as a notched wheel does; positive
    /// `y` scrolls content down.
    ///
    /// # Panics
    ///
    /// As [`Self::hover`].
    #[track_caller]
    pub fn scroll_lines(&mut self, at: impl Into<Locator>, delta: Vec2) -> InputDelta {
        self.hover(at);
        self.on_input(InputEvent::scroll_lines(delta))
    }

    /// Move the pointer to `at` and pinch by `factor`: above 1 zooms in.
    ///
    /// # Panics
    ///
    /// As [`Self::hover`].
    #[track_caller]
    pub fn pinch(&mut self, at: impl Into<Locator>, factor: ZoomFactor) -> InputDelta {
        self.hover(at);
        self.on_input(InputEvent::Pinch(factor))
    }

    /// Press and release a key in one frame, a tap: [`Self::key_down`], then [`Self::key_up`].
    pub fn key(&mut self, key: impl Into<Shortcut>) -> InputDelta {
        let shortcut = key.into();
        let down = self.key_down(shortcut);
        let up = self.key_up(shortcut);
        either(down, up)
    }

    /// Press a key, typing what it types, and leave it down until [`Self::key_up`]. A shortcut
    /// with modifiers holds them for the press and then restores the ones held before; a bare key
    /// presses under the modifiers held ([`Self::hold_modifiers`]).
    pub fn key_down(&mut self, key: impl Into<Shortcut>) -> InputDelta {
        let shortcut = key.into();
        self.under_mods(shortcut.mods, |h| h.on_input(shortcut.key_down()))
    }

    /// Release a key. A shortcut with modifiers holds them for the release and then restores the
    /// ones held before, as [`Self::key_down`] does for the press.
    pub fn key_up(&mut self, key: impl Into<Shortcut>) -> InputDelta {
        let shortcut = key.into();
        self.under_mods(shortcut.mods, |h| {
            h.on_input(InputEvent::KeyUp { key: shortcut.key })
        })
    }

    /// Run `send` with `mods` held, then restore the modifiers held before; with no modifiers,
    /// under the ones held.
    fn under_mods(
        &mut self,
        mods: ShortcutMods,
        send: impl FnOnce(&mut Self) -> InputDelta,
    ) -> InputDelta {
        if mods == ShortcutMods::NONE {
            return send(self);
        }
        let held = self.ui().input().modifiers_after_held_input();
        self.hold_modifiers(mods.held());
        let delta = send(self);
        self.hold_modifiers(held);
        delta
    }

    /// Hold `mods` from now on. Sends nothing when they are held already, counting input that
    /// waits for a later frame.
    pub fn hold_modifiers(&mut self, mods: Modifiers) -> InputDelta {
        if self.ui().input().modifiers_after_held_input() == mods {
            InputDelta::default()
        } else {
            self.on_input(InputEvent::ModifiersChanged(mods))
        }
    }

    /// Type `text` as key taps, one a character, each typing it.
    pub fn type_text(&mut self, text: &str) -> InputDelta {
        text.chars().fold(InputDelta::default(), |delta, c| {
            either(delta, self.key(Key::Char(c)))
        })
    }

    /// Commit `text` to whatever holds the keyboard focus, as an input method commits it: no key
    /// event.
    pub fn insert_text(&mut self, text: &str) -> InputDelta {
        self.on_input(InputEvent::ImeCommit(text))
    }

    /// This harness with `app`, driven through the [`AgentWindow`](crate::AgentWindow) verbs a
    /// [`Remote`](crate::Remote) offers on a live window: play, settle, shot, read, query, find and
    /// widgets.
    pub const fn session<'h, A>(&'h mut self, app: &'h mut A) -> HarnessSession<'h, A> {
        HarnessSession { harness: self, app }
    }

    /// The visible interactive widgets of the last frame whose text holds `filter`, ignoring
    /// case, in paint order: what an agent reads to pick a locator. Needs no app, so a test with a
    /// closure app reads it here; a [session](Self::session) lists the same through
    /// [`AgentWindow::widgets`](crate::AgentWindow::widgets).
    pub fn widgets(&self, filter: Option<&str>) -> Vec<InspectedWidget> {
        InspectedWidget::visible(self.ui(), filter)
    }

    /// `id`'s visible rect in the last frame: after transforms and clips, what the pointer hits.
    /// [`Self::arranged`] is the rect a layout test means; the two differ only under a
    /// transform or a clip.
    pub fn rect(&self, id: WidgetId) -> Option<Rect> {
        self.ui().response_for(id).rect
    }

    /// `id`'s arranged rect in the last frame: before transforms, not clipped; `None` when `id`
    /// did not arrange in it.
    pub fn arranged(&self, id: WidgetId) -> Option<Rect> {
        self.ui().response_for(id).layout_rect
    }

    /// The window point of `local`, a point in `id`'s arranged space, through every ancestor
    /// transform.
    ///
    /// # Panics
    ///
    /// When `id` did not arrange in the last frame, or another widget takes at that point the
    /// input `id` acts on: a press for a widget that clicks or drags, a hover for one that only
    /// hovers.
    #[track_caller]
    pub fn point_in(&self, id: WidgetId, local: Vec2) -> Vec2 {
        let response = self.ui().response_for(id);
        let layout = response.layout_rect.unwrap_or_else(|| {
            panic!("{id:?} has no arranged rect: it did not record, or no frame ran")
        });
        let pos = response.transform.apply_point(layout.min + local);
        let cascade = self.ui().cascade();
        let routing = cascade
            .hit_rows()
            .iter()
            .find(|row| row.widget_id == id)
            .and_then(|row| row.sense.routing());
        if let Some(routes) = routing {
            let hit = cascade.hit_test(pos, routes);
            assert_eq!(
                hit,
                Some(id),
                "{id:?} does not take the pointer at {pos:?}; {hit:?} is on top there",
            );
        }
        pos
    }

    /// The topmost widget a hover at `pos` reaches.
    pub fn hit_at(&self, pos: Vec2) -> Option<WidgetId> {
        self.ui().cascade().hit_test(pos, Sense::hovers)
    }

    /// `id`'s state row of type `S`, as [`Ui::state`] reads it; `None` when there is no such
    /// row.
    pub fn state<S: 'static>(&self, id: WidgetId) -> Option<&S> {
        self.ui().state::<S>(id)
    }

    /// Every window the frames opened or closed, oldest first: a harness has one window, and
    /// records the rest. An open of a window the log holds open is not recorded, as a host ignores
    /// it: an app that opens a window each frame, as one does here, where only the harness's own
    /// window is open, records one open. Every close is recorded: the harness's own window can
    /// close and come back with no open, as an app launched again in it does, and close again.
    pub fn window_commands(&self) -> &[WindowCommand] {
        &self.window_commands
    }

    /// The text on the window's clipboard, `None` when it holds none.
    ///
    /// # Panics
    ///
    /// When no clipboard answers; the harness's own, in memory, always does.
    pub fn clipboard_text(&self) -> Option<String> {
        self.ui()
            .clipboard()
            .text()
            .expect("the harness clipboard is in memory")
    }

    /// Put `text` on the window's clipboard, for a paste to come.
    pub fn set_clipboard_text(&mut self, text: &str) -> &mut Self {
        self.ui()
            .clipboard()
            .set_text(text)
            .expect("the harness clipboard is in memory");
        self
    }

    /// Block until the device finished every frame painted so far, and the copies of their
    /// screenshots are back, so a screenshot asked lands in the next frame. Returns how the wait
    /// ended: [`GpuWait::Done`], or why the copies did not come back. Below
    /// [`Depth::Gpu`](crate::Depth::Gpu) nothing paints, and it is done at once.
    pub fn wait_for_gpu(&self) -> GpuWait {
        match &self.paint {
            HarnessPaint::Record { .. } | HarnessPaint::Compose { .. } => GpuWait::Done,
            HarnessPaint::Gpu { device, .. } => device.wait_for_gpu(&self.window.frames),
        }
    }

    /// Block until a [`Waker`](crate::Waker) of this window wakes it, or `timeout` passes, in
    /// real time; whether a wake waits for the next frame. For a caller that waits on work it
    /// knows of, past what [a session's settle](crate::AgentWindow::settle) waits for.
    pub fn wait_for_wake(&self, timeout: Duration) -> bool {
        self.window.wait_for_wake(timeout)
    }

    /// Whether a [`Waker`](crate::Waker) of this window woke it since the last frame, which the
    /// next frame takes.
    pub fn has_pending_wake(&self) -> bool {
        self.window.is_wake_pending()
    }

    /// Whether the device this harness paints with is lost, as
    /// [`OffscreenHost::is_device_lost`](crate::OffscreenHost::is_device_lost) reads it; never
    /// below [`Depth::Gpu`](crate::Depth::Gpu). A frame then runs the app and paints nothing
    /// until [`Self::recover`] hands the harness a new device.
    pub fn is_device_lost(&self) -> bool {
        match &self.paint {
            HarnessPaint::Record { .. } | HarnessPaint::Compose { .. } => false,
            HarnessPaint::Gpu { device, .. } => device.gpu().is_lost(),
        }
    }

    /// Paint with `gpu` from now on, in place of a lost device, as
    /// [`OffscreenHost::recover`](crate::OffscreenHost::recover) does: every GPU object the
    /// harness holds is built again on it, the window's target included at its size and format,
    /// and the UI state stays.
    ///
    /// # Panics
    ///
    /// Below [`Depth::Gpu`](crate::Depth::Gpu), which paints with no device, and if `gpu` cannot
    /// run Palantir's pipelines, as [`HarnessBuilder::build`] says.
    pub fn recover(&mut self, gpu: Gpu) {
        let HarnessPaint::Gpu { target, device } = &mut self.paint else {
            panic!("a harness below Depth::Gpu paints with no device to recover");
        };
        *target = OwnedTarget::window(
            &gpu,
            TARGET_LABEL,
            target.size(),
            target.as_target().format(),
        );
        self.window.recover(device, gpu);
    }

    /// Render one frame of `app` at the clock as it stands, where [`Self::at`] parks it, and write
    /// the window's pixels into `pixels` in the target's format
    /// ([`Depth::Gpu`](crate::Depth::Gpu)'s `format`): rows top down with no
    /// padding, 4 bytes a pixel of 8-bit sRGB RGBA by default, 8 of `Rgba16Float`. That frame
    /// alone: no held input is delivered, the clock does not move, and no frame waits for the
    /// picture to land, as [a session's shot](crate::AgentWindow::shot) does. For a recording,
    /// which sets each frame's time.
    ///
    /// # Errors
    ///
    /// [`PlayError::NoPixels`] below [`Depth::Gpu`](crate::Depth::Gpu),
    /// [`PlayError::DeviceLost`] when the device is lost before the pixels come back, and
    /// [`PlayError::GpuTimedOut`] when it does not finish the frame in time; `pixels` is left
    /// empty on either.
    pub fn capture(&mut self, app: &mut impl App, pixels: &mut Vec<u8>) -> Result<(), PlayError> {
        if !self.paint.paints() {
            return Err(PlayError::NoPixels);
        }
        self.drive(app);
        let target = self
            .paint
            .target_mut()
            .expect("a harness that paints holds a target");
        device_answered(target.copy_texels(pixels))
    }

    /// The window's `Ui`, for what no method here reads.
    pub const fn ui(&self) -> &Ui {
        self.window.frames.ui()
    }

    /// The window's `Ui`, to change what no method here changes.
    pub const fn ui_mut(&mut self) -> &mut Ui {
        self.window.frames.ui_mut()
    }

    fn deliver_held_input(&mut self, app: &mut impl App) {
        while self.ui().input().has_held_input() {
            self.drive(app);
        }
    }

    /// One frame of `app` at the harness's time: record to the harness's depth, keep the window
    /// commands, and paint onto the target at [`Depth::Gpu`](crate::Depth::Gpu) while the device
    /// lives.
    fn drive(&mut self, app: &mut impl App) -> FrameReport {
        let now = self.time;
        let report = match &mut self.paint {
            HarnessPaint::Record { size } => {
                self.window
                    .record_frame(app, HarnessPaint::unpainted(*size), now)
            }
            HarnessPaint::Compose { size } => {
                self.window
                    .compose_frame(app, HarnessPaint::unpainted(*size), now)
            }
            HarnessPaint::Gpu { target, device } => self.window.paint_frame(
                app,
                HeadlessCanvas {
                    device,
                    target: target.as_target(),
                },
                now,
            ),
        };
        for command in self.window.drain_commands() {
            log_window_command(&mut self.window_commands, command);
        }
        report
    }

    /// Frames of `app` until it settles, as [a session's settle](crate::AgentWindow::settle) runs
    /// them; the frames recorded. Only a frame that asks for another counts against
    /// [`settle_frames`](crate::HarnessBuilder::settle_frames): a frame a busy app's wake brought
    /// is its progress, and the wait for the next wake starts again after each.
    fn settle_frames_of(&mut self, app: &mut impl App) -> Result<u32, PlayError> {
        let first = self.ui().record_frame_id();
        let began = Instant::now();
        let busy_limit = self.pace.background_wait.saturating_mul(BUSY_SETTLE_WAITS);
        let mut asking = 0;
        loop {
            let report = self.next_frame(app);
            let busy = app.is_busy();
            let screenshots = self.window.frames.has_pending_screenshots();
            // A wake that is no busy work's progress asks for frames as an animation does.
            if report.repaint_requested || screenshots || (self.window.is_wake_pending() && !busy) {
                // A lost device lands no screenshot and paints no frame an animation asks for.
                if self.is_device_lost() {
                    return Err(PlayError::DeviceLost);
                }
                asking += 1;
                if asking == self.pace.settle_frames {
                    return Err(PlayError::Unsettled {
                        frames: self.pace.settle_frames,
                    });
                }
                if screenshots {
                    device_answered(self.wait_for_gpu())?;
                }
                continue;
            }
            if busy {
                let waited = began.elapsed();
                if waited > busy_limit {
                    return Err(PlayError::StillBusy { waited });
                }
            }
            if self.window.is_wake_pending() {
                continue;
            }
            if !busy {
                return Ok(play::frames_recorded(first, self.ui().record_frame_id()));
            }
            if !self.window.wait_for_wake(self.pace.background_wait) {
                return Err(PlayError::Busy {
                    waited: self.pace.background_wait,
                });
            }
        }
    }

    /// One frame of `app`, one frame's time after the last.
    fn next_frame(&mut self, app: &mut impl App) -> FrameReport {
        self.advance(self.pace.frame_dt);
        self.frame_app(app)
    }

    fn spec(&self) -> FrameSpec {
        self.window.spec(self.paint.target_key(), self.time)
    }

    /// Show the display the next frame lays out at before it runs, so a point read off it is
    /// right.
    fn sync_display(&mut self) {
        let stamp = self.window.frames.stamp(&self.spec());
        self.window.frames.set_display(stamp.display);
    }

    fn mark_warm(&mut self) {
        let stamp = self.window.frames.stamp(&self.spec());
        self.window.frames.set_prev_stamp(Some(stamp));
    }

    /// The point `at` names, for an input method.
    #[track_caller]
    fn aim(&self, at: &Locator) -> Vec2 {
        at.find(&(), self.ui())
            .unwrap_or_else(|error| panic!("{error}"))
            .point
    }

    #[track_caller]
    fn assert_frame_dt(dt: Duration) {
        assert!(
            dt.as_secs_f32() <= MAX_ANIMATION_DT,
            "a {dt:?} step is past the {MAX_ANIMATION_DT} s an animation integrates, and would end \
             it short; run more, shorter frames",
        );
    }
}

/// Append `command` to `log` unless it opens a window the log holds open: the last command for
/// it is an open already.
fn log_window_command(log: &mut Vec<WindowCommand>, command: WindowCommand) {
    let token = command.token();
    let held_open = matches!(command, WindowCommand::Open { .. })
        && matches!(
            log.iter().rev().find(|logged| logged.token() == token),
            Some(WindowCommand::Open { .. })
        );
    if !held_open {
        log.push(command);
    }
}

/// What a wait for the device means to a player. A wait that timed out ends the step: the device
/// hangs, and another wait would hold the caller for the whole bound again, as many as
/// [`settle_frames`](crate::HarnessBuilder::settle_frames) times.
const fn device_answered(wait: GpuWait) -> Result<(), PlayError> {
    match wait {
        GpuWait::Done => Ok(()),
        GpuWait::DeviceLost => Err(PlayError::DeviceLost),
        GpuWait::TimedOut => Err(PlayError::GpuTimedOut),
    }
}

/// Whether either of two inputs asked a repaint.
const fn either(a: InputDelta, b: InputDelta) -> InputDelta {
    InputDelta {
        repaint_requested: a.repaint_requested || b.repaint_requested,
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::damage::Damage;
    use crate::damage::region::{CollapsedDamage, DamageRegion, MergePolicy};
    use crate::host::harness::{Harness, HarnessPaint};
    use crate::renderer::frontend::BuildPlan;
    use crate::renderer::render_buffer::RenderBuffer;
    use crate::renderer::render_plan::RenderPlan;
    use crate::ui::window_frames::WindowFrames;
    #[cfg(test)]
    use crate::{
        internals::paint_capture::PaintCapture,
        primitives::geometry::rect::Rect,
        primitives::geometry::translate_scale::TranslateScale,
        primitives::identity::widget_id::WidgetId,
        primitives::input::input_event::InputEvent,
        primitives::input::interaction::input_delta::InputDelta,
        primitives::layout::sizing::Sizing,
        primitives::math::animatable::Animatable,
        renderer::frontend::Frontend,
        renderer::frontend::encoder::{self, Encoder},
        scene::endpoint::Endpoint,
        scene::layer::Layer,
        scene::tree::node_id::NodeId,
        ui::Ui,
        ui::app::{App, RecordPass},
        ui::frame_report::FrameReport,
        ui::resources::UiResources,
        widget_core::configure::Configure,
        widgets::panel::Panel,
        window::window_token::WindowToken,
    };
    #[cfg(test)]
    use glam::{UVec2, Vec2};
    use image::RgbaImage;

    impl Harness {
        /// Snap paint edges to physical pixels from the next frame on, or not, as a host's
        /// options flip it.
        #[cfg(test)]
        pub(crate) fn set_pixel_snap(&mut self, pixel_snap: bool) -> &mut Self {
            self.window
                .cpu
                .resources
                .settings()
                .set_pixel_snap(pixel_snap);
            self.sync_display();
            self
        }

        /// The draw list the last composed or painted frame built.
        pub(crate) const fn render_buffer(&self) -> &RenderBuffer {
            &self.window.cpu.frontend.buffer
        }

        /// Build the draw list of the last frame's scene by `plan`, its panes left out, and read
        /// it: the encode and compose alone, as a bench times them.
        pub(crate) fn rebuild(&mut self, plan: RenderPlan) -> &RenderBuffer {
            self.window.cpu.frontend.build(
                self.window.frames.ui().frame_scene(),
                BuildPlan::without_panes(plan),
            );
            &self.window.cpu.frontend.buffer
        }

        /// Build the draw list of the whole retained scene, as a full repaint does; at
        /// [`Depth::Compose`](crate::Depth::Compose), it paints nothing.
        pub fn paint_full(&mut self) {
            let plan = RenderPlan {
                clear: self.ui().theme().window_bg,
                damage: Damage::Full,
            };
            self.window
                .frames
                .compose(&mut self.window.cpu.frontend, plan);
        }

        /// The window's pixels as the last frame left them, waited for. See
        /// [`OwnedTarget::read_back`](crate::internals::OwnedTarget::read_back).
        ///
        /// # Panics
        ///
        /// Below [`Depth::Gpu`](crate::Depth::Gpu), which paints no pixels.
        pub fn read_back(&self) -> RgbaImage {
            let HarnessPaint::Gpu { target, .. } = &self.paint else {
                panic!("a harness below Depth::Gpu paints no pixels");
            };
            target.read_back()
        }

        /// The window's `Ui` and the engines its frames run on.
        pub(crate) const fn frames(&self) -> &WindowFrames {
            &self.window.frames
        }

        pub(crate) const fn frames_mut(&mut self) -> &mut WindowFrames {
            &mut self.window.frames
        }

        /// This frame's raw damage rects, collapsed as the damage engine collapses them.
        pub(crate) fn collapsed_damage(&self) -> CollapsedDamage {
            DamageRegion::collapse_from(
                self.frames().damage().raw_rects(),
                MergePolicy::DEFAULT,
                self.ui().display().logical_rect(),
            )
        }

        pub(crate) fn damage_region(&self) -> DamageRegion {
            self.collapsed_damage().region
        }
    }

    /// The values one frame's record closure returned, one per record pass in run order, warmup
    /// left out. Pass A sees the one-frame edges; pass B, when one runs, runs after they are
    /// drained.
    #[cfg(test)]
    #[derive(Debug)]
    pub(crate) struct Passes<R> {
        values: Vec<R>,
        report: FrameReport,
    }

    #[cfg(test)]
    impl<R> Passes<R> {
        /// What the frame reported to its host.
        pub(crate) const fn report(&self) -> &FrameReport {
            &self.report
        }

        /// Pass A's value.
        pub(crate) fn a(&self) -> &R {
            self.values.first().expect("the frame ran no record pass")
        }

        /// Pass B's value, `None` on a frame of one pass.
        pub(crate) fn b(&self) -> Option<&R> {
            self.values.get(1)
        }

        /// How many record passes ran, warmup left out: 0, 1 or 2.
        pub(crate) const fn len(&self) -> usize {
            self.values.len()
        }

        /// How many passes returned a value matching `pred`, so a signal both reported reads as
        /// a double fire.
        pub(crate) fn count_where(&self, pred: impl Fn(&R) -> bool) -> usize {
            self.values.iter().filter(|value| pred(value)).count()
        }
    }

    #[cfg(test)]
    impl Harness {
        /// The CPU frontend every frame builds its draw list with.
        pub(crate) const fn frontend(&self) -> &Frontend {
            &self.window.cpu.frontend
        }

        /// The blurs the last frame ran, one per pane whose cached result went stale.
        pub(crate) const fn backdrop_blurs(&self) -> usize {
            match &self.paint {
                HarnessPaint::Gpu { device, .. } => device.backend().backdrop_blurs(),
                HarnessPaint::Record { .. } | HarnessPaint::Compose { .. } => {
                    panic!("a harness below Depth::Gpu runs no blur")
                }
            }
        }

        /// A window of `size` that shapes with the mono fallback, whose metrics this crate's
        /// tests compute by hand.
        pub(crate) fn mono(size: UVec2) -> Self {
            Self::builder(size).mono().build()
        }

        /// [`Self::mono`] with a cold recorder: frame 1 runs the warmup pass.
        pub(crate) fn cold(size: UVec2) -> Self {
            let mut harness = Self::mono(size);
            harness.window.frames.set_prev_stamp(None);
            harness
        }

        /// A recording window of `size` over `resources`, which other harnesses may share as
        /// windows of one host: it takes the first token none of them holds.
        pub(crate) fn from_resources(resources: UiResources, size: UVec2) -> Self {
            let token = (0..u64::MAX)
                .map(WindowToken)
                .find(|token| !resources.windows().contains(*token))
                .expect("a free token");
            Self::builder(size).build_over(resources, token)
        }

        /// One frame of `record`, keeping each record pass's value, after the frames held input
        /// owes.
        pub(crate) fn frame_passes<R>(
            &mut self,
            mut record: impl FnMut(&mut Ui) -> R,
        ) -> Passes<R> {
            self.deliver_held_input(&mut |ui: &mut Ui| {
                record(ui);
            });
            self.step_passes(record)
        }

        /// [`Self::frame_passes`] over exactly one frame, as [`Self::step`].
        pub(crate) fn step_passes<R>(&mut self, mut record: impl FnMut(&mut Ui) -> R) -> Passes<R> {
            let mut values = Vec::new();
            let report = self.step(|ui| {
                let value = record(ui);
                if ui.record_pass() != RecordPass::Warmup {
                    values.push(value);
                }
            });
            Passes { values, report }
        }

        /// One frame of `record` with the damage baseline dropped, as after a lost target: it
        /// repaints whole.
        pub(crate) fn frame_without_baseline(
            &mut self,
            record: impl FnMut(&mut Ui),
        ) -> FrameReport {
            self.window.frames.invalidate_target_contents();
            self.step(record)
        }

        /// Frames of `app` until one asks no repaint, as a session's settle runs them; the frames
        /// run.
        ///
        /// # Panics
        ///
        /// When the window does not settle.
        pub(crate) fn settled(&mut self, app: &mut impl App) -> u32 {
            self.settle_frames_of(app)
                .unwrap_or_else(|error| panic!("{error}"))
        }

        /// A `FILL` × `FILL` hstack around `f`, returning `f`'s value from pass A.
        pub(crate) fn under_outer<R>(&mut self, mut f: impl FnMut(&mut Ui) -> R) -> R {
            self.frame_value(|ui| {
                Panel::hstack()
                    .auto_id()
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, &mut f)
                    .inner
            })
        }

        /// The pointer leaves the window.
        pub(crate) fn pointer_left(&mut self) -> InputDelta {
            self.on_input(InputEvent::PointerLeft)
        }

        /// `id`'s map from arranged space to the window, unchecked, unlike [`Self::point_in`].
        pub(crate) fn transform(&self, id: WidgetId) -> TranslateScale {
            self.ui().response_for(id).transform
        }

        /// The center of `id`'s visible rect, unchecked.
        pub(crate) fn center_of(&self, id: WidgetId) -> Vec2 {
            self.rect(id)
                .unwrap_or_else(|| panic!("{id:?} has no visible rect"))
                .center()
        }

        pub(crate) fn node_for_widget_id(&self, id: WidgetId) -> NodeId {
            self.ui().forest().node_for_widget_id(Layer::Main, id)
        }

        pub(crate) fn node_of(&self, id: WidgetId) -> Option<Endpoint> {
            self.ui().cascade().endpoint(id)
        }

        pub(crate) fn main_child_ids(&self, parent: NodeId) -> Vec<NodeId> {
            self.ui()
                .tree(Layer::Main)
                .children(parent)
                .map(|child| child.id)
                .collect()
        }

        pub(crate) fn main_child_rects(&self, parent: NodeId) -> Vec<Rect> {
            self.ui()
                .tree(Layer::Main)
                .children(parent)
                .map(|child| self.ui().plane_layout(Layer::Main).rect[child.id.idx()])
                .collect()
        }

        pub(crate) fn animation_row_count<T: Animatable>(&mut self) -> usize {
            self.ui_mut().animation_mut().row_count::<T>()
        }

        pub(crate) fn encode_paint(&self) -> PaintCapture {
            self.encode(Damage::Full)
        }

        pub(crate) fn encode_paint_for(&self, region: DamageRegion) -> PaintCapture {
            self.encode(Damage::Partial(region.unmeasured()))
        }

        fn encode(&self, damage: Damage) -> PaintCapture {
            let plan = RenderPlan {
                clear: self.ui().theme().window_bg,
                damage,
            };
            encoder::internals::encode(self.ui().frame_scene(), &mut Encoder::default(), plan)
        }
    }
}

#[cfg(test)]
mod tests;
