//! Frame-driving test harness: drives a [`Ui`] with synthetic input.
//!
//! # The one fact everything follows from
//!
//! `Ui::frame` calls the record closure one, two or three times, each seeing
//! different input:
//!
//! | pass | when it runs | what it sees |
//! |---|---|---|
//! | warmup | `prev_stamp.is_none()`, the very first frame | `InputState` swapped for an empty one. Builds the cascade so pass A has something to hit-test. |
//! | A | always (except `PaintOnly`) | The real input, including one-frame edges (`clicked`, `drag.started()`). |
//! | B | `frame_had_action` \|\| `relayout_requested` | Post-`drain_per_frame_queues`: the edges are gone. At most one retry. |
//!
//! `PaintOnly` runs zero record passes. The harness keeps no copy of pointer
//! position or modifiers (a mirror desyncs once an event goes through
//! [`UiHarness::on_input`]).
//!
//! # The protocol rules
//!
//! 1. **Warm the recorder.** A bare `Ui` is cold, so frame 1 runs the warmup pass;
//!    only the test-gated `UiHarness::cold` leaves it cold.
//! 2. **Prime before reading.** `response_for` reads last frame's cascade: input
//!    assertions need one prior frame, a stable arranged rect two
//!    ([`UiHarness::prime`]).
//! 3. **Read the response inside the record** ([`UiHarness::response_in`]).
//! 4. **Read the first record pass.** Pass B has no edges, so
//!    [`UiHarness::frame_value`] keeps pass A's value.
//! 5. **The closure's side effects run once per pass too.** Anything accumulating
//!    across frames must read pass one only or be idempotent.
//! 6. **One clock, two doors.** [`advance`](UiHarness::advance) and
//!    [`at`](UiHarness::at) move `UiHarness::time`, which stamps both frames and
//!    fed events.
//! 7. **A frozen clock makes every click simultaneous**: `DOUBLE_CLICK_WINDOW` is
//!    500 ms and `DOUBLE_CLICK_RADIUS` 5 px, so use
//!    [`advance_past_double_click`](UiHarness::advance_past_double_click) between
//!    separate clicks.
//! 8. **Animation time is not the frame clock.** Per-frame animation dt is clamped
//!    to `MAX_ANIM_DT` (0.1 s) and quantized at `ANIM_SUBSTEP_DT` (1/240 s). Use
//!    [`advance_frames`](UiHarness::advance_frames).
//! 9. **A frame can run no record pass.** A focused `TextEdit`'s caret blink keeps
//!    `PaintOnly` frames coming for 30 s. [`frame_value`](UiHarness::frame_value)
//!    panics on one; [`frame_passes`](UiHarness::frame_passes) reports no passes.
//! 10. **Surface size is physical; pointer positions are logical** (`physical /
//!     dpr`).
//! 11. **Keyboard events are discarded at ingress when nothing is focused**
//!     (`InputState::on_input` gates `KeyDown` on
//!     `focused.is_some() || subs.matches_press(kp) || keyboard_mask`). Focus first.
//! 12. **One change of a kind per frame.** A second press or release of one button,
//!     or a key press after the frame's command key, waits for the next frame
//!     (`InputQueue`), so `click_at` is a press frame then a release frame.
//!     [`UiHarness::frame`] runs the frames held input owes first and answers from
//!     the last; [`UiHarness::step`] steps one at a time.
//!
//! Tiers: `impl UiHarness` (`pub`) leaves the crate through
//! `palantir::internals::harness`; `impl UiHarness` (`pub(crate)`) is construction;
//! `mod unit` (`#[cfg(test)]`) is for in-tree suites only, so `bench` builds do not
//! see it as unused.

use crate::app::App;
use crate::common::time::MAX_ANIM_DT;
use crate::display::Display;
use crate::display::user_scale::UserScale;
use crate::input::capture::{DOUBLE_CLICK_WINDOW, DRAG_THRESHOLD};
use crate::input::input_event::InputEvent;
use crate::input::interaction::input_delta::InputDelta;
use crate::input::interaction::response_state::ResponseState;
use crate::input::keyboard::key::Key;
use crate::input::keyboard::modifiers::Modifiers;
use crate::input::pointer::PointerButton;
use crate::input::sense::Sense;
use crate::internals::harness::passes::Passes;
use crate::internals::record_app::RecordApp;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::ui::Ui;
use crate::ui::frame_engines::FrameEngines;
use crate::ui::frame_report::FrameReport;
use crate::ui::frame_stamp::FrameInput;
use crate::ui::frame_stamp::FrameStamp;
use crate::ui::resources::UiResources;
use crate::window::window_token::WindowToken;
use glam::{UVec2, Vec2};
use std::any;
use std::time::Duration;

