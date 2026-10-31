//! [`Harness`]: one window with no desktop, on a stepped clock, which a unit test, an app's UI
//! test and an agent drive alike. A host beside [`WinitHost`](crate::WinitHost) and
//! [`OffscreenHost`](crate::OffscreenHost), over the same core and window driver, so its frames
//! run the stamp, display, damage and screenshot path the desktop runs.
//!
//! # The one fact everything follows from
//!
//! A frame calls the record closure one, two or three times, each seeing different input:
//!
//! | pass | when it runs | what it sees |
//! |---|---|---|
//! | warmup | the very first frame of a cold recorder | No input. Builds the cascade so pass A has something to hit-test. |
//! | A | always, except on a paint-only frame | The real input, including one-frame edges (`clicked`, `drag.started()`). |
//! | B | pass A acted or asked a relayout | The edges are gone. At most one retry. |
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
//!    reads pass A alone or is idempotent.
//! 6. **One clock, two doors.** [`Harness::advance`] and [`Harness::at`] move the time that
//!    stamps both frames and input.
//! 7. **A frozen clock makes every click one run of clicks**: the double-click window is 500 ms
//!    and 5 px, so [`Harness::advance_past_double_click`] goes between separate clicks.
//!    [`Harness::play`] waits it out by itself.
//! 8. **Animation time is not the frame clock.** An animation's step is clamped to 0.1 s and
//!    quantized at 1/240 s, so it moves through [`Harness::advance_frames`].
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
//! that names no one place panics. [`Harness::play`] is a person's: it settles the window after
//! each step and returns an error an agent can read.

pub(crate) mod harness_builder;
mod play;

use crate::app::App;
use crate::common::time::MAX_ANIMATION_DT;
use crate::display;
use crate::display::Display;
use crate::display::user_scale::UserScale;
use crate::driver::error::LocateError;
use crate::driver::locator::{Found, Locate, Locator};
use crate::gpu::surface::owned_target::OwnedTarget;
use crate::gpu::surface::render_target::TargetFormat;
use crate::host::core::HostCore;
use crate::host::harness::harness_builder::{HarnessBuilder, TARGET_LABEL};
use crate::host::wake_signal::WakeSignal;
use crate::host::window_driver::{CpuFrame, FrameDepth, FrameSpec, WindowDriver};
use crate::input::input_event::InputEvent;
use crate::input::interaction::input_delta::InputDelta;
use crate::input::interaction::response_state::ResponseState;
use crate::input::keyboard::key::Key;
use crate::input::keyboard::modifiers::Modifiers;
use crate::input::pointer::PointerButton;
use crate::input::sense::Sense;
use crate::input::shortcut::{Shortcut, ShortcutMods};
use crate::input_state::capture::DRAG_THRESHOLD;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::ui::Ui;
use crate::ui::frame_report::FrameReport;
use crate::ui::inspected_widget::InspectedWidget;
use crate::window::window_commands::{WindowCommand, WindowCommands};
use crate::window::window_token::WindowToken;
use glam::{UVec2, Vec2};
use std::any;
use std::sync::Arc;
use std::time::Duration;

/// The token of a harness's one window, as [`App::update`] and [`App::record`] receive it.
const WINDOW: WindowToken = WindowToken(0);

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
    core: HostCore,
    driver: WindowDriver,
    depth: FrameDepth,
    /// The texture a painted frame paints into, at [`Depth::Gpu`](crate::Depth::Gpu): one, kept
    /// between frames, so a still frame paints nothing.
    target: Option<OwnedTarget>,
    physical: UVec2,
    system_scale: f32,
    refresh_millihertz: Option<u32>,
    /// Moves only on [`Self::advance`] and [`Self::at`], and the frames [`Self::settle`] and
    /// [`Self::play`] run.
    time: Duration,
    /// Each button pressed and not released, with the press position; written only in
    /// [`Self::on_input`].
    held: [Option<PressOrigin>; PointerButton::COUNT],
    /// The time of the frame after [`Self::play`]'s last press, which its next press waits out
    /// the double-click window from.
    last_press: Option<Duration>,
    frame_dt: Duration,
    settle_frames: u32,
    /// The real time a settle waits for a wake while the app is busy.
    background_wait: Duration,
    /// The wakes the window's [`Waker`](crate::Waker) sends, which the next frame takes.
    wake: Arc<WakeSignal>,
    /// Every window command the frames recorded, oldest first.
    window_commands: Vec<WindowCommand>,
    /// One frame's commands, drained into `window_commands`.
    drained: WindowCommands,
    /// Frames run, which a settle counts by.
    frames: u32,
}

