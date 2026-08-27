//! Frame-driving test harness — the intended single entry point for
//! driving a [`Ui`] with synthetic input, in-crate and (once exported)
//! from consumers.
//!
//! # The one fact everything follows from
//!
//! [`Ui::frame`] calls the record closure **one, two, or three times**,
//! and each call sees different input:
//!
//! | pass | when it runs | what it sees |
//! |---|---|---|
//! | warmup | `prev_stamp.is_none()` — the very first frame | `InputState` swapped for an empty one: no pointer, no keys. Purely to build the cascade so pass A hit-tests against something. |
//! | A | always (except `PaintOnly`) | The real input, including one-frame edges — `clicked`, `drag.started()`. |
//! | B | `frame_had_action` \|\| `relayout_requested` | Post-`drain_per_frame_queues`: the edges are **gone**. Capped at one retry. |
//!
//! `FrameProcessing` reports `SingleLayout` / `DoubleLayout` — it counts
//! A and B, not the warmup. `PaintOnly` is a fourth case that runs
//! **zero** record passes. Every rule below is a corollary, and this
//! type holds the display, the clock, and the press origin so they can
//! be *enforced* rather than documented. It deliberately holds *no*
//! copy of anything `InputState` already owns — the pointer position
//! and the modifier set are read back out of it, because a mirror
//! desyncs the moment one event goes through [`UiHarness::on_input`].
//!
//! Holding them is also why there is one frame driver and not a matrix:
//! a caller changes the surface with [`UiHarness::resize`] and the clock
//! with [`UiHarness::at`], then calls [`UiHarness::frame`]. Passing a
//! `Display` per frame only ever let a test disagree with itself about
//! what it was rendering at.
//!
//! # The protocol rules
//!
//! None of these appear in a signature. They are what a caller driving
//! frames by hand gets silently wrong.
//!
//! ## Passes
//!
//! 1. **Warm the recorder.** A bare `Ui` is cold, so frame 1 runs the
//!    warmup pass; seeding `prev_stamp` skips it. That is the split
//!    between the test-gated `UiHarness::cold` and every other
//!    constructor. On a cold
//!    recorder "the first pass" means the input-blind one, so rules 3–4
//!    resolve to the wrong pass.
//! 2. **Prime before reading.** `response_for`'s `rect` / `layout_rect`
//!    / `hovered` / `disabled` come from *last* frame's cascade. Any
//!    input assertion needs a prior frame; a stable arranged rect needs
//!    two ([`UiHarness::prime`]). Content sized only after arrange
//!    (scroll thumbs, container text) can need a third — pass the count
//!    you need rather than guessing two.
//! 3. **Read the response inside the record.** Between frames you get
//!    the prior frame's input — the `frame_quiescent` snapshot is taken
//!    at record-pass start. That is what [`UiHarness::response_in`] is.
//! 4. **Read the first record pass.** Pass B runs after
//!    `drain_per_frame_queues`, which clears the one-frame edges, so
//!    [`UiHarness::frame_value`] keeps pass A's value while still
//!    recording both.
//! 5. **The closure's *side effects* run once per pass too.** The
//!    write-direction peer of 3–4, and the easiest to miss: a closure
//!    that pushes into a `Vec` or drains an intent queue does it twice
//!    on an action frame and twice on frame 1. Anything accumulating
//!    *across* frames must read pass one only or be idempotent.
//!
//! ## Clocks — there are two, and they diverge
//!
//! 6. **The frame clock only moves at a frame boundary.** `Ui::frame`
//!    copies its stamp into `input.frame_time`, so events fed between
//!    frames carry the value the *last* frame published. Moving time is
//!    [`advance`](UiHarness::advance) or [`at`](UiHarness::at) → `frame`
//!    → *then* the input; either one with no frame between changes
//!    nothing.
//! 7. **A frozen clock makes every click simultaneous.**
//!    `DOUBLE_CLICK_WINDOW` is 500 ms against `input.frame_time`, so
//!    without an `advance` a second `click_at` within
//!    `DOUBLE_CLICK_RADIUS` (5 px) *always* reports `double_clicked`.
//!    Two deliberately separate clicks need
//!    [`advance_past_double_click`](UiHarness::advance_past_double_click)
//!    or >5 px of travel.
//! 8. **Animation time is not the frame clock.** `advance_clock` clamps
//!    per-frame animation dt to `MAX_ANIM_DT` (0.1 s) and quantizes
//!    through an accumulator at `ANIM_SUBSTEP_DT` (1/240 s). One frame
//!    at +500 ms moves the double-click clock 500 ms and animations
//!    100 ms; one at +1 ms moves animations not at all. Use
//!    [`advance_frames`](UiHarness::advance_frames).
//! 9. **A frame can run no record pass.** `FrameProcessing::PaintOnly`
//!    fires when a paint-anim wake is the frame's only cause. A focused
//!    `TextEdit` is enough — its caret blink re-queues an `ANIM` wake
//!    every frame for 30 s. Hence
//!    [`try_frame_value`](UiHarness::try_frame_value).
//!
//! ## Coordinates, text, routing
//!
//! 10. **Surface size is physical; pointer positions are logical.**
//!     `Display::from_physical` derives logical as `physical / dpr`. At
//!     `dpr = 1.0` they coincide, which is why a DPI test can look right
//!     and not be.
//! 11. **Mono vs. real text.** [`UiHarness::new`] uses the mono fallback
//!     shaper; [`UiHarness::with_text`] uses cosmic with bundled Inter +
//!     JetBrains Mono. Anything whose width follows its label measures
//!     wrong under mono. But real shaping is *not* pixel-identical
//!     across machines — `FontSystem::new_with_fonts` also loads
//!     platform fonts as fallback — so assert relations, not exact
//!     widths.
//! 12. **Scroll and pinch route to the widget under the pointer at
//!     event time.** They carry no position of their own; `InputState`
//!     resolves them against the last `PointerMoved`. Hence each comes
//!     in two forms: [`pinch`](UiHarness::pinch) and friends fire at
//!     wherever the pointer already is, and the `_at` peers move it
//!     first. Signs follow winit: positive `y` scrolls content down.
//! 13. **Modifiers are sticky state, not per-event.**
//!     `ModifiersChanged` carries a snapshot that persists, so a chord
//!     set through [`set_modifiers`](UiHarness::set_modifiers) is still
//!     held by the *next* key until it is set back.
//!     `Modifiers.ctrl` is platform-normalized — Cmd on macOS.
//! 14. **Typed text arrives as `KeyDown { key: Key::Char(c) }`.** The
//!     winit host emits `InputEvent::Text` only from `Ime::Commit` and
//!     never calls `set_ime_allowed`, so that path is dead in production
//!     today. `TextEdit` consumes **both**, so emitting both
//!     double-inserts — hence [`type_text`](UiHarness::type_text) vs.
//!     [`ime_commit`](UiHarness::ime_commit).
//! 15. **Keyboard events are discarded at ingress when nothing is
//!     focused.** `InputState::on_input` gates `KeyDown` and `Text` on
//!     `focused.is_some() || subs.matches_press(kp) || keyboard_mask`,
//!     and drops what is not observable rather than queueing it. A
//!     keyboard test must establish focus first — by clicking, or via
//!     `Ui::request_focus` — or it asserts on a queue that can never
//!     fill.
//!
//! **Three tiers, one per block.** The whole module is already
//! `#[cfg(any(test, feature = "internals"))]`; the tiers say *who inside
//! that* a given method is for.
//!
//! 1. `impl UiHarness` (`pub`) — the surface that leaves the crate
//!    through `palantir::internals`, addressing widgets by [`WidgetId`]
//!    and nothing else.
//! 2. `impl UiHarness` (`pub(crate)`) — construction plus the one knob
//!    the **benches** read, `damage_region`. Benches compile under
//!    `internals` without `cfg(test)`, which is why that one method
//!    cannot drop to tier 3; `from_resources` stays because tier 1's
//!    constructors call it.
//! 3. `mod unit` (`#[cfg(test)]`) — what only the *in-tree* suite calls:
//!    the tree/encoder reach-ins, the cold constructor, the
//!    no-baseline frame driver.
//!
//! Tier 3's extra gate is not about encapsulation — `pub(crate)` already
//! stops all of this leaving the crate. It is the only way to say "the
//! benches don't use this": under `--features bench` alone, tier 3
//! simply isn't compiled, so nothing is spuriously unused and a
//! genuinely dead method still gets reported.
//!
//! No block here carries a lint allow. `palantir::internals` is gated on
//! `any(test, feature = "internals")` — the same condition as this
//! module — so tier 1 is reachable in every build that compiles it, and
//! `unreachable_pub` / `dead_code` stay live on all three tiers.

