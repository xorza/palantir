pub(crate) mod cascade;
pub(crate) mod damage;
pub(crate) mod frame_state;
pub(crate) mod state;

use crate::animation::animatable::Animatable;
use crate::animation::{AnimMap, AnimSlot, AnimSpec};
use crate::forest::Forest;
use crate::forest::element::Element;
use crate::forest::tree::{Layer, NodeId};
use crate::forest::widget_id::WidgetId;
use crate::input::{FocusPolicy, InputEvent, InputState, ResponseState};
use crate::layout::Layout;
use crate::layout::layoutengine::LayoutEngine;
use crate::layout::types::display::Display;
use crate::primitives::approx::EPS;
use crate::primitives::color::Color;
use crate::primitives::mesh::Mesh;
use crate::renderer::frontend::FrameReport;
use crate::shape::Shape;
use crate::text::TextShaper;
use crate::ui::cascade::CascadesEngine;
use crate::ui::damage::{Damage, DamageEngine};
use crate::ui::frame_state::FrameState;
use crate::ui::state::StateMap;
use crate::widgets::theme::Theme;
use std::time::Duration;

/// Recorder + input/response broker. All public coordinates are
/// logical pixels (DIPs); `Display::scale_factor` converts to
/// physical at the wgpu boundary. See `docs/repaint.md` for the
/// frame-lifecycle rationale.
pub struct Ui {
    pub(crate) forest: Forest,
    pub theme: Theme,
    /// Cross-frame `WidgetId → Any` widget state.
    pub(crate) state: StateMap,
    /// Shared font/glyph shaper. Set at construction via
    /// [`Self::with_text`] (or left at the mono-fallback default by
    /// [`Self::new`]). Read-only after construction — apps should
    /// share the same handle with the wgpu backend so both see one
    /// buffer cache.
    pub(crate) text: TextShaper,
    pub(crate) layout_engine: LayoutEngine,
    pub(crate) layout: Layout,
    pub(crate) input: InputState,
    pub(crate) cascades_engine: CascadesEngine,
    pub(crate) display: Display,
    pub(crate) damage_engine: DamageEngine,
    /// `now - prev_now` clamped to [`Self::MAX_DT`].
    pub(crate) dt: f32,
    /// Bumped once per [`Self::run_frame`], before either pass —
    /// pinned by `run_frame_pass_count_matches_action_trigger`.
    pub(crate) frame_id: u64,
    /// Host-supplied monotonic timestamp for this frame.
    pub(crate) time: Duration,
    /// Set by [`Self::animate`] when an animation hasn't settled.
    pub(crate) repaint_requested: bool,
    pub(crate) anim: AnimMap,
    /// Submission status of the last *painted* frame. NOT reset in
    /// `pre_record` — `click_on_empty_bg_does_not_force_full`
    /// pins why.
    pub(crate) frame_state: FrameState,
    /// Set by [`Self::request_relayout`]; consumed by
    /// `post_record` to trigger one re-record per
    /// `run_frame`.
    pub(crate) relayout_requested: bool,
}

impl Default for Ui {
    fn default() -> Self {
        Self::new()
    }
}

impl Ui {
    /// Per-frame `dt` clamp (seconds). Stalled frames freeze
    /// animation tickers instead of teleporting; [`Self::time`]
    /// still tracks the host's true clock.
    pub(crate) const MAX_DT: f32 = 0.1;

    /// Construct with the mono-fallback shaper. Use for headless /
    /// test / preview contexts where glyph cache identity doesn't
    /// matter; production apps use [`crate::Host`], which builds a
    /// `Ui` via [`Self::with_text`] under the hood and shares the
    /// shaper with the backend.
    pub fn new() -> Self {
        Self::with_text(TextShaper::default())
    }

    /// Construct with an explicit shaper. The same handle must reach
    /// the wgpu backend (via [`crate::Host::new`]) so layout-time
    /// measurement and render-time shaping hit one buffer cache.
    pub fn with_text(text: TextShaper) -> Self {
        Self {
            forest: Forest::default(),
            theme: Theme::default(),
            state: StateMap::default(),
            text,
            layout_engine: LayoutEngine::default(),
            layout: Layout::default(),
            input: InputState::new(),
            cascades_engine: CascadesEngine::default(),
            display: Display::default(),
            damage_engine: DamageEngine::default(),
            dt: 0.0,
            frame_id: 0,
            time: Duration::ZERO,
            anim: AnimMap::default(),
            frame_state: FrameState::default(),
            relayout_requested: false,
            repaint_requested: false,
        }
    }

    // ── Frame lifecycle ───────────────────────────────────────────────