pub mod frontend_harness;
#[cfg(test)]
pub(crate) mod oracle;
pub mod passes;
#[cfg(test)]
pub(crate) mod size_trio;

/// Never framed; the value only has to be non-degenerate.
const ARENA_SURFACE: UVec2 = UVec2::splat(1);

/// Drives a [`Ui`] through frames with synthetic input; see the module doc for the pass model.
#[derive(Debug)]
#[must_use]
pub struct UiHarness {
    /// `pub(crate)` so in-crate suites assert through `Ui`'s gated accessors; consumers use
    /// [`Self::ui`].
    pub(crate) ui: Ui,
    /// The engines this harness's frames run on; `pub(crate)` for in-crate damage and layout tests.
    pub(crate) engines: FrameEngines,
    /// What every frame stamps with. `physical` is physical pixels; pointer positions are logical.
    /// Its `user_scale` is not read: a frame takes [`Ui::user_scale`].
    display: Display,
    /// Absolute; moves only on [`Self::advance`] / [`Self::at`].
    time: Duration,
    /// Buttons pressed and not released, with the press position; written only in
    /// [`Self::on_input`].
    held: [Option<PressOrigin>; PointerButton::COUNT],
}

/// Where a held button went down; `None` when the pointer was off-surface.
#[derive(Clone, Copy, Debug)]
struct PressOrigin {
    at: Option<Vec2>,
}

impl UiHarness {
    /// Mono-fallback text: deterministic, wrong for width-follows-label assertions.
    pub fn new(surface: UVec2) -> Self {
        Self::from_resources(UiResources::isolated_mono(), surface)
    }

    /// Real cosmic shaping over the bundled faces only, so metrics and exact widths are
    /// machine-independent.
    pub fn with_text(surface: UVec2) -> Self {
        Self::from_resources(UiResources::isolated_text(), surface)
    }

    /// A harness that is never framed; its [`Self::ui`] is a string-interning arena.
    pub fn arena() -> Self {
        Self::new(ARENA_SURFACE)
    }

    /// Device pixel ratio. The surface stays physical, so at `dpr = 2.0` a 600x200 surface is
    /// 300x100 logical.
    pub fn scale(mut self, dpr: f32) -> Self {
        self.display.system_scale = dpr;
        self.sync_display();
        self.rewarm();
        self
    }

    /// The app's own scale on top of [`Self::scale`]; goes to [`Ui::set_user_scale`].
    pub fn user_scale(mut self, scale: UserScale) -> Self {
        self.ui.set_user_scale(scale);
        self.sync_display();
        self.rewarm();
        self
    }

    /// Monitor refresh, read by repaint-wake coalescing.
    pub fn refresh_millihertz(mut self, mhz: u32) -> Self {
        self.display.refresh_millihertz = Some(mhz);
        self.sync_display();
        self.rewarm();
        self
    }

    /// Change the surface between frames. Does not re-warm, so the next frame reads a
    /// `display_changed` like a real resize.
    pub fn resize(&mut self, surface: UVec2) -> &mut Self {
        self.display.physical = surface;
        self.sync_display();
        self
    }

    /// Swap the whole display between frames (DPI move, pixel-snap flip). Same no-re-warm contract.
    pub fn set_display(&mut self, display: Display) -> &mut Self {
        self.display = display;
        self.ui.set_user_scale(display.user_scale);
        self.sync_display();
        self
    }

    /// Run one frame; the closure runs once, twice or not at all. First runs the frames held input
    /// still owes (rule 12).
    pub fn frame(&mut self, record: impl FnMut(&mut Ui)) -> FrameReport {
        self.frame_app(&mut RecordApp::new(record))
    }

    /// Run frames of a whole [`App`] as a host does, delivering held input first.
    pub fn frame_app(&mut self, app: &mut impl App) -> FrameReport {
        self.deliver_held_input(app);
        self.drive(true, app)
    }

    fn deliver_held_input(&mut self, app: &mut impl App) {
        while self.ui.input().has_held_input() {
            self.drive(true, app);
        }
    }