use crate::app::internals::RecordApp;
use crate::common::time::MAX_ANIM_DT;
use crate::display::Display;
use crate::host::shared::HostShared;
use crate::input::capture::{DOUBLE_CLICK_WINDOW, DRAG_THRESHOLD};
use crate::input::input_event::InputEvent;
use crate::input::keyboard::key::Key;
use crate::input::keyboard::modifiers::Modifiers;
use crate::input::keyboard::text_chunk::TextChunk;
use crate::input::pointer::PointerButton;
use crate::input::response::input_delta::InputDelta;
use crate::input::response::response_state::ResponseState;
use crate::input::sense::Sense;
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::renderer::texture_limit::TextureLimit;
// Carries `damage_region`'s gate: this whole module is build-gated test
// support, and under a non-test `internals` build that method is absent.
#[cfg(any(test, feature = "bench"))]
use crate::scene::damage::region::{CollapsedDamage, DamageRegion};
use crate::scene::seen_ids::Endpoint;
use crate::text::shaper::TextShaper;
use crate::ui::Ui;
use crate::ui::frame_engines::FrameEngines;
use crate::ui::frame_report::FrameReport;
use crate::ui::frame_stamp::FrameInput;
use crate::ui::frame_stamp::FrameStamp;
use crate::ui::resources::UiResources;
use crate::window::window_token::WindowToken;
use glam::{UVec2, Vec2};
use std::time::Duration;