/// What [`HarnessBuilder::build`] seals besides the core and the driver.
#[derive(Debug)]
struct Seal {
    depth: FrameDepth,
    target: Option<OwnedTarget>,
    physical: UVec2,
    system_scale: f32,
    user_scale: UserScale,
    refresh_millihertz: Option<u32>,
    frame_dt: Duration,
    settle_frames: u32,
    background_wait: Duration,
}

/// Where a held button went down; `None` when the pointer was off the window.
#[derive(Clone, Copy, Debug)]
struct PressOrigin {
    at: Option<Vec2>,
}

impl Harness {
    /// A window of `size` physical pixels at scale 1 that records and builds no draw list,
    /// shaping with the bundled faces.
    pub fn new(size: UVec2) -> Self {
        Self::builder(size).build()
    }

    /// Start building a window of `size` physical pixels.
    pub const fn builder(size: UVec2) -> HarnessBuilder {
        HarnessBuilder::new(size)
    }

    fn open(core: HostCore, driver: WindowDriver, seal: Seal) -> Self {
        let mut harness = Self {
            core,
            driver,
            depth: seal.depth,
            target: seal.target,
            physical: seal.physical,
            system_scale: seal.system_scale,
            refresh_millihertz: seal.refresh_millihertz,
            time: Duration::ZERO,
            held: [None; PointerButton::COUNT],
            last_press: None,
            frame_dt: seal.frame_dt,
            settle_frames: seal.settle_frames,
            background_wait: seal.background_wait,
            wake: Arc::default(),
            window_commands: Vec::new(),
            drained: WindowCommands::default(),
            frames: 0,
        };
        harness.driver.ui.set_user_scale(seal.user_scale);
        let waker = harness.wake.waker();
        harness.driver.ui.set_waker(waker);
        #[cfg(test)]
        {
            harness.driver.ui.theme_mut().text.font_size = FIXTURE_FONT_SIZE;
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
        let mut warmup = self.driver.ui.frame_runtime().is_first_frame();
        let mut first = None;
        self.step(|ui| {
            // `record` runs on every pass: skipping it would record an empty tree.
            let value = record(ui);
            if warmup {
                warmup = false;
            } else if first.is_none() {
                first = Some(value);
            }
        });
        first.expect(
            "the frame ran no record pass: a paint animation was its only cause, as a focused \
             TextEdit's caret blink is. Feed an input or request a repaint first.",
        )
    }

    /// `id`'s response, read inside pass A: the one way to read a one-frame edge.
    pub fn response_in(&mut self, id: WidgetId, mut record: impl FnMut(&mut Ui)) -> ResponseState {
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

    /// Move the clock past the double-click window, so the next press starts a new run of
    /// clicks. Runs no frame.
    pub fn advance_past_double_click(&mut self) -> &mut Self {
        let window = self.driver.ui.double_click_window();
        self.advance(window + Duration::from_millis(1))
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
        self.physical = size;
        if let Some(target) = &mut self.target
            && target.size() != size
        {
            *target = target.resized(TARGET_LABEL, size);
        }
        self.sync_display();
        self
    }

    /// Swap the whole display from the next frame on, as a move to another monitor or a change of
    /// pixel snapping does; the frame reads the change.
    ///
    /// # Panics
    ///
    /// As [`Self::resize`], and when the scale is not finite or below `1e-4`.
    pub fn set_display(&mut self, display: Display) -> &mut Self {
        self.system_scale = display::scale_factor(display.system_scale);
        self.refresh_millihertz = display.refresh_millihertz;
        self.driver.set_pixel_snap(display.pixel_snap);
        self.driver.ui.set_user_scale(display.user_scale);
        self.resize(display.physical)
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
                    at: self.driver.ui.input().pointer_pos(),
                });
            }
            InputEvent::PointerReleased(button) => self.held[button.idx()] = None,
            _ => {}
        }
        self.driver.ui.on_input(event, self.time)
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
    pub fn click(&mut self, at: impl Into<Locator>) {
        self.press(at, PointerButton::Left);
        self.release(PointerButton::Left);
    }

    /// [`Self::click`] with the right button.
    ///
    /// # Panics
    ///
    /// As [`Self::press`].
    #[track_caller]
    pub fn right_click(&mut self, at: impl Into<Locator>) {
        self.press(at, PointerButton::Right);
        self.release(PointerButton::Right);
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
        self.on_input(InputEvent::ScrollLines(delta))
    }

    /// Move the pointer to `at` and pinch by `factor`: above 1 zooms in.
    ///
    /// # Panics
    ///
    /// As [`Self::hover`].
    #[track_caller]
    pub fn pinch(&mut self, at: impl Into<Locator>, factor: f32) -> InputDelta {
        self.hover(at);
        self.on_input(InputEvent::Zoom(factor))
    }

    /// Press a key, typing what it types. A shortcut with modifiers holds them for the press and
    /// then restores the ones held before; a bare key presses under the modifiers held
    /// ([`Self::set_modifiers`]). The key stays down until [`Self::key_up`].
    pub fn key(&mut self, key: impl Into<Shortcut>) -> InputDelta {
        let shortcut = key.into();
        let held = self.driver.ui.input().modifiers_after_held_input();
        let mods = shortcut.mods != ShortcutMods::NONE;
        if mods {
            self.set_modifiers(shortcut.mods.held());
        }
        let delta = self.on_input(InputEvent::KeyDown {
            key: shortcut.key,
            repeat: false,
            physical: Key::Other,
            text: shortcut.typed(),
        });
        if mods {
            self.set_modifiers(held);
        }
        delta
    }

    /// Release `key`.
    pub fn key_up(&mut self, key: Key) -> InputDelta {
        self.on_input(InputEvent::KeyUp { key })
    }

    /// Hold `mods` from now on. Sends nothing when they are held already, counting input that
    /// waits for a later frame.
    pub fn set_modifiers(&mut self, mods: Modifiers) {
        if self.driver.ui.input().modifiers_after_held_input() != mods {
            self.on_input(InputEvent::ModifiersChanged(mods));
        }
    }

    /// Type `text` as key presses, one a character, each typing it.
    pub fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            self.key(Key::Char(c));
        }
    }

    /// The one place `at` names in `app`'s last frame. See [`Locator::find`].
    ///
    /// # Errors
    ///
    /// [`LocateError`] when it names no place, or several.
    pub fn find<A, X: Locate<A>>(&self, app: &A, at: &Locator<X>) -> Result<Found, LocateError> {
        at.find(app, &self.driver.ui)
    }

    /// The visible interactive widgets of the last frame whose text holds `filter`, ignoring
    /// case, in paint order: what an agent reads to pick a locator.
    pub fn widgets(&self, filter: Option<&str>) -> Vec<InspectedWidget> {
        InspectedWidget::visible(&self.driver.ui, filter)
    }

    /// `id`'s visible rect in the last frame: after transforms and clips, what the pointer hits.
    /// [`Self::arranged`] is the rect a layout test means; the two differ only under a
    /// transform or a clip.
    pub fn rect(&self, id: WidgetId) -> Option<Rect> {
        self.driver.ui.response_for(id).rect
    }

    /// `id`'s arranged rect in the last frame: before transforms, not clipped.
    ///
    /// # Panics
    ///
    /// When `id` did not arrange in the last frame.
    #[track_caller]
    pub fn arranged(&self, id: WidgetId) -> Rect {
        self.driver
            .ui
            .response_for(id)
            .layout_rect
            .unwrap_or_else(|| panic!("{id:?} did not arrange last frame"))
    }

    /// The window point of `local`, a point in `id`'s arranged space, through every ancestor
    /// transform.
    ///
    /// # Panics
    ///
    /// When `id` did not arrange in the last frame, or another widget takes the pointer at that
    /// point.
    #[track_caller]
    pub fn point_in(&self, id: WidgetId, local: Vec2) -> Vec2 {
        let response = self.driver.ui.response_for(id);
        let layout = response.layout_rect.unwrap_or_else(|| {
            panic!("{id:?} has no arranged rect: it did not record, or no frame ran")
        });
        let pos = response.transform.apply_point(layout.min + local);
        let senses_anything = |sense: Sense| sense != Sense::NONE;
        let hit = self.driver.ui.cascade().hit_test(pos, senses_anything);
        assert_eq!(
            hit,
            Some(id),
            "{id:?} does not take the pointer at {pos:?}; {hit:?} is on top there",
        );
        pos
    }

    /// The topmost widget a hover at `pos` reaches.
    pub fn hit_at(&self, pos: Vec2) -> Option<WidgetId> {
        self.driver.ui.cascade().hit_test(pos, Sense::hovers)
    }

    /// `id`'s state row of type `S`.
    ///
    /// # Panics
    ///
    /// When there is no such row.
    #[track_caller]
    pub fn state<S: 'static>(&self, id: WidgetId) -> &S {
        self.driver.ui.state::<S>(id).unwrap_or_else(|| {
            panic!(
                "no `{}` row for {id:?}: a wrong id, a wrong type, or it never recorded",
                any::type_name::<S>(),
            )
        })
    }

    /// Every window the frames opened or closed, oldest first: a harness has one window, and
    /// records the rest.
    pub fn window_commands(&self) -> &[WindowCommand] {
        &self.window_commands
    }

    /// The text on the window's clipboard.
    ///
    /// # Panics
    ///
    /// When no clipboard answers; the harness's own, in memory, always does.
    pub fn clipboard_text(&self) -> String {
        self.driver
            .ui
            .clipboard()
            .text()
            .expect("the harness clipboard is in memory")
    }

    /// Put `text` on the window's clipboard, for a paste to come.
    pub fn set_clipboard_text(&mut self, text: &str) {
        self.driver
            .ui
            .clipboard()
            .set_text(text)
            .expect("the harness clipboard is in memory");
    }

    /// Block until the device finished every frame painted so far, and the copies of their
    /// screenshots are back, so a screenshot asked lands in the next frame. Does nothing below
    /// [`Depth::Gpu`](crate::Depth::Gpu).
    pub fn wait_for_gpu(&self) {
        if self.core.paints() {
            self.core.gpu().wait();
        }
    }

    /// The window's `Ui`, for what no method here reads.
    pub const fn ui(&mut self) -> &mut Ui {
        &mut self.driver.ui
    }

    fn deliver_held_input(&mut self, app: &mut impl App) {
        while self.driver.ui.input().has_held_input() {
            self.drive(app);
        }
    }

    /// One frame of `app` at the harness's time: record to the harness's depth, keep the window
    /// commands, and paint onto the target at [`Depth::Gpu`](crate::Depth::Gpu) while the device
    /// lives.
    fn drive(&mut self, app: &mut impl App) -> FrameReport {
        if self.wake.take() {
            self.driver.ui.request_repaint();
        }
        let spec = self.spec();
        let CpuFrame { report, mode } = self.core.record(&mut self.driver, spec, app, self.depth);
        self.driver.drain_window_output(&mut self.drained);
        self.window_commands.extend(self.drained.drain());
        if let Some(target) = &self.target
            && !self.core.gpu().is_lost()
        {
            self.core.submit(&mut self.driver, target.as_target(), mode);
        }
        self.frames += 1;
        report
    }

    fn spec(&self) -> FrameSpec {
        let (format, gives_copy) =
            self.target
                .as_ref()
                .map_or((TargetFormat::RGBA8_SRGB, true), |target| {
                    let target = target.as_target();
                    (target.format(), target.gives_copy())
                });
        FrameSpec {
            physical: self.physical,
            format,
            gives_copy,
            system_scale: self.system_scale,
            refresh_millihertz: self.refresh_millihertz,
            now: self.time,
        }
    }

    /// Show the display the next frame lays out at before it runs, so a point read off it is
    /// right.
    fn sync_display(&mut self) {
        let stamp = self.driver.stamp(&self.spec());
        self.driver.ui.set_display(stamp.display);
    }

    fn mark_warm(&mut self) {
        let stamp = self.driver.stamp(&self.spec());
        self.driver.ui.set_prev_stamp(Some(stamp));
    }

    /// The point `at` names, for an input method.
    #[track_caller]
    fn aim(&self, at: &Locator) -> Vec2 {
        at.find(&(), &self.driver.ui)
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

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::damage::Damage;
    use crate::damage::region::{CollapsedDamage, DEFAULT_PASS_BUDGET_PX, DamageRegion};
    use crate::host::harness::Harness;
    use crate::host::window_driver::internals::DriverParts;
    use crate::primitives::identity::widget_id::WidgetId;
    use crate::renderer::frontend::BuildPlan;
    #[cfg(test)]
    use crate::renderer::frontend::Frontend;
    use crate::renderer::render_buffer::RenderBuffer;
    use crate::renderer::render_plan::RenderPlan;
    use crate::ui::Ui;
    use crate::ui::frame_engines::FrameEngines;
    use image::RgbaImage;

    impl Harness {
        /// The draw list the last composed or painted frame built.
        pub(crate) const fn render_buffer(&self) -> &RenderBuffer {
            &self.core.frontend.buffer
        }

        /// The CPU frontend every frame builds its draw list with.
        #[cfg(test)]
        pub(crate) const fn frontend(&self) -> &Frontend {
            &self.core.frontend
        }

        /// Build the draw list of the last frame's scene by `plan`, its panes left out, and read
        /// it: the encode and compose alone, as a bench times them.
        pub(crate) fn rebuild(&mut self, plan: RenderPlan) -> &RenderBuffer {
            self.core
                .frontend
                .build(self.driver.ui.frame_scene(), BuildPlan::without_panes(plan));
            &self.core.frontend.buffer
        }

        /// Build the draw list of the whole retained scene, as a full repaint does; at
        /// [`Depth::Compose`](crate::Depth::Compose), it paints nothing.
        pub fn paint_full(&mut self) {
            let plan = RenderPlan {
                clear: self.driver.ui.theme().window_clear,
                damage: Damage::Full,
            };
            self.driver.compose(&mut self.core.frontend, plan);
        }

        /// The window's pixels as the last frame left them, waited for. See
        /// [`OwnedTarget::read_back`](crate::internals::OwnedTarget::read_back).
        ///
        /// # Panics
        ///
        /// Below [`Depth::Gpu`](crate::Depth::Gpu), which paints no pixels.
        pub fn read_back(&self) -> RgbaImage {
            self.target
                .as_ref()
                .expect("a harness that paints holds a target")
                .read_back()
        }

        /// The blurs the last frame ran, one per pane whose cached result went stale.
        #[cfg(test)]
        pub(crate) const fn backdrop_blurs(&self) -> usize {
            self.core.backend().backdrop_blurs()
        }

        pub(crate) const fn engines(&self) -> &FrameEngines {
            self.driver.engines()
        }

        pub(crate) const fn engines_mut(&mut self) -> &mut FrameEngines {
            self.driver.engines_mut()
        }

        /// The window's `Ui` and the engines its frames run on, borrowed at once.
        pub(crate) const fn parts(&mut self) -> DriverParts<'_> {
            self.driver.parts()
        }

        /// The window's `Ui`, borrowed shared.
        pub(crate) const fn ui_ref(&self) -> &Ui {
            &self.driver.ui
        }

        pub(crate) const fn set_focus(&mut self, id: WidgetId) {
            self.driver.ui.set_focus(id);
        }

        /// This frame's raw damage rects, collapsed as the damage engine collapses them.
        pub(crate) fn collapsed_damage(&self) -> CollapsedDamage {
            DamageRegion::collapse_from(
                self.engines().damage.raw_rects(),
                DEFAULT_PASS_BUDGET_PX,
                self.driver.ui.display().logical_rect(),
            )
        }

        pub(crate) fn damage_region(&self) -> DamageRegion {
            self.collapsed_damage().region
        }
    }
}

