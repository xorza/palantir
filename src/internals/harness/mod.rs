//! Frame-driving test harness — the intended single entry point for
//! driving a [`Ui`] with synthetic input, in-crate and from consumers.
//!
//! # The one fact everything follows from
//!
//! `Ui::frame` calls the record closure **one, two, or three times**,
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
//!    constructor. On a cold recorder "the first pass" means the
//!    input-blind one, so rules 3–4 resolve to the wrong pass.
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
//! 6. **One clock, read at two doors.** [`advance`](UiHarness::advance)
//!    and [`at`](UiHarness::at) move `UiHarness::time`, which stamps the
//!    frames this harness drives *and* the input events it feeds — the
//!    way a host reads its own clock at both. So an `advance` reaches
//!    input timing with no frame in between, and a frame with no
//!    `advance` moves nothing.
//! 7. **A frozen clock makes every click simultaneous.**
//!    `DOUBLE_CLICK_WINDOW` is 500 ms against that clock, so without an
//!    `advance` a second `click_at` within `DOUBLE_CLICK_RADIUS` (5 px)
//!    *always* reports `double_clicked`. Two deliberately separate
//!    clicks need an `advance` past the window — which is what
//!    [`advance_past_double_click`](UiHarness::advance_past_double_click)
//!    spells — or >5 px of travel.
//! 8. **Animation time is not the frame clock.** `advance_clock` clamps
//!    per-frame animation dt to `MAX_ANIM_DT` (0.1 s) and quantizes
//!    through an accumulator at `ANIM_SUBSTEP_DT` (1/240 s). One frame
//!    at +500 ms moves the double-click clock 500 ms and animations
//!    100 ms; one at +1 ms moves animations not at all. Use
//!    [`advance_frames`](UiHarness::advance_frames).
//! 9. **A frame can run no record pass.** `FrameProcessing::PaintOnly`
//!    fires when a paint-anim wake is the frame's only cause. A focused
//!    `TextEdit` is enough — its caret blink re-queues an `ANIM` wake
//!    every frame for 30 s. [`frame_value`](UiHarness::frame_value)
//!    panics on such a frame; [`frame_passes`](UiHarness::frame_passes)
//!    reports it as no passes.
//!
//! ## Coordinates, text, routing
//!
//! 10. **Surface size is physical; pointer positions are logical.**
//!     `Display::from_physical` derives logical as `physical / dpr`. At
//!     `dpr = 1.0` they coincide, which is why a DPI test can look right
//!     and not be.
//! 11. **Mono vs. real text.** [`UiHarness::new`] uses the mono fallback
//!     shaper; [`UiHarness::with_text`] shapes with the four bundled faces
//!     (`FontScope::Bundled`) and nothing from the machine, so its metrics
//!     are the same everywhere and a test may pin exact widths. Anything
//!     whose width follows its label measures wrong under mono.
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
//! 14. **A field types a press's `text`, not its key.** The harness
//!     reports one the way a window does — a printable key carries the
//!     character it produced, a named key carries none — so
//!     [`type_text`](UiHarness::type_text) and [`key`](UiHarness::key)
//!     both type. A case that needs several characters from one press,
//!     or a character on an unnamed key, builds the event through
//!     [`on_input`](UiHarness::on_input).
//! 15. **Keyboard events are discarded at ingress when nothing is
//!     focused.** `InputState::on_input` gates `KeyDown` on
//!     `focused.is_some() || subs.matches_press(kp) || keyboard_mask`,
//!     and drops what is not observable rather than queueing it. A
//!     keyboard test must establish focus first — by clicking, or via
//!     `Ui::set_focus` — or it asserts on a queue that can never
//!     fill.
//! 16. **One change of a kind per frame.** A second press or release
//!     of one button, or a key press after the frame's command key,
//!     waits for the next frame (`InputQueue`) — so `click_at` is a
//!     press frame and then a release frame. [`UiHarness::frame`] and
//!     every reader built on it run the frames held input owes before
//!     their own, with the same closure, and answer from the last one:
//!     `click_at` then `frame_value` reads the click. A test of the
//!     spread itself steps one frame at a time with [`UiHarness::step`].
//!
//! **Three tiers, one per block.** The whole module is already
//! `#[cfg(any(test, feature = "internals"))]`; the tiers say *who inside
//! that* a given method is for.
//!
//! 1. `impl UiHarness` (`pub`) — the surface that leaves the crate
//!    through `palantir::internals::harness`, addressing widgets by
//!    [`WidgetId`] and nothing else.
//! 2. `impl UiHarness` (`pub(crate)`) — construction, with
//!    `from_resources` there because tier 1's constructors call it. The
//!    two damage reads the **benches** make, `collapsed_damage` and
//!    `damage_region`, sit in `mod internals`: benches compile under
//!    `bench` without `cfg(test)`, so they cannot drop to tier 3.
//! 3. `mod unit` (`#[cfg(test)]`) — what only the *in-tree* suite calls:
//!    the tree/encoder reach-ins, the cold constructor, the
//!    no-baseline frame driver.
//!
//! Tier 3's extra gate is not about encapsulation — `pub(crate)` already
//! stops all of this leaving the crate. It is the only way to say "the
//! benches don't use this": under `--features bench` alone, tier 3
//! simply isn't compiled, so nothing is spuriously unused and a
//! genuinely dead method in tiers 2 and 3 still gets reported.
//!
//! No block here carries a lint allow. This module is `pub` under
//! `palantir::internals`, so tier 1 is reachable in every build that
//! compiles it. Being `pub`, a tier-1 method nobody calls is invisible
//! to `dead_code`, so one is pruned when its last caller goes.

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