/// Surface for [`UiHarness::arena`]. Never framed, so the value only has
/// to be non-degenerate.
const ARENA_SURFACE: UVec2 = UVec2::splat(1);

#[derive(Debug)]
pub struct UiHarness {
    /// `pub(crate)` rather than behind an accessor: the recorder state the
    /// in-crate suites assert on is reached through `Ui`'s own gated
    /// accessors (`h.ui.cascade()`, `h.ui.forest()`), so wrapping the field
    /// too would only add a hop. Consumers cannot see it and go through
    /// [`Self::ui`].
    pub(crate) ui: Ui,
    /// The engines this harness's frames run on. `pub(crate)` for the same
    /// reason [`Self::ui`] is: in-crate damage and layout tests assert on
    /// engine internals, and reaching them off the harness is the only door
    /// now that `Ui` no longer holds them.
    pub(crate) engines: FrameEngines,
    /// What every frame stamps with — the harness owns it so no caller
    /// has to rebuild one. `physical` is in physical pixels; pointer
    /// positions are logical (see [`Self::scale`]).
    display: Display,
    /// Absolute; each frame stamps with it. Only moves on
    /// [`Self::advance`] / [`Self::at`].
    time: Duration,
    /// Press origin, solely so `drag_to` can check `DRAG_THRESHOLD`.
    pressed_at: Option<Vec2>,
}

/// Tier 1 — the surface that leaves the crate. See the module doc for
/// why this block, and only this block, silences the two reachability
/// lints.
impl UiHarness {
    /// `UiResources::isolated_mono` — mono-fallback text: fast,
    /// deterministic, and wrong for width-follows-label assertions.
    pub fn new(surface: UVec2) -> Self {
        Self::from_resources(UiResources::isolated_mono(), surface)
    }

    /// Real cosmic shaping, over a thread-local shared shaper. Use when
    /// anything under test sizes to its text. Metrics are not identical
    /// across machines — the bundled faces are joined by platform fonts
    /// as fallback — so assert relations, not exact widths.
    pub fn with_text(surface: UVec2) -> Self {
        thread_local! {
            static SHARED: TextShaper = TextShaper::new();
        }
        let shared = HostShared::new(SHARED.with(Clone::clone), TextureLimit::default());
        Self::from_resources(shared.resources.clone(), surface)
    }

    /// A harness that is never framed — its [`Self::ui`] is a
    /// string-interning arena for tests that build `InternedStr`-bearing
    /// projections without recording. Exists because `InternedStr` is
    /// public and `Ui::intern` is the only public way to mint one.
    pub fn arena() -> Self {
        Self::new(ARENA_SURFACE)
    }

    /// Device pixel ratio. The surface stays physical, so at `dpr = 2.0`
    /// a 600×200 surface is 300×100 logical — and every position below is
    /// logical.
    pub fn scale(mut self, dpr: f32) -> Self {
        self.display.scale_factor = dpr;
        self.sync_display();
        self.mark_warm();
        self
    }

    /// Monitor refresh, which repaint-wake coalescing reads.
    pub fn refresh_millihertz(mut self, mhz: u32) -> Self {
        self.display.refresh_millihertz = Some(mhz);
        self.sync_display();
        self.mark_warm();
        self
    }

    pub fn pixel_snap(mut self, on: bool) -> Self {
        self.display.pixel_snap = on;
        self.sync_display();
        self.mark_warm();
        self
    }

    /// Change the surface between frames — the resize path. Takes
    /// `&mut self` rather than `self` for a reason the three builders
    /// above do not share: it deliberately does **not** re-warm, so the
    /// next frame reads it as `display_changed` exactly as a real resize
    /// does.
    pub fn resize(&mut self, surface: UVec2) -> &mut Self {
        self.display.physical = surface;
        self.sync_display();
        self
    }

    /// Swap the whole display between frames, for the changes
    /// [`Self::resize`] cannot express — a DPI move (physical *and*
    /// scale together), a pixel-snap flip. Same no-re-warm contract.
    pub fn set_display(&mut self, display: Display) -> &mut Self {
        self.display = display;
        self.sync_display();
        self
    }