    /// Exactly one host frame, delivering nothing held.
    pub fn step(&mut self, record: impl FnMut(&mut Ui)) -> FrameReport {
        self.drive(true, &mut RecordApp::new(record))
    }

    /// Run one frame and keep each record pass's value, warmup excluded. See [`Passes`].
    pub fn frame_passes<R>(&mut self, mut record: impl FnMut(&mut Ui) -> R) -> Passes<R> {
        self.deliver_held_input(&mut RecordApp::new(|ui: &mut Ui| {
            record(ui);
        }));
        self.step_passes(record)
    }

    /// [`Self::frame_passes`] over exactly one host frame, like [`Self::step`].
    pub fn step_passes<R>(&mut self, mut record: impl FnMut(&mut Ui) -> R) -> Passes<R> {
        let mut warmup = self.ui.frame_runtime().is_first_frame();
        let mut values = Vec::new();
        let report = self.step(|ui| {
            // `record` runs on every pass: skipping it would record an empty tree and wipe the
            // cascade.
            let value = record(ui);
            if warmup {
                warmup = false;
            } else {
                values.push(value);
            }
        });
        Passes::new(values, report)
    }

    /// The value from pass A. Panics if the frame ran no record pass; see [`Self::frame_passes`].
    pub fn frame_value<R>(&mut self, record: impl FnMut(&mut Ui) -> R) -> R {
        self.frame_passes(record).into_a().expect(
            "the frame ran no record pass — FrameProcessing::PaintOnly. A paint-anim \
             wake was the frame's only cause (a focused TextEdit's caret blink is \
             enough). Feed an input, request a repaint, or use `frame_passes`.",
        )
    }

    /// `n` discarded frames; two is the usual minimum. Not named `settle`, which means the second
    /// record pass within a frame.
    pub fn prime(&mut self, n: u32, mut record: impl FnMut(&mut Ui)) {
        for _ in 0..n {
            self.frame(&mut record);
        }
    }

    /// Move the absolute clock; events fed afterward and the next frame carry it. Animation dt is
    /// clamped, so use [`Self::advance_frames`] for animation.
    pub fn advance(&mut self, dt: Duration) -> &mut Self {
        self.time += dt;
        self
    }

    /// Park the absolute clock at `time`.
    pub const fn at(&mut self, time: Duration) -> &mut Self {
        self.time = time;
        self
    }

    /// `n` frames stepping `dt` each, the way to move an animation. A `dt` past `MAX_ANIM_DT`
    /// panics rather than under-integrate silently.
    pub fn advance_frames(&mut self, n: u32, dt: Duration, mut record: impl FnMut(&mut Ui)) {
        Self::assert_anim_step(dt);
        for _ in 0..n {
            self.advance(dt);
            self.frame(&mut record);
        }
    }

    /// Frames stepping `dt` each until one requests no repaint, at most `max`. Returns how many it
    /// took or `None` if the last still asked for more.
    pub fn frames_until_idle(
        &mut self,
        max: u32,
        dt: Duration,
        mut record: impl FnMut(&mut Ui),
    ) -> Option<u32> {
        Self::assert_anim_step(dt);
        (1..=max).find(|_| {
            self.advance(dt);
            !self.frame(&mut record).repaint_requested
        })
    }

    fn assert_anim_step(dt: Duration) {
        assert!(
            dt.as_secs_f32() <= MAX_ANIM_DT,
            "a {dt:?} step exceeds MAX_ANIM_DT ({MAX_ANIM_DT}s) and would silently \
             under-integrate the animation; use more, smaller frames",
        );
    }

    /// Move the clock past `DOUBLE_CLICK_WINDOW` so the next click starts a fresh
    /// press run. Records no frame, which would sweep the state rows under test.
    pub fn advance_past_double_click(&mut self) -> &mut Self {
        self.advance(DOUBLE_CLICK_WINDOW + Duration::from_millis(1))
    }

    /// The raw input door, for events with no typed helper; the typed helpers enforce the
    /// press-origin and modifier rules.
    pub fn on_input(&mut self, event: InputEvent<'_>) -> InputDelta {
        match event {
            InputEvent::PointerPressed(button) => {
                assert!(
                    self.held[button.idx()].is_none(),
                    "{button:?} is already held — release it before pressing it again",
                );
                self.held[button.idx()] = Some(PressOrigin {
                    at: self.ui.input().pointer_pos(),
                });
            }
            InputEvent::PointerReleased(button) => self.held[button.idx()] = None,
            _ => {}
        }
        self.ui.on_input(event, self.time)
    }