/// Surface for [`UiHarness::arena`]. Never framed, so the value only has
/// to be non-degenerate.
const ARENA_SURFACE: UVec2 = UVec2::splat(1);

/// Drives a [`Ui`] through frames with synthetic input. See the module
/// doc for the pass model every helper here is built around.
#[derive(Debug)]
#[must_use]
pub struct UiHarness {
    /// `pub(crate)` rather than behind an accessor: the recorder state the
    /// in-crate suites assert on is reached through `Ui`'s own gated
    /// accessors (`h.ui.cascade()`, `h.ui.forest()`), so wrapping the field
    /// too would only add a hop. Consumers cannot see it and go through
    /// [`Self::ui`].
    pub(crate) ui: Ui,
    /// The engines this harness's frames run on. `pub(crate)` for the same
    /// reason [`Self::ui`] is: in-crate damage and layout tests assert on
    /// engine internals, and `Ui` does not hold the engines.
    pub(crate) engines: FrameEngines,
    /// What every frame stamps with — the harness owns it so no caller
    /// has to rebuild one. `physical` is in physical pixels; pointer
    /// positions are logical (see [`Self::scale`]). Its `user_scale` is
    /// not read: a frame takes [`Ui::user_scale`], as the window driver
    /// does.
    display: Display,
    /// Absolute; each frame stamps with it. Only moves on
    /// [`Self::advance`] / [`Self::at`].
    time: Duration,
    /// Which buttons this harness has pressed and not released, with the
    /// pointer position each press landed at. Per button, and written
    /// only in [`Self::on_input`], so a press fed through the raw door
    /// counts the same as one fed through a typed helper.
    held: [Option<PressOrigin>; PointerButton::COUNT],
}

/// Where a held button went down: `None` when the pointer was off the
/// surface at the press.
#[derive(Clone, Copy, Debug)]
struct PressOrigin {
    at: Option<Vec2>,
}

/// Tier 1 — the surface that leaves the crate.
impl UiHarness {
    /// `UiResources::isolated_mono` — mono-fallback text: fast,
    /// deterministic, and wrong for width-follows-label assertions.
    pub fn new(surface: UVec2) -> Self {
        Self::from_resources(UiResources::isolated_mono(), surface)
    }

    /// Real cosmic shaping over the bundled faces, through a shaper of
    /// this harness's own. Use when anything under test sizes to its text.
    /// The bundled faces are all it sees, so metrics are identical on every
    /// machine and exact widths are fair to assert.
    pub fn with_text(surface: UVec2) -> Self {
        Self::from_resources(UiResources::isolated_text(), surface)
    }