    pub fn frame(&mut self, record: impl FnMut(&mut Ui)) -> FrameReport {
        self.drive(true, record)
    }

    /// The value from the **input-observing** pass — pass A, the one
    /// that sees one-frame edges (`clicked`, `drag.started()`). Panics
    /// if the frame ran no record pass at all; see [`Self::try_frame_value`].
    pub fn frame_value<R>(&mut self, record: impl FnMut(&mut Ui) -> R) -> R {
        self.try_frame_value(record).expect(
            "the frame ran no record pass — FrameProcessing::PaintOnly. A paint-anim \
             wake was the frame's only cause (a focused TextEdit's caret blink is \
             enough). Feed an input, request a repaint, or use `try_frame_value`.",
        )
    }

    /// [`Self::frame_value`] without the `PaintOnly` panic, for callers
    /// deliberately driving paint-anim frames.
    pub fn try_frame_value<R>(&mut self, mut record: impl FnMut(&mut Ui) -> R) -> Option<R> {
        let mut first = None;
        self.frame(|ui| {
            // `record` runs on *every* pass — it is the scene, and a pass
            // that skipped it would record an empty tree and wipe the
            // cascade the next frame reads. Only the value is pass A's.
            let value = record(ui);
            if first.is_none() {
                first = Some(value);
            }
        });
        first
    }

    /// `n` discarded frames. Two is the usual minimum: one to lay out,
    /// one for `response_for` to resolve against a settled cascade.
    ///
    /// Named `prime`, not `settle` — palantir already uses "settle" for
    /// the second record pass *within* one frame.
    pub fn prime(&mut self, n: u32, mut record: impl FnMut(&mut Ui)) {
        for _ in 0..n {
            self.frame(&mut record);
        }
    }

    /// Move the absolute clock. Takes effect on the **next** frame, and
    /// only then do subsequent input events carry it: `Ui::frame` is what
    /// publishes the clock to the input machine, so the order is
    /// `advance` → `frame` → input.
    ///
    /// Animation dt is a separate clock — it is clamped per frame to
    /// `MAX_ANIM_DT`, so one big jump here does not integrate one big
    /// step there. Use [`Self::advance_frames`] for that.
    pub fn advance(&mut self, dt: Duration) -> &mut Self {
        self.time += dt;
        self
    }

    /// Park the absolute clock at `time` — [`Self::advance`] for a test
    /// written against absolute stamps rather than deltas. Same timing
    /// rule: it reaches the input machine only through the next frame,
    /// so the order is `at` → `frame` → input.
    pub fn at(&mut self, time: Duration) -> &mut Self {
        self.time = time;
        self
    }

    /// `n` frames stepping `dt` each — the correct way to move an
    /// animation, since a single large jump is clamped to `MAX_ANIM_DT`.
    ///
    /// No caller yet: the animation suite drives absolute stamps through
    /// [`Self::at`]. Kept because the assert below is the crate's only
    /// guard on rule 8, and a test that trips it fails silently — it
    /// under-integrates rather than panicking.
    pub fn advance_frames(&mut self, n: u32, dt: Duration, mut record: impl FnMut(&mut Ui)) {
        assert!(
            dt.as_secs_f32() <= MAX_ANIM_DT,
            "a {dt:?} step exceeds MAX_ANIM_DT ({MAX_ANIM_DT}s) and would silently \
             under-integrate the animation; use more, smaller frames",
        );
        for _ in 0..n {
            self.advance(dt);
            self.frame(&mut record);
        }
    }

    /// One frame past `DOUBLE_CLICK_WINDOW`, so the next click starts a
    /// fresh press run. Without this the clock never moves, every click
    /// is simultaneous, and a second `click_at` on the same spot always
    /// reports `double_clicked`.
    ///
    /// No caller outside this module's own tests — the multi-click suite
    /// uses absolute stamps. Kept as rule 7's remedy: it is where the
    /// window constant and the mandatory intervening frame live.
    pub fn advance_past_double_click(&mut self, record: impl FnMut(&mut Ui)) {
        self.advance(DOUBLE_CLICK_WINDOW + Duration::from_millis(1));
        self.frame(record);
    }

    /// The raw input door, for events with no typed helper above — a
    /// `KeyDown` with a specific `physical`, an `InputEvent::Text` you
    /// built yourself. Everything the typed helpers cover should go
    /// through them: they are what enforce the press-origin, modifier,
    /// and threshold rules, and a helper that emits exactly one event
    /// hands back that event's [`InputDelta`] so nothing is given up by
    /// using it.
    pub fn on_input(&mut self, event: InputEvent) -> InputDelta {
        self.ui.on_input(event)
    }

    /// Returns the move's [`InputDelta`] — `on_input`'s value, so a
    /// repaint-hint assertion has no reason to build the event by hand.
    pub fn move_to(&mut self, pos: Vec2) -> InputDelta {
        self.ui.on_input(InputEvent::PointerMoved(pos))
    }