    /// Returns the move's [`InputDelta`].
    pub fn move_to(&mut self, pos: Vec2) -> InputDelta {
        self.on_input(InputEvent::PointerMoved(pos))
    }

    /// The pointer leaves the surface; returns the event's [`InputDelta`].
    pub fn pointer_left(&mut self) -> InputDelta {
        self.on_input(InputEvent::PointerLeft)
    }

    /// Press wherever the pointer is. The press origin is read back from `InputState`, so a press
    /// after a raw `PointerMoved` still latches [`Self::drag_to`]'s threshold.
    pub fn press(&mut self) -> InputDelta {
        self.press_button(PointerButton::Left)
    }

    /// [`Self::press`] with the button named.
    pub fn press_button(&mut self, button: PointerButton) -> InputDelta {
        self.on_input(InputEvent::PointerPressed(button))
    }

    /// Move to `pos`, then [`Self::press`].
    pub fn press_at(&mut self, pos: Vec2) -> InputDelta {
        self.press_button_at(PointerButton::Left, pos)
    }

    /// [`Self::press_at`] with the button named.
    pub fn press_button_at(&mut self, button: PointerButton, pos: Vec2) -> InputDelta {
        self.move_to(pos);
        self.press_button(button)
    }

    /// Release the left button, clearing the press origin [`Self::drag_to`] measures from.
    pub fn release(&mut self) -> InputDelta {
        self.release_button(PointerButton::Left)
    }

    /// [`Self::release`] with the button named.
    pub fn release_button(&mut self, button: PointerButton) -> InputDelta {
        self.on_input(InputEvent::PointerReleased(button))
    }

    /// Press and release the left button at `pos`.
    pub fn click_at(&mut self, pos: Vec2) {
        self.click_button_at(PointerButton::Left, pos);
    }

    /// [`Self::click_at`] with the right button.
    pub fn right_click_at(&mut self, pos: Vec2) {
        self.click_button_at(PointerButton::Right, pos);
    }

    /// The button-generic click, for sweeping every [`PointerButton`].
    pub fn click_button_at(&mut self, button: PointerButton, pos: Vec2) {
        self.press_button_at(button, pos);
        self.release_button(button);
    }

    /// `id`'s center, checked: panics unless the pointer would reach `id` there (an
    /// overlapping hover-sensing widget wins the scan). Needs a primed frame. The filter
    /// is "senses anything", wider than [`Self::hit_at`]'s hover filter, since a
    /// `SCROLL`-only widget is invisible to hover by design.
    fn hit_center_of(&self, id: WidgetId) -> Vec2 {
        let center = self.center_of(id);
        self.assert_reaches(id, center);
        center
    }

    fn assert_reaches(&self, id: WidgetId, pos: Vec2) {
        let senses_anything = |sense: Sense| sense != Sense::NONE;
        let hit = self.ui.cascade().hit_test(pos, senses_anything);
        assert_eq!(
            hit,
            Some(id),
            "{id:?} does not receive the pointer at {pos:?} — {hit:?} is on top there. \
             Aim by position if that is intended.",
        );
    }

    /// The screen position of `local`, a point in `id`'s arranged space, through every ancestor
    /// transform. Checked like [`Self::click_on`].
    pub fn point_in(&self, id: WidgetId, local: Vec2) -> Vec2 {
        let response = self.ui.response_for(id);
        let layout = response.layout_rect.unwrap_or_else(|| {
            panic!("{id:?} has no arranged rect — it did not record, or nothing primed the frame")
        });
        let pos = response.transform.apply_point(layout.min + local);
        self.assert_reaches(id, pos);
        pos
    }

    /// `id`'s map from arranged space to screen. Unchecked, unlike [`Self::point_in`].
    pub fn transform(&self, id: WidgetId) -> TranslateScale {
        self.ui.response_for(id).transform
    }

    /// Click `id` at [`Self::point_in`]`(id, local)`.
    pub fn click_in(&mut self, id: WidgetId, local: Vec2) {
        let pos = self.point_in(id, local);
        self.click_at(pos);
    }