#[cfg(test)]
pub(crate) mod unit {
    use crate::app::App;
    use crate::damage::Damage;
    use crate::damage::region::DamageRegion;
    use crate::host::harness::Harness;
    use crate::input::input_event::InputEvent;
    use crate::input::interaction::input_delta::InputDelta;
    use crate::internals::paint_capture::PaintCapture;
    use crate::layout::intrinsic::intrinsic_query::IntrinsicQuery;
    use crate::layout::intrinsic::len_req::LenReq;
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::geometry::translate_scale::TranslateScale;
    use crate::primitives::identity::widget_id::WidgetId;
    use crate::primitives::layout::axis::Axis;
    use crate::primitives::layout::sizing::Sizing;
    use crate::primitives::math::animatable::Animatable;
    use crate::renderer::frontend::encoder;
    use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
    use crate::renderer::render_plan::RenderPlan;
    use crate::scene::endpoint::Endpoint;
    use crate::scene::layer::Layer;
    use crate::scene::tree::node_id::NodeId;
    use crate::ui::Ui;
    use crate::ui::frame_report::FrameReport;
    use crate::ui::resources::UiResources;
    use crate::widget_core::configure::Configure;
    use crate::widgets::panel::Panel;
    use crate::window::window_token::WindowToken;
    use glam::{UVec2, Vec2};