    pub fn pointer_left(&mut self) -> InputDelta {
        self.ui.on_input(InputEvent::PointerLeft)
    }

    /// Press wherever the pointer already is — the peer of
    /// [`Self::release`], for a press deliberately separated from the
    /// move that positioned it. The press origin is read back from
    /// `InputState` rather than a second copy on this type, so a press
    /// that follows a raw `PointerMoved` still latches
    /// [`Self::drag_to`]'s threshold check.
    pub fn press(&mut self) -> InputDelta {
        self.press_button(PointerButton::Left)
    }

    pub fn press_button(&mut self, button: PointerButton) -> InputDelta {
        self.pressed_at = self.ui.input.pointer_pos;
        self.ui.on_input(InputEvent::PointerPressed(button))
    }

    pub fn press_at(&mut self, pos: Vec2) -> InputDelta {
        self.press_button_at(PointerButton::Left, pos)
    }

    pub fn press_button_at(&mut self, button: PointerButton, pos: Vec2) -> InputDelta {
        self.move_to(pos);
        self.press_button(button)
    }

    pub fn release(&mut self) -> InputDelta {
        self.release_button(PointerButton::Left)
    }

    pub fn release_button(&mut self, button: PointerButton) -> InputDelta {
        self.pressed_at = None;
        self.ui.on_input(InputEvent::PointerReleased(button))
    }

    pub fn click_at(&mut self, pos: Vec2) {
        self.click_button_at(PointerButton::Left, pos);
    }

    pub fn right_click_at(&mut self, pos: Vec2) {
        self.click_button_at(PointerButton::Right, pos);
    }

    /// The button-generic form the two above name — for a test sweeping
    /// every [`PointerButton`] rather than exercising a named one.
    pub fn click_button_at(&mut self, button: PointerButton, pos: Vec2) {
        self.press_button_at(button, pos);
        self.release_button(button);
    }

    /// `id`'s center, **checked**: panics unless the pointer would
    /// actually reach `id` there.
    ///
    /// The check is the point. A widget's center is not the same thing
    /// as a hit on that widget — anything overlapping it that senses
    /// hover wins the topmost-first scan, so aiming at a rect and
    /// assuming the event lands is how a test ends up passing for the
    /// wrong reason. The `_on` / `_onto` helpers below all route through
    /// here; a case that *wants* to aim at an occluded widget is
    /// deliberate and says so by computing the position itself.
    ///
    /// Reads last frame's cascade, so it needs a primed frame — see
    /// [`Self::center_of`] for the failure when nothing recorded.
    ///
    /// The filter is "senses anything", deliberately wider than
    /// [`Self::hit_at`]'s hover filter: a `SCROLL`-only widget is
    /// invisible to the hover layer *by design* (`Sense::hovers`), so
    /// checking against hover alone would reject aiming at a scroll or
    /// pinch target — which is one of the main reasons to aim at all.
    fn hit_center_of(&self, id: WidgetId) -> Vec2 {
        let center = self.center_of(id);
        let senses_anything = |sense: Sense| sense != Sense::NONE;
        let hit = self.ui.cascade.hit_test(center, senses_anything);
        assert_eq!(
            hit,
            Some(id),
            "{id:?} does not receive the pointer at its own center {center:?} — \
             {hit:?} is on top there. Aim by position if that is intended.",
        );
        center
    }

    /// Click `id` at its center. See `Self::hit_center_of` for what is
    /// checked and why.
    pub fn click_on(&mut self, id: WidgetId) {
        self.click_at(self.hit_center_of(id));
    }

    pub fn right_click_on(&mut self, id: WidgetId) {
        self.right_click_at(self.hit_center_of(id));
    }

    pub fn press_on(&mut self, id: WidgetId) -> InputDelta {
        self.press_at(self.hit_center_of(id))
    }

    /// Hover `id` — the move half of [`Self::press_on`], for a test that
    /// aims at a widget and then feeds something positionless.
    pub fn move_onto(&mut self, id: WidgetId) -> InputDelta {
        self.move_to(self.hit_center_of(id))
    }

    /// Move while pressed, returning the move's [`InputDelta`] like
    /// [`Self::move_to`]. Panics if travel since the press has not
    /// crossed `DRAG_THRESHOLD` — the capture would not latch and the
    /// test would pass or fail for the wrong reason. A deliberate
    /// sub-threshold move is [`Self::move_to`].
    pub fn drag_to(&mut self, pos: Vec2) -> InputDelta {
        let origin = self
            .pressed_at
            .expect("drag_to needs a press first — no button is down");
        let travel = origin.distance(pos);
        assert!(
            travel >= DRAG_THRESHOLD,
            "drag_to({pos:?}) travels {travel} px from the press at {origin:?}, under \
             the {DRAG_THRESHOLD} px DRAG_THRESHOLD — no drag would latch",
        );
        self.move_to(pos)
    }