    /// Press `id` at [`Self::point_in`]`(id, local)`.
    pub fn press_in(&mut self, id: WidgetId, local: Vec2) -> InputDelta {
        let pos = self.point_in(id, local);
        self.press_at(pos)
    }

    /// Click `id` at its center; see `Self::hit_center_of`.
    pub fn click_on(&mut self, id: WidgetId) {
        self.click_at(self.hit_center_of(id));
    }

    /// Press `id` at its center, checked like [`Self::click_on`].
    pub fn press_on(&mut self, id: WidgetId) -> InputDelta {
        self.press_at(self.hit_center_of(id))
    }

    /// Hover `id`: the move half of [`Self::press_on`].
    pub fn move_onto(&mut self, id: WidgetId) -> InputDelta {
        self.move_to(self.hit_center_of(id))
    }

    /// Move while pressed. Panics if travel since the press has not crossed `DRAG_THRESHOLD` (the
    /// capture would not latch).
    pub fn drag_to(&mut self, pos: Vec2) -> InputDelta {
        let origin = self
            .held
            .iter()
            .find_map(|held| *held)
            .expect("drag_to needs a press first — no button is down")
            .at
            .expect("drag_to needs a press on the surface — the pointer was off it");
        let travel = origin.distance(pos);
        assert!(
            travel >= DRAG_THRESHOLD,
            "drag_to({pos:?}) travels {travel} px from the press at {origin:?}, under \
             the {DRAG_THRESHOLD} px DRAG_THRESHOLD — no drag would latch",
        );
        self.move_to(pos)
    }

    /// Scroll and pinch carry no position; `InputState` routes them to whatever the pointer was
    /// last over. Positive `y` scrolls content down.
    pub fn scroll_lines(&mut self, delta: Vec2) -> InputDelta {
        self.on_input(InputEvent::ScrollLines(delta))
    }

    /// [`Self::scroll_lines`] in pixels.
    pub fn scroll_pixels(&mut self, delta: Vec2) -> InputDelta {
        self.on_input(InputEvent::ScrollPixels(delta))
    }

    /// A pinch-zoom step, aimed like [`Self::scroll_lines`].
    pub fn pinch(&mut self, factor: f32) -> InputDelta {
        self.on_input(InputEvent::Zoom(factor))
    }

    /// Aim, then scroll; the delta returned is the scroll's.
    pub fn scroll_lines_at(&mut self, pos: Vec2, delta: Vec2) -> InputDelta {
        self.move_to(pos);
        self.scroll_lines(delta)
    }

    /// Aim, then [`Self::scroll_pixels`].
    pub fn scroll_pixels_at(&mut self, pos: Vec2, delta: Vec2) -> InputDelta {
        self.move_to(pos);
        self.scroll_pixels(delta)
    }

    /// Aim, then [`Self::pinch`].
    pub fn pinch_at(&mut self, pos: Vec2, factor: f32) -> InputDelta {
        self.move_to(pos);
        self.pinch(factor)
    }

    /// A non-repeat press whose `physical` is [`Key::Other`]; use [`Self::on_input`] for a real
    /// physical key.
    pub fn key(&mut self, key: Key) -> InputDelta {
        self.on_input(InputEvent::key_down(key))
    }

    /// Emits `ModifiersChanged` only on an actual change (read from `InputState` after
    /// held input, not a mirror), so it returns no [`InputDelta`].
    pub fn set_modifiers(&mut self, mods: Modifiers) {
        if self.ui.input().modifiers_after_held_input() != mods {
            self.on_input(InputEvent::ModifiersChanged(mods));
        }
    }

    /// One press per character, each carrying it as text.
    pub fn type_text(&mut self, s: &str) {
        for c in s.chars() {
            self.key(Key::Char(c));
        }
    }

    /// The visible rect from the previous frame's cascade (after transforms and
    /// clipping), which is what the pointer hits. For layout assertions use
    /// [`Self::layout_rect`]; they differ only under a transform or clip.
    pub fn rect(&self, id: WidgetId) -> Option<Rect> {
        self.ui.response_for(id).rect
    }

    /// The arranged rect: pre-transform, unclipped, world coordinates. The one a layout test means.
    pub fn layout_rect(&self, id: WidgetId) -> Option<Rect> {
        self.ui.response_for(id).layout_rect
    }

    /// [`Self::layout_rect`] for a widget known to have arranged; panics otherwise.
    pub fn arranged(&self, id: WidgetId) -> Rect {
        self.layout_rect(id)
            .unwrap_or_else(|| panic!("{id:?} did not arrange last frame"))
    }

