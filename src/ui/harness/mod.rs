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
//! **Two rungs.** The first `impl` block is the surface intended to
//! leave the crate through `palantir::internals`; it addresses widgets
//! by [`WidgetId`] and nothing else. The second reaches into the tree,
//! encoder, and damage engine, and must not leave. Both are `pub(crate)`
//! today because the type is not exported yet — at export the first
//! block becomes `pub` and the second does not.
//!
//! Nothing calls the harness from the library target (its callers are
//! the `cfg(test)` suite and, later, out-of-crate integration tests), so
//! the whole module would read as dead under `--features internals`
//! alone. The allow below is that, not an invitation to leave real dead
//! code here.
#![allow(dead_code)]

use crate::common::time::MAX_ANIM_DT;
use crate::display::Display;
use crate::host::shared::HostShared;
use crate::input::InputEvent;
use crate::input::keyboard::{Key, Modifiers, TextChunk};
use crate::input::pointer::PointerButton;
use crate::input::response::ResponseState;
use crate::input::sense::{DOUBLE_CLICK_WINDOW, DRAG_THRESHOLD, Sense};
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::scene::damage::region::DamageRegion;
use crate::scene::layer::Layer;
use crate::scene::seen_ids::Endpoint;
use crate::text::TextShaper;
use crate::ui::Ui;
use crate::ui::frame::FrameStamp;
use crate::ui::frame_report::FrameReport;
use crate::ui::resources::UiResources;
use glam::{UVec2, Vec2};
use std::time::Duration;

/// Surface for [`UiHarness::arena`]. Never framed, so the value only has
/// to be non-degenerate.
const ARENA_SURFACE: UVec2 = UVec2::splat(1);

#[derive(Debug)]
pub(crate) struct UiHarness {
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

/// The rung intended to leave the crate.
impl UiHarness {
    /// [`UiResources::isolated_mono`] — mono-fallback text: fast,
    /// deterministic, and wrong for width-follows-label assertions.
    pub(crate) fn new(surface: UVec2) -> Self {
        Self::from_resources(UiResources::isolated_mono(), surface)
    }