    /// Scroll and pinch carry no position of their own: `InputState`
    /// routes them to whatever the pointer was last over. These bare
    /// forms are for a gesture deliberately separated from the move that
    /// aimed it — the `_at` peers below are the same events with the aim
    /// inlined. Positive `y` means the content scrolls down.
    pub fn scroll_lines(&mut self, delta: Vec2) -> InputDelta {
        self.ui.on_input(InputEvent::ScrollLines(delta))
    }

    pub fn scroll_pixels(&mut self, delta: Vec2) -> InputDelta {
        self.ui.on_input(InputEvent::ScrollPixels(delta))
    }

    pub fn pinch(&mut self, factor: f32) -> InputDelta {
        self.ui.on_input(InputEvent::Zoom(factor))
    }

    /// Aim, then scroll — the delta returned is the scroll's, not the
    /// positioning move's.
    pub fn scroll_lines_at(&mut self, pos: Vec2, delta: Vec2) -> InputDelta {
        self.move_to(pos);
        self.scroll_lines(delta)
    }

    pub fn scroll_pixels_at(&mut self, pos: Vec2, delta: Vec2) -> InputDelta {
        self.move_to(pos);
        self.scroll_pixels(delta)
    }

    pub fn pinch_at(&mut self, pos: Vec2, factor: f32) -> InputDelta {
        self.move_to(pos);
        self.pinch(factor)
    }

    /// A non-repeat press whose `physical` is [`Key::Other`]. That is
    /// only consulted for a non-ASCII `Char` under a command modifier
    /// (`Shortcut::matches`), so it is inert for every other key; a case
    /// that needs a real physical position builds the event through
    /// [`Self::on_input`].
    pub fn key(&mut self, key: Key) -> InputDelta {
        self.ui.on_input(InputEvent::KeyDown {
            key,
            repeat: false,
            physical: Key::Other,
        })
    }

    /// Emits `ModifiersChanged` only when the set actually changes —
    /// `Modifiers` is a snapshot the input machine holds, not a per-event
    /// flag, and a redundant emit would wake a `KeyboardWake::MODIFIER`
    /// watcher for nothing. That conditional emit is also why this
    /// returns no [`InputDelta`] where its neighbours do: a test
    /// asserting on the modifier wake wants the event unconditionally
    /// and goes through [`Self::on_input`].
    ///
    /// The "actually changes" is read off `InputState`, not a mirror on
    /// this type. A mirror desyncs the moment one modifier goes through
    /// [`Self::on_input`] — and then this suppresses the very emit that
    /// would have cleared it, leaving a chord silently held.
    pub fn set_modifiers(&mut self, mods: Modifiers) {
        if self.ui.input.modifiers != mods {
            self.ui.on_input(InputEvent::ModifiersChanged(mods));
        }
    }

    /// One `KeyDown { key: Key::Char(c) }` per char — the path a real
    /// window produces. The winit host emits `InputEvent::Text` only from
    /// an IME commit; see [`Self::ime_commit`], and do not use both for
    /// the same text (`TextEdit` consumes each, so it would double-insert).
    pub fn type_text(&mut self, s: &str) {
        for c in s.chars() {
            self.key(Key::Char(c));
        }
    }

    /// The IME path: `InputEvent::Text`, split exactly as a commit is.
    pub fn ime_commit(&mut self, s: &str) {
        for chunk in TextChunk::split(s) {
            self.ui.on_input(InputEvent::Text(chunk));
        }
    }

    /// The **visible** rect from the previous frame's cascade — after
    /// ancestor transforms and clipping, so it is what the pointer
    /// actually hits and what [`Self::center_of`] aims at. For the
    /// arranged geometry a layout assertion wants, see
    /// [`Self::layout_rect`]; the two differ under any transform or
    /// clip, and only there, which is what makes picking the wrong one
    /// pass everywhere except the scroll and canvas cases.
    ///
    /// Safe to read between frames — geometry is stable across them,
    /// one-frame edges are not.
    pub fn rect(&self, id: WidgetId) -> Option<Rect> {
        self.ui.response_for(id).rect
    }

    /// The **arranged** rect: pre-transform and unclipped, in world
    /// coordinates — what the layout pass produced, before a scrolling
    /// or zooming ancestor moved it. This is the one a layout test
    /// means; [`Self::rect`] is the post-transform peer.
    ///
    /// Equal to indexing the layout pass output by the widget's node,
    /// without making a test carry a `NodeId` to get there.
    pub fn layout_rect(&self, id: WidgetId) -> Option<Rect> {
        self.ui.response_for(id).layout_rect
    }