    /// Center of `id`'s visible rect, [`Self::rect`].
    pub fn center_of(&self, id: WidgetId) -> Vec2 {
        self.rect(id)
            .unwrap_or_else(|| {
                panic!(
                    "{id:?} has no visible rect — it did not record, or nothing primed the frame"
                )
            })
            .center()
    }

    /// `id`'s response captured inside pass A, the only correct way to read a one-frame edge.
    pub fn response_in(&mut self, id: WidgetId, mut record: impl FnMut(&mut Ui)) -> ResponseState {
        self.frame_value(move |ui| {
            record(ui);
            ui.response_for(id)
        })
    }

    /// Focus and pointer reads with no protocol hazard. Use [`Self::rect`] for geometry and
    /// [`Self::response_in`] for edges.
    pub const fn focus(&self) -> Option<WidgetId> {
        self.ui.focus()
    }

    /// [`Ui::set_focus`].
    pub const fn set_focus(&mut self, id: WidgetId) {
        self.ui.set_focus(id);
    }

    /// [`Ui::clear_focus`].
    pub const fn clear_focus(&mut self) {
        self.ui.clear_focus();
    }

    /// [`Ui::is_focus_within`].
    pub fn is_focus_within(&self, ancestor: WidgetId) -> bool {
        self.ui.is_focus_within(ancestor)
    }

    /// Topmost widget the pointer would hit at `pos`, by the hover routing filter.
    pub fn hit_at(&self, pos: Vec2) -> Option<WidgetId> {
        self.ui.cascade().hit_test(pos, Sense::hovers)
    }

    /// The harness clipboard's text.
    ///
    /// # Panics
    ///
    /// Panics when no backend answers; the in-memory one always does.
    pub fn clipboard_text(&self) -> String {
        self.ui
            .clipboard()
            .text()
            .expect("the harness clipboard is the in-memory backend")
    }

    /// Seed the harness clipboard for a coming paste.
    pub fn set_clipboard_text(&mut self, text: &str) {
        self.ui
            .clipboard()
            .set_text(text)
            .expect("the memory clipboard is always available");
    }

    /// `id`'s cross-frame state row of type `S`.
    ///
    /// # Panics
    ///
    /// Panics when no such row exists.
    pub fn state<S: 'static>(&self, id: WidgetId) -> &S {
        self.ui.state::<S>(id).unwrap_or_else(|| {
            panic!(
                "no `{}` row for {id:?} — wrong id, wrong type, or it never recorded",
                any::type_name::<S>(),
            )
        })
    }

    /// Escape hatch. Prefer [`Self::response_in`] to `response_for` off this.
    pub const fn ui(&mut self) -> &mut Ui {
        &mut self.ui
    }
}

impl UiHarness {
    pub(crate) fn from_resources(resources: UiResources, surface: UVec2) -> Self {
        let mut harness = Self {
            engines: FrameEngines::new(&resources),
            ui: Ui::new(resources),
            display: Display::from_physical(surface, 1.0),
            time: Duration::ZERO,
            held: [None; PointerButton::COUNT],
        };
        harness.sync_display();
        harness.mark_warm();
        harness
    }

    fn drive(&mut self, damage_baseline_valid: bool, app: &mut impl App) -> FrameReport {
        let stamp = FrameStamp::new(self.frame_display(), self.time);
        self.ui.frame(
            &mut self.engines,
            FrameInput::new(stamp, damage_baseline_valid),
            WindowToken(0),
            app,
        )
    }

    /// The display a frame runs at, derived as the window driver does.
    fn frame_display(&self) -> Display {
        Display {
            user_scale: self.ui.user_scale(),
            ..self.display
        }
    }

    fn sync_display(&mut self) {
        self.ui.set_display(self.frame_display());
    }

    /// Re-seed `prev_stamp` after a builder moved the display, so frame 1 does not read a display
    /// change. Warm harnesses only.
    fn rewarm(&mut self) {
        if !self.ui.frame_runtime().is_first_frame() {
            self.mark_warm();
        }
    }

    fn mark_warm(&mut self) {
        self.ui
            .set_prev_stamp(Some(FrameStamp::new(self.frame_display(), self.time)));
    }
}