    /// Real cosmic shaping, over a thread-local shared shaper. Use when
    /// anything under test sizes to its text. Metrics are not identical
    /// across machines — the bundled faces are joined by platform fonts
    /// as fallback — so assert relations, not exact widths.
    pub(crate) fn with_text(surface: UVec2) -> Self {
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
    pub(crate) fn arena() -> Self {
        Self::new(ARENA_SURFACE)
    }

    /// Device pixel ratio. `surface` stays physical, so at `dpr = 2.0` a
    /// 600×200 surface is 300×100 logical — and every position below is
    /// logical.
    pub(crate) fn scale(mut self, dpr: f32) -> Self {
        self.scale = dpr;
        self.sync_display();
        self.mark_warm();
        self
    }

    /// Monitor refresh, which repaint-wake coalescing reads.
    pub(crate) fn refresh_millihertz(mut self, mhz: u32) -> Self {
        self.refresh_millihertz = Some(mhz);
        self.sync_display();
        self.mark_warm();
        self
    }

    pub(crate) fn pixel_snap(mut self, on: bool) -> Self {
        self.pixel_snap = on;
        self.sync_display();
        self.mark_warm();
        self
    }

    /// Change the surface between frames — the resize path. Deliberately
    /// not a builder and deliberately does not re-warm: the next frame
    /// must read this as `display_changed`, exactly as a real resize does.
    pub(crate) fn resize(&mut self, surface: UVec2) {
        self.surface = surface;
        self.sync_display();
    }

    pub(crate) fn frame(&mut self, record: impl FnMut(&mut Ui)) -> FrameReport {
        let (display, time) = (self.display(), self.time);
        self.ui.record_test_frame(display, time, record)
    }

    /// The value from the **input-observing** pass — pass A, the one
    /// that sees one-frame edges (`clicked`, `drag.started()`). Panics
    /// if the frame ran no record pass at all; see [`Self::try_frame_value`].
    pub(crate) fn frame_value<R>(&mut self, record: impl FnMut(&mut Ui) -> R) -> R {
        self.try_frame_value(record).expect(
            "the frame ran no record pass — FrameProcessing::PaintOnly. A paint-anim \
             wake was the frame's only cause (a focused TextEdit's caret blink is \
             enough). Feed an input, request a repaint, or use `try_frame_value`.",
        )
    }

    /// [`Self::frame_value`] without the `PaintOnly` panic, for callers
    /// deliberately driving paint-anim frames.
    pub(crate) fn try_frame_value<R>(&mut self, mut record: impl FnMut(&mut Ui) -> R) -> Option<R> {
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
    pub(crate) fn prime(&mut self, n: u32, mut record: impl FnMut(&mut Ui)) {
        for _ in 0..n {
            self.frame(&mut record);
        }
    }

    /// Frames until every arranged rect matches the previous frame's, up
    /// to `max`. For content whose size is only known after arrange
    /// (scroll thumbs, container text), where a fixed `2` is a guess.
    /// Panics if it never converges — so an animated UI, which never
    /// will, must use [`Self::prime`].
    pub(crate) fn prime_stable(&mut self, max: u32, mut record: impl FnMut(&mut Ui)) {
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
    pub(crate) fn advance(&mut self, dt: Duration) {
        self.time += dt;
    }

    /// `n` frames stepping `dt` each — the correct way to move an
    /// animation, since a single large jump is clamped to `MAX_ANIM_DT`.
    pub(crate) fn advance_frames(&mut self, n: u32, dt: Duration, mut record: impl FnMut(&mut Ui)) {
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
    pub(crate) fn advance_past_double_click(&mut self, record: impl FnMut(&mut Ui)) {
        self.advance(DOUBLE_CLICK_WINDOW + Duration::from_millis(1));
        self.frame(record);
    }

    pub(crate) fn move_to(&mut self, pos: Vec2) {
        self.ui.on_input(InputEvent::PointerMoved(pos));
    }

    pub(crate) fn pointer_left(&mut self) {
        self.ui.on_input(InputEvent::PointerLeft);
    }

    pub(crate) fn press_at(&mut self, pos: Vec2) {
        self.press_button_at(PointerButton::Left, pos);
    }

    pub(crate) fn press_button_at(&mut self, button: PointerButton, pos: Vec2) {
        self.move_to(pos);
        self.pressed_at = Some(pos);
        self.ui.on_input(InputEvent::PointerPressed(button));
    }

    pub(crate) fn release(&mut self) {
        self.release_button(PointerButton::Left);
    }

    pub(crate) fn release_button(&mut self, button: PointerButton) {
        self.pressed_at = None;
        self.ui.on_input(InputEvent::PointerReleased(button));
    }

    pub(crate) fn click_at(&mut self, pos: Vec2) {
        self.press_at(pos);
        self.release();
    }

    pub(crate) fn right_click_at(&mut self, pos: Vec2) {
        self.press_button_at(PointerButton::Right, pos);
        self.release_button(PointerButton::Right);
    }

    /// Two clicks at one point with the clock still — which is what puts
    /// them inside `DOUBLE_CLICK_WINDOW` and `DOUBLE_CLICK_RADIUS`.
    pub(crate) fn double_click_at(&mut self, pos: Vec2) {
        self.click_at(pos);
        self.click_at(pos);
    }

    /// Move while pressed. Panics if travel since the press has not
    /// crossed `DRAG_THRESHOLD` — the capture would not latch and the
    /// test would pass or fail for the wrong reason.
    pub(crate) fn drag_to(&mut self, pos: Vec2) {
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
    pub(crate) fn scroll_lines_at(&mut self, pos: Vec2, delta: Vec2) {
        self.move_to(pos);
        self.ui.on_input(InputEvent::ScrollLines(delta));
    }

    pub(crate) fn scroll_pixels_at(&mut self, pos: Vec2, delta: Vec2) {
        self.move_to(pos);
        self.ui.on_input(InputEvent::ScrollPixels(delta));
    }

    pub(crate) fn pinch_at(&mut self, pos: Vec2, factor: f32) {
        self.move_to(pos);
        self.ui.on_input(InputEvent::Zoom(factor));
    }

    pub(crate) fn key(&mut self, key: Key) {
        self.ui.on_input(InputEvent::KeyDown {
            key,
            repeat: false,
            physical: Key::Other,
        });
    }

    /// Set modifiers, emit the key, restore. Modifiers are sticky state,
    /// so without the restore every later key inherits them.
    pub(crate) fn key_mods(&mut self, key: Key, mods: Modifiers) {
        let saved = self.mods;
        self.set_modifiers(mods);
        self.key(key);
        self.set_modifiers(saved);
    }

    /// Emits `ModifiersChanged` only when the set actually changes —
    /// `Modifiers` is a snapshot the input machine holds, not a per-event
    /// flag.
    pub(crate) fn set_modifiers(&mut self, mods: Modifiers) {
        if self.mods != mods {
            self.mods = mods;
            self.ui.on_input(InputEvent::ModifiersChanged(mods));
        }
    }

    /// One `KeyDown { key: Key::Char(c) }` per char — the path a real
    /// window produces. The winit host emits `InputEvent::Text` only from
    /// an IME commit; see [`Self::ime_commit`], and do not use both for
    /// the same text (`TextEdit` consumes each, so it would double-insert).
    pub(crate) fn type_text(&mut self, s: &str) {
        for c in s.chars() {
            self.key(Key::Char(c));
        }
    }

    /// The IME path: `InputEvent::Text`, split exactly as a commit is.
    pub(crate) fn ime_commit(&mut self, s: &str) {
        for chunk in TextChunk::split(s) {
            self.ui.on_input(InputEvent::Text(chunk));
        }
    }

    /// Arranged rect from the previous frame's cascade. Safe to read
    /// between frames — geometry is stable across them, one-frame edges
    /// are not.
    pub(crate) fn rect(&self, id: WidgetId) -> Option<Rect> {
        self.ui.response_for(id).rect
    }

    /// Center of `id`'s arranged rect.
    pub(crate) fn center_of(&self, id: WidgetId) -> Vec2 {
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
    pub(crate) fn response_in(
        &mut self,
        id: WidgetId,
        mut record: impl FnMut(&mut Ui),
    ) -> ResponseState {
        self.frame_value(move |ui| {
            record(ui);
            ui.response_for(id)
        })
    }

    /// Topmost widget the pointer would hit at `pos`, by the same filter
    /// hover routing uses. Turns "the press didn't land and I don't know
    /// why" into one assertion.
    pub(crate) fn hit_at(&self, pos: Vec2) -> Option<WidgetId> {
        self.ui.cascades.hit_test(pos, Sense::hovers)
    }

    /// Explicit-id collisions recorded last frame, as the colliding
    /// pairs. These otherwise surface only as a magenta runtime overlay,
    /// which no test can see.
    pub(crate) fn collisions(&self) -> Vec<(WidgetId, WidgetId)> {
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

    pub(crate) fn assert_no_collisions(&self) {
        let collisions = self.collisions();
        assert!(
            collisions.is_empty(),
            "{} explicit widget-id collision(s) last frame: {collisions:?}",
            collisions.len(),
        );
    }

    pub(crate) fn clipboard_text(&self) -> String {
        self.ui.resources.clipboard.get()
    }

    pub(crate) fn set_clipboard_text(&mut self, text: &str) {
        self.ui
            .resources
            .clipboard
            .set(text)
            .expect("the memory clipboard is always available");
    }

    /// Escape hatch. Reading `response_for` off this between frames sees
    /// the previous frame's input — prefer [`Self::response_in`].
    pub(crate) fn ui(&mut self) -> &mut Ui {
        &mut self.ui
    }
}

/// The in-crate rung: tree, encoder, damage, and the schedule knobs.
/// None of this leaves the crate when the type is exported.
impl UiHarness {
    /// Cold recorder — `prev_stamp` unseeded, so frame 1 runs the extra
    /// blackout warmup pass. For the tests that pin cold start itself;
    /// every other constructor is warm.
    pub(crate) fn cold(surface: UVec2) -> Self {
        let mut harness = Self::new(surface);
        harness.ui.frame_runtime.prev_stamp = None;
        harness
    }

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

    /// `damage_baseline_valid: false` — forces a full frame. A damage
    /// knob, so it stays in-crate.
    pub(crate) fn frame_without_baseline(&mut self, record: impl FnMut(&mut Ui)) -> FrameReport {
        let (display, time) = (self.display(), self.time);
        self.ui
            .record_test_frame_without_baseline(display, time, record)
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

    /// Explicit display and time, bypassing `surface` / `scale` / the
    /// clock — for callers driving their own schedule.
    pub(crate) fn frame_at(
        &mut self,
        display: Display,
        time: Duration,
        record: impl FnMut(&mut Ui),
    ) -> FrameReport {
        self.ui.record_test_frame(display, time, record)
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
        self.ui.damage_region()
    }
}

/// Tree and encoder reach-ins. Narrower than the block above because
/// their `Ui` counterparts live in `ui/internals.rs`'s `#[cfg(test)] mod
/// unit` and so do not exist under `--features internals` alone. They
/// widen to match the rest once those move onto this type for real.
/// Full paths rather than imports: these types are needed only by this
/// gated block, and a cfg'd `use` at the top of the file is what the
/// crate's style rules exist to prevent.
#[cfg(test)]
impl UiHarness {
    pub(crate) fn under_outer(
        &mut self,
        f: impl FnMut(&mut Ui) -> crate::scene::tree::node::NodeId,
    ) -> crate::scene::tree::node::NodeId {
        let surface = self.surface;
        self.ui.under_outer(surface, f)
    }

    pub(crate) fn main_child_ids(
        &self,
        parent: crate::scene::tree::node::NodeId,
    ) -> Vec<crate::scene::tree::node::NodeId> {
        self.ui.main_child_ids(parent)
    }

    pub(crate) fn main_child_rects(&self, parent: crate::scene::tree::node::NodeId) -> Vec<Rect> {
        self.ui.main_child_rects(parent)
    }

    pub(crate) fn node_for_widget_id(&self, id: WidgetId) -> crate::scene::tree::node::NodeId {
        self.ui.node_for_widget_id(id)
    }

    pub(crate) fn encode_paint(&self) -> crate::renderer::frontend::record_sink::RecordedPaint {
        self.ui.encode_paint()
    }

    pub(crate) fn encode_paint_for(
        &self,
        region: DamageRegion,
    ) -> crate::renderer::frontend::record_sink::RecordedPaint {
        self.ui.encode_paint_for(region)
    }

    pub(crate) fn anim_row_count<T: crate::animation::animatable::Animatable>(&mut self) -> usize {
        self.ui.anim_row_count::<T>()
    }
}

#[cfg(test)]
mod tests;