    /// A harness that is never framed — its [`Self::ui`] is a
    /// string-interning arena for tests that build `InternedStr`-bearing
    /// projections without recording. Exists because `InternedStr` is
    /// public and `Ui::intern` is the only public way to mint one.
    pub fn arena() -> Self {
        Self::new(ARENA_SURFACE)
    }

    /// Device pixel ratio, as a platform would report it. The surface
    /// stays physical, so at `dpr = 2.0` a 600×200 surface is 300×100
    /// logical — and every position below is logical.
    pub fn scale(mut self, dpr: f32) -> Self {
        self.display.system_scale = dpr;
        self.sync_display();
        self.rewarm();
        self
    }

    /// The app's own scale on top of [`Self::scale`], multiplying into the
    /// same logical space — [`Ui::set_user_scale`], which every frame
    /// reads.
    pub fn user_scale(mut self, scale: UserScale) -> Self {
        self.ui.set_user_scale(scale);
        self.sync_display();
        self.rewarm();
        self
    }

    /// Monitor refresh, which repaint-wake coalescing reads.
    pub fn refresh_millihertz(mut self, mhz: u32) -> Self {
        self.display.refresh_millihertz = Some(mhz);
        self.sync_display();
        self.rewarm();
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
    /// Its `user_scale` lands on [`Ui::set_user_scale`], the one home
    /// a frame reads it from.
    pub fn set_display(&mut self, display: Display) -> &mut Self {
        self.display = display;
        self.ui.set_user_scale(display.user_scale);
        self.sync_display();
        self
    }

    /// Run one frame and report what it did. The record closure runs
    /// once, twice, or not at all — see the module doc.
    ///
    /// First runs, with the same closure, every frame that input fed
    /// before this call still owes — see rule 16. The report is the last
    /// frame's.
    pub fn frame(&mut self, record: impl FnMut(&mut Ui)) -> FrameReport {
        self.frame_app(&mut RecordApp::new(record))
    }

    /// Run frames of a whole [`App`] — `update` once, then `record` per
    /// pass — the way a host drives it, delivering held input first like
    /// [`Self::frame`].
    pub fn frame_app(&mut self, app: &mut impl App) -> FrameReport {
        self.deliver_held_input(app);
        self.drive(true, app)
    }

    /// Run the frames input fed before now still owes, each with `app`,
    /// as a host does on the `repaint_requested` they report.
    fn deliver_held_input(&mut self, app: &mut impl App) {
        while self.ui.input().has_held_input() {
            self.drive(true, app);
        }
    }

    /// Exactly one host frame, delivering nothing held — for a test that
    /// asserts on the frames input is spread over.
    pub fn step(&mut self, record: impl FnMut(&mut Ui)) -> FrameReport {
        self.drive(true, &mut RecordApp::new(record))
    }

    /// Run one frame and keep what each record pass returned, warmup
    /// excluded. See [`Passes`] for which pass sees what.
    pub fn frame_passes<R>(&mut self, mut record: impl FnMut(&mut Ui) -> R) -> Passes<R> {
        self.deliver_held_input(&mut RecordApp::new(|ui: &mut Ui| {
            record(ui);
        }));
        self.step_passes(record)
    }

    /// [`Self::frame_passes`] over exactly one host frame, delivering
    /// nothing held, like [`Self::step`].
    pub fn step_passes<R>(&mut self, mut record: impl FnMut(&mut Ui) -> R) -> Passes<R> {
        // A cold recorder runs the input-blind warmup pass first; it is
        // not one of the passes a caller asks about.
        let mut warmup = self.ui.frame_runtime().is_first_frame();
        let mut values = Vec::new();
        let report = self.step(|ui| {
            // `record` runs on *every* pass — it is the scene, and a pass
            // that skipped it would record an empty tree and wipe the
            // cascade the next frame reads.
            let value = record(ui);
            if warmup {
                warmup = false;
            } else {
                values.push(value);
            }
        });
        Passes::new(values, report)
    }

    /// The value from the **input-observing** pass — pass A, the one
    /// that sees one-frame edges (`clicked`, `drag.started()`). Panics
    /// if the frame ran no record pass at all; [`Self::frame_passes`]
    /// reports that instead.
    pub fn frame_value<R>(&mut self, record: impl FnMut(&mut Ui) -> R) -> R {
        self.frame_passes(record).into_a().expect(
            "the frame ran no record pass — FrameProcessing::PaintOnly. A paint-anim \
             wake was the frame's only cause (a focused TextEdit's caret blink is \
             enough). Feed an input, request a repaint, or use `frame_passes`.",
        )
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

    /// Move the absolute clock. Every event fed after this carries the
    /// new time, and so does the next frame — one clock, read at both
    /// doors, the way a host reads its own.
    ///
    /// Animation dt is derived rather than read: it is clamped per frame
    /// to `MAX_ANIM_DT`, so one big jump here does not integrate one big
    /// step there. Use [`Self::advance_frames`] for that.
    pub fn advance(&mut self, dt: Duration) -> &mut Self {
        self.time += dt;
        self
    }

    /// Park the absolute clock at `time` — [`Self::advance`] for a test
    /// written against absolute stamps rather than deltas, and the same
    /// clock on both doors.
    pub const fn at(&mut self, time: Duration) -> &mut Self {
        self.time = time;
        self
    }

    /// `n` frames stepping `dt` each — the correct way to move an
    /// animation, since a single large jump is clamped to `MAX_ANIM_DT`.
    /// A `dt` past that clamp panics: a test stepping it would
    /// under-integrate silently rather than fail.
    pub fn advance_frames(&mut self, n: u32, dt: Duration, mut record: impl FnMut(&mut Ui)) {
        Self::assert_anim_step(dt);
        for _ in 0..n {
            self.advance(dt);
            self.frame(&mut record);
        }
    }

    /// Frames stepping `dt` each until one requests no repaint, at most
    /// `max` of them: how many frames that took, the idle one included,
    /// or `None` if the last was still asking for another. The `dt`
    /// bound is [`Self::advance_frames`]'s.
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

    /// Move the clock past `DOUBLE_CLICK_WINDOW`, so the next click starts
    /// a fresh press run. Without this the clock never moves, every click
    /// is simultaneous, and a second `click_at` on the same spot always
    /// reports `double_clicked`.
    ///
    /// The peer of [`Self::advance`], and like it records no frame: a
    /// press is stamped when it arrives, so the gap separates the runs on
    /// its own. A frame here would have to record some tree, and one that
    /// is not the test's own sweeps the state rows the test is about.
    pub fn advance_past_double_click(&mut self) -> &mut Self {
        self.advance(DOUBLE_CLICK_WINDOW + Duration::from_millis(1))
    }

    /// The raw input door, for events with no typed helper above — a
    /// `KeyDown` with a specific `physical`, say. Everything the typed
    /// helpers cover should go through them: they are what enforce the
    /// press-origin, modifier, and threshold rules, and a helper that
    /// emits exactly one event hands back that event's [`InputDelta`] so
    /// nothing is given up by using it.
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

    /// Returns the move's [`InputDelta`] — `on_input`'s value, so a
    /// repaint-hint assertion has no reason to build the event by hand.
    pub fn move_to(&mut self, pos: Vec2) -> InputDelta {
        self.on_input(InputEvent::PointerMoved(pos))
    }

    /// The pointer leaves the surface. Returns the event's
    /// [`InputDelta`], like [`Self::move_to`].
    pub fn pointer_left(&mut self) -> InputDelta {
        self.on_input(InputEvent::PointerLeft)
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

    /// Release the left button, clearing the press origin
    /// [`Self::drag_to`] measures from.
    pub fn release(&mut self) -> InputDelta {
        self.release_button(PointerButton::Left)
    }

    /// [`Self::release`] with the button named.
    pub fn release_button(&mut self, button: PointerButton) -> InputDelta {
        self.on_input(InputEvent::PointerReleased(button))
    }

    /// Press and release the left button at `pos`, in one frame's
    /// worth of events.
    pub fn click_at(&mut self, pos: Vec2) {
        self.click_button_at(PointerButton::Left, pos);
    }

    /// [`Self::click_at`] with the right button.
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

    /// The screen position of `local`, a point in `id`'s arranged space
    /// measured from its layout origin — through every ancestor
    /// transform, the way a pointer has to reach it. **Checked** like
    /// [`Self::click_on`]: panics unless the pointer would reach `id`
    /// there.
    pub fn point_in(&self, id: WidgetId, local: Vec2) -> Vec2 {
        let response = self.ui.response_for(id);
        let layout = response.layout_rect.unwrap_or_else(|| {
            panic!("{id:?} has no arranged rect — it did not record, or nothing primed the frame")
        });
        let pos = response.transform.apply_point(layout.min + local);
        self.assert_reaches(id, pos);
        pos
    }

    /// `id`'s map from its arranged space to the screen: every ancestor's
    /// transform, scroll and zoom composed. Unchecked, unlike
    /// [`Self::point_in`] — for a point or a rect that need not land on
    /// `id`, such as a drag target or an edge carried off the surface.
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

    /// Click `id` at its center. See `Self::hit_center_of` for what is
    /// checked and why.
    pub fn click_on(&mut self, id: WidgetId) {
        self.click_at(self.hit_center_of(id));
    }

    /// Press `id` at its center, checked the same way
    /// [`Self::click_on`] is.
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

    /// Scroll and pinch carry no position of their own: `InputState`
    /// routes them to whatever the pointer was last over. These bare
    /// forms are for a gesture deliberately separated from the move that
    /// aimed it — the `_at` peers below are the same events with the aim
    /// inlined. Positive `y` means the content scrolls down.
    pub fn scroll_lines(&mut self, delta: Vec2) -> InputDelta {
        self.on_input(InputEvent::ScrollLines(delta))
    }

    /// [`Self::scroll_lines`] in pixels rather than lines.
    pub fn scroll_pixels(&mut self, delta: Vec2) -> InputDelta {
        self.on_input(InputEvent::ScrollPixels(delta))
    }

    /// A pinch-zoom step, aimed like [`Self::scroll_lines`].
    pub fn pinch(&mut self, factor: f32) -> InputDelta {
        self.on_input(InputEvent::Zoom(factor))
    }

    /// Aim, then scroll — the delta returned is the scroll's, not the
    /// positioning move's.
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

    /// A non-repeat press whose `physical` is [`Key::Other`]
    /// (`InputEvent::key_down`); a case that needs a real physical
    /// position builds the event through [`Self::on_input`]. Text rides
    /// along under any modifier: the command gate is the *field's*, not
    /// this one's — a platform reports text under Ctrl too.
    pub fn key(&mut self, key: Key) -> InputDelta {
        self.on_input(InputEvent::key_down(key))
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
    /// would have cleared it, leaving a chord silently held. It is read
    /// *after held input*: a change still waiting in the queue (rule 16)
    /// is one this harness already sent.
    pub fn set_modifiers(&mut self, mods: Modifiers) {
        if self.ui.input().modifiers_after_held_input() != mods {
            self.on_input(InputEvent::ModifiersChanged(mods));
        }
    }

    /// One press per character, each carrying that character as its
    /// text — the path a real window produces for someone typing.
    pub fn type_text(&mut self, s: &str) {
        for c in s.chars() {
            self.key(Key::Char(c));
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

    /// [`Self::layout_rect`] for a widget the test knows arranged —
    /// the read a layout assertion makes. Panics when `id` did not
    /// arrange last frame.
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

    /// Topmost widget the pointer would hit at `pos`, by the same filter
    /// hover routing uses. Turns "the press didn't land and I don't know
    /// why" into one assertion.
    pub fn hit_at(&self, pos: Vec2) -> Option<WidgetId> {
        self.ui.cascade().hit_test(pos, Sense::hovers)
    }

    /// The harness clipboard's text.
    ///
    /// # Panics
    ///
    /// Panics when no clipboard backend answers. The harness runs on the
    /// in-memory backend, which always does.
    pub fn clipboard_text(&self) -> String {
        self.ui
            .clipboard()
            .text()
            .expect("the harness clipboard is the in-memory backend")
    }

    /// Seed the harness clipboard, for a paste a test is about to make.
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
    /// Panics when no such row exists, so a wrong id or a wrong type fails
    /// instead of reading a default.
    pub fn state<S: 'static>(&self, id: WidgetId) -> &S {
        self.ui.state::<S>(id).unwrap_or_else(|| {
            panic!(
                "no `{}` row for {id:?} — wrong id, wrong type, or it never recorded",
                any::type_name::<S>(),
            )
        })
    }

    /// Escape hatch. Reading `response_for` off this between frames sees
    /// the previous frame's input — prefer [`Self::response_in`].
    pub const fn ui(&mut self) -> &mut Ui {
        &mut self.ui
    }
}

/// Tier 2 — construction and the frame entry tier 1 builds on.
impl UiHarness {
    /// A recorder over `resources` — two over one, for the shared-text-cache
    /// and idle/active-window tests, or one over a shaper of the caller's
    /// own, for a case that *changes* the font database.
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

    /// The one place a frame is actually entered. `Ui::frame` is
    /// `pub(crate)`, so the harness drives it directly rather than
    /// through a test method on `Ui`.
    fn drive(&mut self, damage_baseline_valid: bool, app: &mut impl App) -> FrameReport {
        let stamp = FrameStamp::new(self.frame_display(), self.time);
        self.ui.frame(
            &mut self.engines,
            FrameInput::new(stamp, damage_baseline_valid),
            WindowToken(0),
            app,
        )
    }

    /// The display a frame runs at: the harness's surface and scale under
    /// the app's [`Ui::user_scale`], derived as the window driver derives
    /// it, so a setting changed inside a frame lands on the next.
    fn frame_display(&self) -> Display {
        Display {
            user_scale: self.ui.user_scale(),
            ..self.display
        }
    }

    fn sync_display(&mut self) {
        self.ui.set_display(self.frame_display());
    }

    /// Re-seed `prev_stamp` after a builder moved the display, so the
    /// move does not read as a display change on frame 1 — but only on a
    /// warm harness: seeding a cold one would skip the warmup pass it
    /// exists for.
    fn rewarm(&mut self) {
        if !self.ui.frame_runtime().is_first_frame() {
            self.mark_warm();
        }
    }

    /// Seed `prev_stamp` so frame 1 skips the cold-start warmup pass and
    /// runs one record pass like every later frame.
    fn mark_warm(&mut self) {
        self.ui
            .set_prev_stamp(Some(FrameStamp::new(self.frame_display(), self.time)));
    }
}

/// The damage reads the crate's damage tests and the `damage` bench make;
/// a non-test `internals` build has no caller.
#[cfg(any(test, feature = "bench"))]
pub(crate) mod internals {
    use crate::damage::region::{CollapsedDamage, DEFAULT_PASS_BUDGET_PX, DamageRegion};
    use crate::internals::harness::UiHarness;

    impl UiHarness {
        /// Collapse this frame's accumulated raw rects the way
        /// `DamageEngine::finish_region` does — the rects *and* the coverage
        /// they cover the surface with.
        pub(crate) fn collapsed_damage(&self) -> CollapsedDamage {
            DamageRegion::collapse_from(
                &self.engines.damage.raw_rects,
                DEFAULT_PASS_BUDGET_PX,
                self.ui.display().logical_rect(),
            )
        }

        /// Just the rects — what most damage assertions are about.
        pub(crate) fn damage_region(&self) -> DamageRegion {
            self.collapsed_damage().region
        }
    }
}

/// Tier 3 — everything only the *in-tree* suite calls. One gated `mod`
/// rather than attributes on single methods, so its imports live with it.
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
        /// Cold recorder — `prev_stamp` unseeded, so frame 1 runs the
        /// extra blackout warmup pass. For the tests that pin cold start
        /// itself; every other constructor is warm.
        pub(crate) fn cold(surface: UVec2) -> Self {
            let mut harness = Self::new(surface);
            harness.ui.set_prev_stamp(None);
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
            self.drive(false, &mut RecordApp::new(record))
        }

        /// A `FILL`/`FILL` hstack wrapped around `f`, returning the node
        /// `f` produced — the fixture for "arrange this one subtree
        /// against the whole surface".
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

        /// `node`'s main-tree intrinsic on `axis` — the query a layout test
        /// makes of the frame it just ran.
        pub(crate) fn intrinsic(&mut self, node: NodeId, axis: Axis, req: LenReq) -> f32 {
            self.engines
                .layout
                .main_intrinsic(self.ui.forest(), node, axis, req)
        }

        /// Where `id` recorded last frame, on whichever layer took it.
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