#[cfg(any(test, feature = "bench"))]
pub(crate) mod internals {
    use crate::damage::region::{CollapsedDamage, DEFAULT_PASS_BUDGET_PX, DamageRegion};
    use crate::internals::harness::UiHarness;

    impl UiHarness {
        /// Collapse this frame's raw damage rects as `DamageEngine::finish_region` does.
        pub(crate) fn collapsed_damage(&self) -> CollapsedDamage {
            DamageRegion::collapse_from(
                &self.engines.damage.raw_rects,
                DEFAULT_PASS_BUDGET_PX,
                self.ui.display().logical_rect(),
            )
        }

        pub(crate) fn damage_region(&self) -> DamageRegion {
            self.collapsed_damage().region
        }
    }
}

#[cfg(test)]
mod unit {
    use crate::animation::animatable::Animatable;
    use crate::damage::Damage;
    use crate::damage::region::DamageRegion;
    use crate::internals::harness::UiHarness;
    use crate::internals::paint_capture::PaintCapture;
    use crate::internals::record_app::RecordApp;
    use crate::layout::intrinsic::len_req::LenReq;
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::identity::widget_id::WidgetId;
    use crate::primitives::layout::axis::Axis;
    use crate::primitives::layout::sizing::Sizing;
    use crate::renderer::frontend::encoder;
    use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
    use crate::renderer::render_plan::RenderPlan;
    use crate::scene::endpoint::Endpoint;
    use crate::scene::layer::Layer;
    use crate::scene::tree::node_id::NodeId;
    use crate::ui::Ui;
    use crate::ui::frame_report::FrameReport;
    use crate::widget_core::configure::Configure;
    use crate::widgets::panel::Panel;
    use glam::UVec2;

    impl UiHarness {
        /// Cold recorder (`prev_stamp` unseeded): frame 1 runs the warmup pass.
        pub(crate) fn cold(surface: UVec2) -> Self {
            let mut harness = Self::new(surface);
            harness.ui.set_prev_stamp(None);
            harness
        }

        /// One frame with the damage baseline dropped, forcing a full repaint.
        pub(crate) fn frame_without_baseline(
            &mut self,
            record: impl FnMut(&mut Ui),
        ) -> FrameReport {
            self.drive(false, &mut RecordApp::new(record))
        }

        /// A `FILL`/`FILL` hstack wrapped around `f`, returning `f`'s node.
        pub(crate) fn under_outer<R>(&mut self, mut f: impl FnMut(&mut Ui) -> R) -> R {
            self.frame_value(|ui| {
                Panel::hstack()
                    .auto_id()
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, &mut f)
                    .inner
            })
        }

        pub(crate) fn node_for_widget_id(&self, id: WidgetId) -> NodeId {
            self.ui.forest().node_for_widget_id(Layer::Main, id)
        }

        pub(crate) fn intrinsic(&mut self, node: NodeId, axis: Axis, req: LenReq) -> f32 {
            self.engines
                .layout
                .main_intrinsic(self.ui.forest(), node, axis, req)
        }

        pub(crate) fn node_of(&self, id: WidgetId) -> Option<Endpoint> {
            self.ui.cascade().endpoint(id)
        }

        pub(crate) fn main_child_ids(&self, parent: NodeId) -> Vec<NodeId> {
            self.ui
                .tree(Layer::Main)
                .children(parent)
                .map(|child| child.id)
                .collect()
        }

        pub(crate) fn main_child_rects(&self, parent: NodeId) -> Vec<Rect> {
            self.ui
                .tree(Layer::Main)
                .children(parent)
                .map(|child| self.ui.layout(Layer::Main).rect[child.id.idx()])
                .collect()
        }

        pub(crate) fn anim_row_count<T: Animatable>(&mut self) -> usize {
            self.ui.anim_mut().row_count::<T>()
        }

        pub(crate) fn encode_paint(&self) -> PaintCapture {
            self.encode(Damage::Full)
        }

        pub(crate) fn encode_paint_for(&self, region: DamageRegion) -> PaintCapture {
            self.encode(Damage::Partial(region.unmeasured()))
        }

        fn encode(&self, damage: Damage) -> PaintCapture {
            let plan = RenderPlan {
                clear: self.ui.theme().window_clear,
                damage,
            };
            encoder::internals::encode(self.ui.frame_scene(), &SharedGradientAtlas::default(), plan)
        }
    }
}

#[cfg(test)]
mod tests;