    /// The only public entry point for driving a frame. Runs `record`
    /// once, re-records on action input or `request_relayout`, paints
    /// the last pass. `now` is monotonic host time;
    /// `Ui::{dt,time,frame_id}` derive from it. See `docs/repaint.md`.
    pub fn frame(
        &mut self,
        display: Display,
        now: Duration,
        mut record: impl FnMut(&mut Ui),
    ) -> FrameReport {
        assert!(
            display.scale_factor >= EPS,
            "Display::scale_factor must be ≥ EPSILON; got {}",
            display.scale_factor,
        );

        let raw_dt = now.saturating_sub(self.time);
        self.dt = raw_dt.as_secs_f32().min(Self::MAX_DT);
        self.time = now;
        self.frame_id += 1;
        self.repaint_requested = false;
        self.relayout_requested = false;

        if self.should_invalidate_prev(display) {
            self.damage_engine.invalidate_prev();
        }
        self.display = display;
        // Pending until the renderer (`Host::render`) confirms a
        // successful submit. Tests driving `Ui::frame` directly must
        // ack via `ui.frame_state.mark_submitted()` or the next
        // frame's `should_invalidate_prev` will force a `Full`.
        self.frame_state.mark_pending();

        let action_flag = self.record_pass(&mut record);
        if action_flag || self.relayout_requested {
            // Pass B paints, regardless of any further re-record
            // request — caps relayout at one retry per `run_frame`.
            self.input.drain_per_frame_queues();
            let _ = self.record_pass(&mut record);
        }
        let damage = self.finalize_frame();

        FrameReport {
            repaint_requested: self.repaint_requested,
            skip_render: damage.is_none(),
            damage,
        }
    }

    /// Should we discard the last painted frame's damage snapshot? True
    /// on first frame, on a surface-rect change, or when the host
    /// failed to confirm submission of the last frame.
    fn should_invalidate_prev(&self, new_display: Display) -> bool {
        let new_surface = new_display.logical_rect();
        let display_changed = self
            .damage_engine
            .prev_surface
            .is_some_and(|prev| prev != new_surface);
        let frame_skipped = !self.frame_state.was_last_submitted();
        let invalidate = display_changed || frame_skipped;
        if invalidate {
            tracing::debug!(
                display_changed,
                frame_skipped,
                first_frame = self.damage_engine.prev_surface.is_none(),
                "damage.invalidate_prev"
            );
        }
        invalidate
    }

    /// One `pre_record` → user record → drain action flag → `post_record`
    /// cycle. Returns whether the cycle saw action input (which triggers
    /// a second pass in `Ui::frame`).
    fn record_pass(&mut self, record: &mut impl FnMut(&mut Ui)) -> bool {
        self.pre_record();
        record(self);
        let action_flag = self.input.take_action_flag();
        self.post_record();
        action_flag
    }

    /// Feed a palantir-native input event. Hosts own redraw scheduling.
    pub fn on_input(&mut self, event: InputEvent) {
        self.input.on_input(event, &self.layout.cascades);
    }

    /// Re-record this frame after measure runs (for widgets that
    /// realize their record-time inputs were stale). Capped at one
    /// re-record per `run_frame`.
    pub fn request_relayout(&mut self) {
        self.relayout_requested = true;
    }

    fn pre_record(&mut self) {
        self.forest.pre_record();
    }

    /// Record-half of `frame`: finalize hashes, run measure / arrange.
    /// Stale cache entries (for widgets recorded last frame but
    /// absent this pass) are tolerated through `layout.run` — they
    /// can't match live keys — and reaped once in `finalize_frame`
    /// against the final pass's id set.
    fn post_record(&mut self) {
        self.forest.post_record();
        self.layout_engine.run(
            &self.forest,
            self.display.logical_rect(),
            &self.text,
            &mut self.layout,
        );
    }

    /// Paint-half of `frame`: diff seen ids against the last painted
    /// frame, fan the `removed` set out to per-widget caches, cascade
    /// → hit-index → damage. Reads the `Layout` from the most recent
    /// `post_record`. Sweep runs here (once per `frame`) rather than
    /// per `post_record` so a widget that vanishes in pass A but
    /// returns in pass B keeps its state across the discard.
    fn finalize_frame(&mut self) -> Option<Damage> {
        let removed = self.forest.ids.rollover();
        self.text.sweep_removed(removed);
        self.layout_engine.sweep_removed(removed);
        self.state.sweep_removed(removed);
        self.anim.sweep_removed(removed);

        self.cascades_engine.run(&self.forest, &mut self.layout);
        self.input.post_record(&self.layout.cascades);
        self.damage_engine.compute(
            &self.forest,
            &self.layout.cascades,
            &self.forest.ids.removed,
            self.display.logical_rect(),
        )
    }