    /// Center of `id`'s arranged rect.
    pub fn center_of(&self, id: WidgetId) -> Vec2 {
        self.rect(id)
            .unwrap_or_else(|| {
                panic!(
                    "{id:?} has no arranged rect — it did not record, or nothing primed the frame"
                )
            })
            .center()
    }

    /// `id`'s response captured inside pass A — the only correct way to
    /// read a one-frame edge, since reading between frames sees the
    /// previous frame's input and pass B has already had the edges drained.
    pub fn response_in(&mut self, id: WidgetId, mut record: impl FnMut(&mut Ui)) -> ResponseState {
        self.frame_value(move |ui| {
            record(ui);
            ui.response_for(id)
        })
    }

    /// Focus and pointer reads that carry no protocol hazard — unlike
    /// `response_for`, none of these is one-frame-stale in a way that
    /// makes a between-frames read wrong. `response_for` deliberately
    /// stays off this rung; use [`Self::rect`] for geometry and
    /// [`Self::response_in`] for edges.
    pub fn focused_id(&self) -> Option<WidgetId> {
        self.ui.focused_id()
    }

    pub fn request_focus(&mut self, id: Option<WidgetId>) {
        self.ui.request_focus(id);
    }

    pub fn focus_within(&self, ancestor: WidgetId) -> bool {
        self.ui.focus_within(ancestor)
    }

    pub fn hover_within(&self, ancestor: WidgetId) -> bool {
        self.ui.hover_within(ancestor)
    }

    pub fn pointer_pos(&mut self) -> Option<Vec2> {
        self.ui.pointer_pos()
    }

    pub fn escape_pressed(&mut self) -> bool {
        self.ui.escape_pressed()
    }

    /// Topmost widget the pointer would hit at `pos`, by the same filter
    /// hover routing uses. Turns "the press didn't land and I don't know
    /// why" into one assertion.
    pub fn hit_at(&self, pos: Vec2) -> Option<WidgetId> {
        self.ui.cascade.hit_test(pos, Sense::hovers)
    }

    /// Explicit-id collisions recorded last frame, as the colliding
    /// pairs. These otherwise surface only as a magenta runtime overlay,
    /// which no test can see.
    pub fn collisions(&self) -> Vec<(WidgetId, WidgetId)> {
        self.ui
            .forest
            .collisions
            .iter()
            .map(|record| {
                let id_of = |endpoint: Endpoint| {
                    self.ui.forest.trees[endpoint.layer].records.widget_id()[endpoint.node.idx()]
                };
                (id_of(record.first), id_of(record.second))
            })
            .collect()
    }

    pub fn clipboard_text(&self) -> String {
        self.ui.resources.clipboard.get()
    }

    pub fn set_clipboard_text(&mut self, text: &str) {
        self.ui
            .resources
            .clipboard
            .set(text)
            .expect("the memory clipboard is always available");
    }

    /// Escape hatch. Reading `response_for` off this between frames sees
    /// the previous frame's input — prefer [`Self::response_in`].
    pub fn ui(&mut self) -> &mut Ui {
        &mut self.ui
    }
}

/// The in-crate rung: tree, encoder, damage, and the schedule knobs.
/// None of this leaves the crate when the type is exported.
impl UiHarness {
    /// Two recorders over one `HostShared`, for the shared-text-cache and
    /// idle/active-window tests.
    pub(crate) fn from_resources(resources: UiResources, surface: UVec2) -> Self {
        let mut harness = Self {
            engines: FrameEngines::new(&resources),
            ui: Ui::new(resources),
            display: Display::from_physical(surface, 1.0),
            time: Duration::ZERO,
            pressed_at: None,
        };
        harness.sync_display();
        harness.mark_warm();
        harness
    }

    /// The one place a frame is actually entered. `Ui::frame` is
    /// `pub(crate)`, so the harness drives it directly rather than
    /// through a test method on `Ui`.
    fn drive(&mut self, damage_baseline_valid: bool, record: impl FnMut(&mut Ui)) -> FrameReport {
        let mut app = RecordApp::new(record);
        self.ui.frame(
            &mut self.engines,
            FrameInput {
                stamp: FrameStamp::new(self.display, self.time),
                damage_baseline_valid,
            },
            WindowToken(0),
            &mut app,
        )
    }

    fn sync_display(&mut self) {
        self.ui.display = self.display;
    }

    /// Seed `prev_stamp` so frame 1 skips the cold-start warmup pass and
    /// runs one record pass like every later frame.
    fn mark_warm(&mut self) {
        self.ui.frame_runtime.prev_stamp = Some(FrameStamp::new(self.display, self.time));
    }