    /// The values one frame's record closure returned, one per record pass in run order, warmup
    /// left out. Pass A sees the one-frame edges; pass B, when one runs, runs after they are
    /// drained.
    #[derive(Debug)]
    pub(crate) struct Passes<R> {
        values: Vec<R>,
        report: FrameReport,
    }

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

    impl Harness {
        /// A window of `size` that shapes with the mono fallback, whose metrics this crate's
        /// tests compute by hand.
        pub(crate) fn mono(size: UVec2) -> Self {
            Self::builder(size).mono().build()
        }

        /// [`Self::mono`] with a cold recorder: frame 1 runs the warmup pass.
        pub(crate) fn cold(size: UVec2) -> Self {
            let mut harness = Self::mono(size);
            harness.driver.ui.set_prev_stamp(None);
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
            let mut warmup = self.driver.ui.frame_runtime().is_first_frame();
            let mut values = Vec::new();
            let report = self.step(|ui| {
                let value = record(ui);
                if warmup {
                    warmup = false;
                } else {
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
            self.driver.invalidate_target_contents();
            self.step(record)
        }

        /// Frames of `app` until one asks no repaint, as [`Self::settle`] runs them; the frames
        /// run.
        ///
        /// # Panics
        ///
        /// When the window does not settle.
        pub(crate) fn settled(&mut self, app: &mut impl App) -> u32 {
            self.settle(app).unwrap_or_else(|error| panic!("{error}"))
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

        pub(crate) const fn focus(&self) -> Option<WidgetId> {
            self.driver.ui.focus()
        }

        pub(crate) const fn clear_focus(&mut self) {
            self.driver.ui.clear_focus();
        }

        pub(crate) fn is_focus_within(&self, ancestor: WidgetId) -> bool {
            self.driver.ui.is_focus_within(ancestor)
        }

        /// The pointer leaves the window.
        pub(crate) fn pointer_left(&mut self) -> InputDelta {
            self.on_input(InputEvent::PointerLeft)
        }

        /// `id`'s arranged rect, `None` when it did not arrange.
        pub(crate) fn layout_rect(&self, id: WidgetId) -> Option<Rect> {
            self.driver.ui.response_for(id).layout_rect
        }

        /// `id`'s map from arranged space to the window, unchecked, unlike [`Self::point_in`].
        pub(crate) fn transform(&self, id: WidgetId) -> TranslateScale {
            self.driver.ui.response_for(id).transform
        }

        /// The center of `id`'s visible rect, unchecked.
        pub(crate) fn center_of(&self, id: WidgetId) -> Vec2 {
            self.rect(id)
                .unwrap_or_else(|| panic!("{id:?} has no visible rect"))
                .center()
        }

        pub(crate) fn node_for_widget_id(&self, id: WidgetId) -> NodeId {
            self.driver.ui.forest().node_for_widget_id(Layer::Main, id)
        }

        pub(crate) fn intrinsic(&mut self, node: NodeId, axis: Axis, req: LenReq) -> f32 {
            self.driver
                .intrinsic(node, IntrinsicQuery::single(axis, req))
                .get(req)
        }

        pub(crate) fn node_of(&self, id: WidgetId) -> Option<Endpoint> {
            self.driver.ui.cascade().endpoint(id)
        }

        pub(crate) fn main_child_ids(&self, parent: NodeId) -> Vec<NodeId> {
            self.driver
                .ui
                .tree(Layer::Main)
                .children(parent)
                .map(|child| child.id)
                .collect()
        }

        pub(crate) fn main_child_rects(&self, parent: NodeId) -> Vec<Rect> {
            self.driver
                .ui
                .tree(Layer::Main)
                .children(parent)
                .map(|child| self.driver.ui.layout(Layer::Main).rect[child.id.idx()])
                .collect()
        }

        pub(crate) fn animation_row_count<T: Animatable>(&mut self) -> usize {
            self.driver.ui.animation_mut().row_count::<T>()
        }

        pub(crate) fn encode_paint(&self) -> PaintCapture {
            self.encode(Damage::Full)
        }

        pub(crate) fn encode_paint_for(&self, region: DamageRegion) -> PaintCapture {
            self.encode(Damage::Partial(region.unmeasured()))
        }

        fn encode(&self, damage: Damage) -> PaintCapture {
            let plan = RenderPlan {
                clear: self.driver.ui.theme().window_clear,
                damage,
            };
            encoder::internals::encode(
                self.driver.ui.frame_scene(),
                &SharedGradientAtlas::default(),
                plan,
            )
        }
    }
}

#[cfg(test)]
mod tests;