    // ── Recording (widget-facing) ─────────────────────────────────────

    pub fn add_shape(&mut self, shape: Shape<'_>) {
        if shape.is_noop() {
            return;
        }
        if let Shape::Polyline { points, colors, .. } = &shape {
            colors.assert_matches(points.len());
        }
        self.forest.add_shape(shape);
    }

    /// Convenience wrapper for the common "draw this mesh at the
    /// owner's full rect, tint white" case.
    #[inline]
    pub fn add_mesh(&mut self, mesh: &Mesh) {
        self.add_shape(Shape::Mesh {
            mesh,
            local_rect: None,
            tint: Color::WHITE.into(),
        });
    }

    /// Record `body` as a side layer placed at `anchor` (top-left
    /// position). `size = None` makes the body's "available" extend
    /// from `anchor` to the surface bottom-right; `size = Some(s)`
    /// caps it at `s`, still clamped to the surface so an oversized
    /// cap can't bleed past the viewport. The root's own `Sizing`
    /// (Hug/Fill/Fixed) then governs the painted size within that
    /// available. Must be called at top-level (no node open) —
    /// egui-style: finish the `Main` scope first, then layer.
    pub fn layer<R>(
        &mut self,
        layer: Layer,
        anchor: glam::Vec2,
        size: Option<crate::primitives::size::Size>,
        body: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        self.forest.push_layer(layer, anchor, size);
        let result = body(self);
        self.forest.pop_layer();
        result
    }

    pub(crate) fn node(&mut self, element: Element, f: impl FnOnce(&mut Ui)) -> NodeId {
        // Id collision detection + auto-id disambiguation happen
        // inside `Forest::open_node`, so any path that opens a node
        // (including direct `self.forest.open_node` callers) gets the
        // same check. Explicit-id collisions hard-assert, auto-id
        // collisions get silently disambiguated.
        let node = self.forest.open_node(element);
        f(self);
        self.forest.close_node();
        node
    }

    pub(crate) fn response_for(&self, id: WidgetId) -> ResponseState {
        let mut state = self.input.response_for(id, &self.layout.cascades);
        // Cascade lags one frame; OR this frame's ancestor-disabled so
        // a freshly-disabled subtree paints disabled on its first frame.
        state.disabled |= self.forest.trees[self.forest.current_layer as usize].ancestor_disabled();
        state
    }

    // ── Cross-frame state & animation ─────────────────────────────────

    /// Cross-frame state row for `id`, `T::default()` on first
    /// access. Rows for `WidgetId`s not recorded this frame are
    /// evicted in `post_record`. Panics on type collision at `id`.
    pub fn state_mut<T: Default + 'static>(&mut self, id: WidgetId) -> &mut T {
        self.state.get_or_insert_with(id, T::default)
    }

    /// Advance an animation row keyed by `(id, slot)` and return the
    /// current value. `spec = None` snaps to `target` and drops any
    /// stale row without requesting a repaint — the canonical
    /// "no animation" path. See `src/animation/animations.md`.
    pub fn animate<T: Animatable>(
        &mut self,
        id: WidgetId,
        slot: impl Into<AnimSlot>,
        target: T,
        spec: Option<AnimSpec>,
    ) -> T {
        let slot = slot.into();
        // Merge `None` and instant-degenerate specs (`Duration { secs ≈ 0 }`)
        // into one snap path. `tick` then handles only real motion.
        let Some(spec) = spec.filter(|s| !s.is_instant()) else {
            // Drop stale row so a future `Some(_)` starts fresh from
            // `target`. `try_typed_mut` avoids allocating a typed map
            // just to remove from one that may not exist.
            if let Some(typed) = self.anim.try_typed_mut::<T>() {
                typed.rows.remove(&(id, slot));
            }
            return target;
        };
        let r = self
            .anim
            .typed_mut::<T>()
            .tick(id, slot, target, spec, self.dt, self.frame_id);
        if !r.settled {
            self.repaint_requested = true;
        }
        r.current
    }

    // ── Focus ─────────────────────────────────────────────────────────

    /// Currently focused widget id, or `None`.
    pub fn focused_id(&self) -> Option<WidgetId> {
        self.input.focused
    }

    /// Programmatically set or clear focus. Bypasses [`FocusPolicy`].
    pub fn request_focus(&mut self, id: Option<WidgetId>) {
        self.input.focused = id;
    }

    pub fn focus_policy(&self) -> FocusPolicy {
        self.input.focus_policy
    }

    /// Set the press-on-non-focusable behavior. See [`FocusPolicy`].
    pub fn set_focus_policy(&mut self, p: FocusPolicy) {
        self.input.focus_policy = p;
    }
}

#[cfg(test)]
mod tests;
