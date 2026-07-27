//! Frame-driving test harness — the intended single entry point for
//! driving a [`Ui`] with synthetic input, in-crate and (once exported)
//! from consumers.
//!
//! The design and the protocol rules it enforces are in
//! `docs/roadmap/test-harness.md`. In short: `Ui::frame` calls the
//! record closure one, two, or three times per frame and each call sees
//! different input, and almost every way to get a driven test wrong is a
//! corollary of that. This type holds the surface, scale, clock,
//! modifier state, and press origin so those rules can be enforced
//! rather than documented.
//!
//! **Three tiers, one per block.** The whole module is already
//! `#[cfg(any(test, feature = "internals"))]`; the tiers say *who inside
//! that* a given method is for.
//!
//! 1. `impl UiHarness` (`pub`) — the surface that leaves the crate
//!    through `palantir::internals`, addressing widgets by [`WidgetId`]
//!    and nothing else.
//! 2. `impl UiHarness` (`pub(crate)`) — the schedule and damage knobs the
//!    **benches** need. They compile under `internals` without
//!    `cfg(test)`, which is why this tier cannot be narrowed further.
//! 3. `mod unit` (`#[cfg(test)]`) — what only the *in-tree* suite calls:
//!    the tree/encoder reach-ins, the cold constructor, the
//!    no-baseline frame drivers.
//!
//! Tier 3's extra gate is not about encapsulation — `pub(crate)` already
//! stops all of this leaving the crate. It is the only way to say "the
//! benches don't use this", and it is what lets the module carry no
//! `dead_code` allow: under `--features internals` alone, tier 3 simply
//! isn't compiled, so nothing is spuriously unused and a genuinely dead
//! method still gets reported.
//!
//! Tier 1 carries the only lint allows, and they are about build shape
//! rather than design: `palantir::internals` is itself
//! `#[cfg(feature = "internals")]`, so a default-feature build does not
//! compile the door tier 1 leaves by, and every method on it reads as
//! both unreachable and dead. Scoping the allows to that block keeps
//! tiers 2 and 3 honest — a genuinely unused method there still fails
//! the build.

use crate::app::internals::RecordApp;
use crate::common::time::MAX_ANIM_DT;
use crate::display::Display;
use crate::host::shared::HostShared;
use crate::input::InputEvent;
use crate::input::keyboard::{Key, Modifiers, TextChunk};
use crate::input::pointer::PointerButton;
use crate::input::response::{InputDelta, ResponseState};
use crate::input::sense::{DOUBLE_CLICK_WINDOW, DRAG_THRESHOLD, Sense};
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::scene::damage::region::DamageRegion;
use crate::scene::layer::Layer;
use crate::scene::seen_ids::Endpoint;
use crate::text::TextShaper;
use crate::ui::Ui;
use crate::ui::frame::{FrameInput, FrameStamp};
use crate::ui::frame_report::FrameReport;
use crate::ui::resources::UiResources;
use crate::window::WindowToken;
use glam::{UVec2, Vec2};
use std::time::Duration;

/// Surface for [`UiHarness::arena`]. Never framed, so the value only has
/// to be non-degenerate.
const ARENA_SURFACE: UVec2 = UVec2::splat(1);

// Same reachability story as tier 1 below: the type is `pub` because it
// is re-exported from `palantir::internals`, which a default-feature
// build does not compile.
#[allow(unreachable_pub)]
#[derive(Debug)]
pub struct UiHarness {
    /// `pub(crate)` rather than behind an accessor so in-crate tests
    /// reach the engines they assert on (`h.ui.damage_engine`,
    /// `h.ui.cascades`) the way they reach them off a bare `Ui` today.
    /// Consumers cannot see the field and go through [`Self::ui`].
    pub(crate) ui: Ui,
    /// Physical pixels. Pointer positions are logical — see [`Self::scale`].
    surface: UVec2,
    scale: f32,
    pixel_snap: bool,
    refresh_millihertz: Option<u32>,
    /// Absolute; each frame stamps with it. Only moves on `advance`.
    time: Duration,
    /// Mirrors what the `Ui` was last told, so `ModifiersChanged` is
    /// emitted on change and `key_mods` can restore.
    mods: Modifiers,
    /// Press origin, solely so `drag_to` can check `DRAG_THRESHOLD`.
    pressed_at: Option<Vec2>,
}