    /// Collapse this frame's accumulated raw rects the way
    /// `DamageEngine::finish_region` does — the rects *and* the coverage
    /// they cover the surface with.
    ///
    /// Read by the crate's own damage tests and the `damage` bench; a
    /// non-test `internals` build has no caller.
    #[cfg(any(test, feature = "bench"))]
    pub(crate) fn collapsed_damage(&self) -> CollapsedDamage {
        DamageRegion::collapse_from(
            &self.engines.damage.raw_rects,
            self.engines.damage.budget_px,
            self.ui.display.logical_rect(),
        )
    }

    /// Just the rects — what most damage assertions are about.
    #[cfg(any(test, feature = "bench"))]
    pub(crate) fn damage_region(&self) -> DamageRegion {
        self.collapsed_damage().region
    }
}

/// The third tier: everything only the *in-tree* suite calls.
///
/// The module as a whole is already `any(test, feature = "internals")`,
/// and `pub(crate)` already stops any of this leaving the crate. This
/// narrower `cfg(test)` says something the other two cannot — that the
/// benches, which compile under `internals` alone, do not use it. Keeping
/// it as one gated `mod` rather than attributes sprinkled through the
/// rung above means the tier is visible at a glance, its imports live
/// with it, and nothing needs a `dead_code` allow.
#[cfg(test)]
mod unit {
    use crate::animation::animatable::Animatable;
    use crate::layout::types::sizing::Sizing;
    use crate::primitives::rect::Rect;
    use crate::primitives::widget_id::WidgetId;
    use crate::renderer::frontend::capture::PaintCapture;
    use crate::renderer::frontend::encoder;
    use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
    use crate::renderer::render_plan::{RenderKind, RenderPlan};
    use crate::scene::damage::region::DamageRegion;
    use crate::scene::layer::Layer;
    use crate::scene::node::configure::Configure;
    use crate::scene::tree::node_id::NodeId;
    use crate::ui::Ui;
    use crate::ui::frame_report::FrameReport;
    use crate::ui::harness::UiHarness;
    use crate::widgets::panel::Panel;
    use glam::UVec2;

    impl UiHarness {
        /// Cold recorder — `prev_stamp` unseeded, so frame 1 runs the
        /// extra blackout warmup pass. For the tests that pin cold start
        /// itself; every other constructor is warm.
        pub(crate) fn cold(surface: UVec2) -> Self {
            let mut harness = Self::new(surface);
            harness.ui.frame_runtime.prev_stamp = None;
            harness
        }

        /// One frame with the damage baseline dropped — the host's "the
        /// last frame never reached the screen" path
        /// (`WindowDriver::output_valid == false`), which forces a full
        /// repaint. Only the damage tests pinning *that* want it: a test
        /// after a record pass, a full repaint, or a settled layout gets
        /// all three from plain [`UiHarness::frame`].
        pub(crate) fn frame_without_baseline(
            &mut self,
            record: impl FnMut(&mut Ui),
        ) -> FrameReport {
            self.drive(false, record)
        }

        /// A `FILL`/`FILL` hstack wrapped around `f`, returning the node
        /// `f` produced — the fixture for "arrange this one subtree
        /// against the whole surface".
        pub(crate) fn under_outer<F: FnMut(&mut Ui) -> NodeId>(&mut self, mut f: F) -> NodeId {
            self.frame_value(|ui| {
                Panel::hstack()
                    .auto_id()
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, &mut f)
                    .inner
            })
        }

        pub(crate) fn node_for_widget_id(&self, id: WidgetId) -> NodeId {
            self.ui.forest.node_for_widget_id(Layer::Main, id)
        }

        pub(crate) fn main_child_ids(&self, parent: NodeId) -> Vec<NodeId> {
            self.ui.forest.trees[Layer::Main]
                .children(parent)
                .map(|child| child.id)
                .collect()
        }

        pub(crate) fn main_child_rects(&self, parent: NodeId) -> Vec<Rect> {
            self.ui.forest.trees[Layer::Main]
                .children(parent)
                .map(|child| self.ui.layout[Layer::Main].rect[child.id.idx()])
                .collect()
        }

        pub(crate) fn anim_row_count<T: Animatable>(&mut self) -> usize {
            self.ui
                .anim
                .try_typed_mut::<T>()
                .map_or(0, |rows| rows.rows.len())
        }

        pub(crate) fn encode_paint(&self) -> PaintCapture {
            self.encode(RenderKind::Full)
        }

        pub(crate) fn encode_paint_for(&self, region: DamageRegion) -> PaintCapture {
            self.encode(RenderKind::Partial {
                damage: region.unmeasured(),
            })
        }

        fn encode(&self, kind: RenderKind) -> PaintCapture {
            let plan = RenderPlan {
                clear: self.ui.theme.window_clear,
                kind,
            };
            encoder::test_support::encode(
                self.ui.frame_scene(),
                &SharedGradientAtlas::default(),
                plan,
            )
        }
    }
}

#[cfg(test)]
mod tests;