/// Tier 1 — the surface that leaves the crate. See the module doc for
/// why this block, and only this block, silences the two reachability
/// lints.
#[allow(dead_code, unreachable_pub)]
impl UiHarness {
    /// [`UiResources::isolated_mono`] — mono-fallback text: fast,
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
        let shared = HostShared::new(SHARED.with(Clone::clone), None);
        Self::from_resources(shared.resources.clone(), surface)
    }

    /// A harness that is never framed — its [`Self::ui`] is a
    /// string-interning arena for tests that build `InternedStr`-bearing
    /// projections without recording. Exists because `InternedStr` is
    /// public and `Ui::intern` is the only public way to mint one.
    pub fn arena() -> Self {
        Self::new(ARENA_SURFACE)
    }

    /// Device pixel ratio. `surface` stays physical, so at `dpr = 2.0` a
    /// 600×200 surface is 300×100 logical — and every position below is
    /// logical.
    pub fn scale(mut self, dpr: f32) -> Self {
        self.scale = dpr;
        self.sync_display();
        self.mark_warm();
        self
    }

    /// Monitor refresh, which repaint-wake coalescing reads.
    pub fn refresh_millihertz(mut self, mhz: u32) -> Self {
        self.refresh_millihertz = Some(mhz);
        self.sync_display();
        self.mark_warm();
        self
    }

    pub fn pixel_snap(mut self, on: bool) -> Self {
        self.pixel_snap = on;
        self.sync_display();
        self.mark_warm();
        self
    }

    /// Change the surface between frames — the resize path. Deliberately
    /// not a builder and deliberately does not re-warm: the next frame
    /// must read this as `display_changed`, exactly as a real resize does.
    pub fn resize(&mut self, surface: UVec2) {
        self.surface = surface;
        self.sync_display();
    }

    pub fn frame(&mut self, record: impl FnMut(&mut Ui)) -> FrameReport {
        let (display, time) = (self.display(), self.time);
        self.drive(display, time, true, record)
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

    /// Frames until every arranged rect matches the previous frame's, up
    /// to `max`. For content whose size is only known after arrange
    /// (scroll thumbs, container text), where a fixed `2` is a guess.
    /// Panics if it never converges — so an animated UI, which never
    /// will, must use [`Self::prime`].
    pub fn prime_stable(&mut self, max: u32, mut record: impl FnMut(&mut Ui)) {
        assert!(max > 0, "prime_stable needs at least one frame");
        let mut prev: Vec<Rect> = Vec::new();
        for i in 0..max {
            self.frame(&mut record);
            let now = &self.ui.layout[Layer::Main].rect;
            if i > 0 && prev.as_slice() == now.as_slice() {
                return;
            }
            prev.clear();
            prev.extend_from_slice(now);
        }
        panic!(
            "layout did not converge within {max} frames — the arranged rects still \
             changed on the last one. An animated subtree never converges; use `prime`."
        );
    }

    /// Move the absolute clock. Takes effect on the **next** frame, and
    /// only then do subsequent input events carry it: `Ui::frame` is what
    /// publishes the clock to the input machine, so the order is
    /// `advance` → `frame` → input.
    ///
    /// Animation dt is a separate clock — it is clamped per frame to
    /// `MAX_ANIM_DT`, so one big jump here does not integrate one big
    /// step there. Use [`Self::advance_frames`] for that.
    pub fn advance(&mut self, dt: Duration) {
        self.time += dt;
    }

    /// `n` frames stepping `dt` each — the correct way to move an
    /// animation, since a single large jump is clamped to `MAX_ANIM_DT`.
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
    pub fn advance_past_double_click(&mut self, record: impl FnMut(&mut Ui)) {
        self.advance(DOUBLE_CLICK_WINDOW + Duration::from_millis(1));
        self.frame(record);
    }

    /// The raw input door, for events with no typed helper above —
    /// `Zoom`, a `KeyDown` with a specific `physical`, an
    /// `InputEvent::Text` you built yourself — and for the tests that
    /// assert on the returned [`InputDelta`]. Everything the typed
    /// helpers cover should go through them: they are what enforce the
    /// press-origin, modifier, and threshold rules.
    pub fn on_input(&mut self, event: InputEvent) -> InputDelta {
        self.ui.on_input(event)
    }

    pub fn move_to(&mut self, pos: Vec2) {
        self.ui.on_input(InputEvent::PointerMoved(pos));
    }

    pub fn pointer_left(&mut self) {
        self.ui.on_input(InputEvent::PointerLeft);
    }

    pub fn press_at(&mut self, pos: Vec2) {
        self.press_button_at(PointerButton::Left, pos);
    }

    pub fn press_button_at(&mut self, button: PointerButton, pos: Vec2) {
        self.move_to(pos);
        self.pressed_at = Some(pos);
        self.ui.on_input(InputEvent::PointerPressed(button));
    }

    pub fn release(&mut self) {
        self.release_button(PointerButton::Left);
    }

    pub fn release_button(&mut self, button: PointerButton) {
        self.pressed_at = None;
        self.ui.on_input(InputEvent::PointerReleased(button));
    }

    pub fn click_at(&mut self, pos: Vec2) {
        self.press_at(pos);
        self.release();
    }

    pub fn right_click_at(&mut self, pos: Vec2) {
        self.press_button_at(PointerButton::Right, pos);
        self.release_button(PointerButton::Right);
    }

    /// Two clicks at one point with the clock still — which is what puts
    /// them inside `DOUBLE_CLICK_WINDOW` and `DOUBLE_CLICK_RADIUS`.
    pub fn double_click_at(&mut self, pos: Vec2) {
        self.click_at(pos);
        self.click_at(pos);
    }

    /// Move while pressed. Panics if travel since the press has not
    /// crossed `DRAG_THRESHOLD` — the capture would not latch and the
    /// test would pass or fail for the wrong reason.
    pub fn drag_to(&mut self, pos: Vec2) {
        let origin = self
            .pressed_at
            .expect("drag_to needs a press first — no button is down");
        let travel = origin.distance(pos);
        assert!(
            travel >= DRAG_THRESHOLD,
            "drag_to({pos:?}) travels {travel} px from the press at {origin:?}, under \
             the {DRAG_THRESHOLD} px DRAG_THRESHOLD — no drag would latch",
        );
        self.move_to(pos);
    }

    /// Scroll and pinch carry no position of their own: the target is
    /// whatever the pointer was last over, so these move it first.
    /// Positive `y` means the content scrolls down.
    pub fn scroll_lines_at(&mut self, pos: Vec2, delta: Vec2) {
        self.move_to(pos);
        self.ui.on_input(InputEvent::ScrollLines(delta));
    }

    pub fn scroll_pixels_at(&mut self, pos: Vec2, delta: Vec2) {
        self.move_to(pos);
        self.ui.on_input(InputEvent::ScrollPixels(delta));
    }

    pub fn pinch_at(&mut self, pos: Vec2, factor: f32) {
        self.move_to(pos);
        self.ui.on_input(InputEvent::Zoom(factor));
    }

    pub fn key(&mut self, key: Key) {
        self.ui.on_input(InputEvent::KeyDown {
            key,
            repeat: false,
            physical: Key::Other,
        });
    }

    /// Set modifiers, emit the key, restore. Modifiers are sticky state,
    /// so without the restore every later key inherits them.
    pub fn key_mods(&mut self, key: Key, mods: Modifiers) {
        let saved = self.mods;
        self.set_modifiers(mods);
        self.key(key);
        self.set_modifiers(saved);
    }

    /// Emits `ModifiersChanged` only when the set actually changes —
    /// `Modifiers` is a snapshot the input machine holds, not a per-event
    /// flag.
    pub fn set_modifiers(&mut self, mods: Modifiers) {
        if self.mods != mods {
            self.mods = mods;
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

    /// Arranged rect from the previous frame's cascade. Safe to read
    /// between frames — geometry is stable across them, one-frame edges
    /// are not.
    pub fn rect(&self, id: WidgetId) -> Option<Rect> {
        self.ui.response_for(id).rect
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
        self.ui.cascades.hit_test(pos, Sense::hovers)
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

    pub fn assert_no_collisions(&self) {
        let collisions = self.collisions();
        assert!(
            collisions.is_empty(),
            "{} explicit widget-id collision(s) last frame: {collisions:?}",
            collisions.len(),
        );
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
            ui: Ui::new(resources),
            surface,
            scale: 1.0,
            pixel_snap: true,
            refresh_millihertz: None,
            time: Duration::ZERO,
            mods: Modifiers::NONE,
            pressed_at: None,
        };
        harness.sync_display();
        harness.mark_warm();
        harness
    }

    /// Explicit schedule *and* no damage baseline — the combination the
    /// caret-blink and multi-click tests need, since they drive a real
    /// clock and want every frame to repaint in full.
    pub(crate) fn frame_at_without_baseline(
        &mut self,
        display: Display,
        time: Duration,
        record: impl FnMut(&mut Ui),
    ) -> FrameReport {
        self.drive(display, time, false, record)
    }

    /// Explicit display and time, bypassing `surface` / `scale` / the
    /// clock — for callers driving their own schedule.
    pub(crate) fn frame_at(
        &mut self,
        display: Display,
        time: Duration,
        record: impl FnMut(&mut Ui),
    ) -> FrameReport {
        self.drive(display, time, true, record)
    }

    /// The one place a frame is actually entered. `Ui::frame` is
    /// `pub(crate)`, so the harness drives it directly rather than
    /// through a test method on `Ui`.
    fn drive(
        &mut self,
        display: Display,
        time: Duration,
        damage_baseline_valid: bool,
        record: impl FnMut(&mut Ui),
    ) -> FrameReport {
        let mut app = RecordApp::new(record);
        self.ui.frame(
            FrameInput {
                stamp: FrameStamp::new(display, time),
                damage_baseline_valid,
            },
            WindowToken(0),
            &mut app,
        )
    }

    /// The `Display` this harness frames at.
    pub(crate) fn display(&self) -> Display {
        Display {
            pixel_snap: self.pixel_snap,
            refresh_millihertz: self.refresh_millihertz,
            ..Display::from_physical(self.surface, self.scale)
        }
    }

    fn sync_display(&mut self) {
        self.ui.display = self.display();
    }

    /// Seed `prev_stamp` so frame 1 skips the cold-start warmup pass and
    /// runs one record pass like every later frame.
    fn mark_warm(&mut self) {
        self.ui.frame_runtime.prev_stamp = Some(FrameStamp::new(self.display(), self.time));
    }

    pub(crate) fn damage_region(&self) -> DamageRegion {
        DamageRegion::collapse_from(
            &self.ui.damage_engine.raw_rects,
            self.ui.damage_engine.budget_px,
            self.ui.display.logical_rect(),
        )
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
    use crate::renderer::frontend::encoder;
    use crate::renderer::frontend::record_sink::RecordedPaint;
    use crate::renderer::gradient_atlas::handle::SharedGradientAtlas;
    use crate::renderer::plan::{RenderKind, RenderPlan};
    use crate::scene::damage::region::DamageRegion;
    use crate::scene::layer::Layer;
    use crate::scene::node::Configure;
    use crate::scene::tree::node::NodeId;
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

        /// `damage_baseline_valid: false` — forces a full frame.
        pub(crate) fn frame_without_baseline(
            &mut self,
            record: impl FnMut(&mut Ui),
        ) -> FrameReport {
            let (display, time) = (self.display(), self.time);
            self.drive(display, time, false, record)
        }

        pub(crate) fn frame_value_without_baseline<R>(
            &mut self,
            mut record: impl FnMut(&mut Ui) -> R,
        ) -> R {
            let mut first = None;
            self.frame_without_baseline(|ui| {
                let value = record(ui);
                if first.is_none() {
                    first = Some(value);
                }
            });
            first.expect("the frame ran no record pass")
        }

        /// A `FILL`/`FILL` hstack wrapped around `f`, returning the node
        /// `f` produced — the fixture for "arrange this one subtree
        /// against the whole surface".
        pub(crate) fn under_outer<F: FnMut(&mut Ui) -> NodeId>(&mut self, mut f: F) -> NodeId {
            self.frame_value_without_baseline(|ui| {
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

        pub(crate) fn encode_paint(&self) -> RecordedPaint {
            self.encode(RenderKind::Full)
        }

        pub(crate) fn encode_paint_for(&self, region: DamageRegion) -> RecordedPaint {
            self.encode(RenderKind::Partial { region })
        }

        fn encode(&self, kind: RenderKind) -> RecordedPaint {
            let plan = RenderPlan {
                clear: self.ui.theme.window_clear,
                kind,
            };
            encoder::internals::encode(self.ui.frame_scene(), &SharedGradientAtlas::default(), plan)
        }
    }
}

#[cfg(test)]
mod tests;
