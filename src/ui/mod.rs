pub(crate) mod cascade;
pub(crate) mod damage;
pub(crate) mod debug_overlay;
pub(crate) mod state;

use crate::animation::animatable::Animatable;
use crate::animation::{AnimMap, AnimSlot, AnimSpec};
use crate::forest::Forest;
use crate::forest::element::Element;
use crate::forest::tree::{Layer, NodeId};
use crate::forest::widget_id::WidgetId;
use crate::input::{FocusPolicy, InputEvent, InputState, ResponseState};
use crate::layout::LayoutEngine;
use crate::layout::types::display::Display;
use crate::primitives::color::Color;
use crate::primitives::mesh::Mesh;
use crate::renderer::frontend::{FrameOutput, FrameState, Frontend};
use crate::shape::Shape;
use crate::text::TextShaper;
use crate::ui::cascade::Cascades;
use crate::ui::damage::{Damage, DamagePaint};
use crate::ui::debug_overlay::DebugOverlayConfig;
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
    pub(crate) layout: LayoutEngine,
    pub(crate) frontend: Frontend,
    pub(crate) input: InputState,
    pub(crate) cascades: Cascades,
    pub(crate) display: Display,
    pub(crate) damage: Damage,
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
    pub debug_overlay: Option<DebugOverlayConfig>,
    /// Submission status of the last *painted* frame. NOT reset in
    /// `begin_frame` — `click_on_empty_bg_does_not_force_full`
    /// pins why.
    pub(crate) frame_state: FrameState,
    /// Set by [`Self::request_relayout`]; consumed by
    /// `record_phase` to trigger one re-record per
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
    /// matter; production apps should call [`Self::with_text`] with
    /// the shaper they're sharing with `WgpuBackend`.
    pub fn new() -> Self {
        Self::with_text(TextShaper::default())
    }

    /// Construct with an explicit shaper. Share the same handle with
    /// `WgpuBackend::set_text_shaper` so layout-time measurement and
    /// render-time shaping hit one buffer cache.
    pub fn with_text(text: TextShaper) -> Self {
        Self {
            forest: Forest::default(),
            theme: Theme::default(),
            state: StateMap::default(),
            text,
            layout: LayoutEngine::default(),
            frontend: Frontend::default(),
            input: InputState::new(),
            cascades: Cascades::default(),
            display: Display::default(),
            damage: Damage::default(),
            dt: 0.0,
            frame_id: 0,
            time: Duration::ZERO,
            repaint_requested: false,
            anim: AnimMap::default(),
            debug_overlay: None,
            frame_state: FrameState::default(),
            relayout_requested: false,
        }
    }

    // ── Frame lifecycle ───────────────────────────────────────────────

    /// The only public entry point for driving a frame. Runs `build`
    /// once, re-records on action input or `request_relayout`, paints
    /// the last pass. `now` is monotonic host time;
    /// `Ui::{dt,time,frame_id}` derive from it. See `docs/repaint.md`.
    pub fn run_frame(
        &mut self,
        display: Display,
        now: Duration,
        mut build: impl FnMut(&mut Ui),
    ) -> FrameOutput<'_> {
        let raw_dt = now.saturating_sub(self.time);
        self.dt = raw_dt.as_secs_f32().min(Self::MAX_DT);
        self.time = now;
        self.frame_id = self.frame_id.wrapping_add(1);
        self.repaint_requested = false;

        self.begin_frame(display);
        build(self);
        let action_flag = self.input.take_action_flag();
        let needs_relayout = self.record_phase();
        if action_flag {
            self.input.drain_per_frame_queues();
        }
        if action_flag || needs_relayout {
            // Pass B paints, regardless of any further re-record
            // request — caps relayout at one retry per `run_frame`.
            self.begin_frame(display);
            build(self);
            self.record_phase();
            self.relayout_requested = false;
        }
        self.paint_phase()
    }

    /// Feed a palantir-native input event. Hosts own redraw scheduling.
    pub fn on_input(&mut self, event: InputEvent) {
        self.input.on_input(event, &self.cascades.result);
    }

    /// Re-record this frame after measure runs (for widgets that
    /// realize their record-time inputs were stale). Capped at one
    /// re-record per `run_frame`.
    pub fn request_relayout(&mut self) {
        self.relayout_requested = true;
    }

    /// Start recording a frame. Auto-invalidates `damage.prev` on
    /// display change / first frame / dropped previous frame. See
    /// `docs/repaint.md` for the three-trigger detection and why
    /// `frame_state` is *not* reset here.
    pub(crate) fn begin_frame(&mut self, display: Display) {
        assert!(
            display.scale_factor >= f32::EPSILON,
            "Display::scale_factor must be ≥ f32::EPSILON; got {}",
            display.scale_factor,
        );
        let new_surface = display.logical_rect();
        let display_changed = self
            .damage
            .prev_surface
            .is_some_and(|prev| prev != new_surface);
        let frame_skipped = !self.frame_state.was_last_submitted();
        if display_changed || frame_skipped {
            tracing::debug!(
                display_changed,
                frame_skipped,
                first_frame = self.damage.prev_surface.is_none(),
                "damage.invalidate_prev"
            );
            self.damage.invalidate_prev();
        }
        self.display = display;
        self.forest.begin_frame();
        // `record_phase` consumes the flag via `mem::take`; a survivor
        // here means a missing record/paint pair on the prior frame.
        assert!(
            !self.relayout_requested,
            "begin_frame: relayout_requested smuggled across frames \
             — every frame must consume it via `record_phase`",
        );
    }

    /// Record-half of `end_frame`: finalize hashes, sweep evicted
    /// caches, run measure/arrange. Returns whether
    /// [`Self::request_relayout`] fired. Diffs (no commit) so a
    /// discarded pass A still sees the painted frame's `prev`.
    pub(crate) fn record_phase(&mut self) -> bool {
        let surface = self.display.logical_rect();
        self.forest.end_frame();
        // Sweep before `layout.run` so the measure cache compaction
        // sees a consistent live-set.
        self.forest.ids.diff_for_sweep();
        let removed = &self.forest.ids.removed;
        self.text.sweep_removed(removed);
        self.layout.sweep_removed(removed);
        self.state.sweep_removed(removed);
        self.anim.end_frame(removed);

        self.layout.run(&self.forest, surface, &self.text);

        std::mem::take(&mut self.relayout_requested)
    }

    /// Paint-half of `end_frame`: commit the seen-id rollover, then
    /// cascade → hit-index → damage → encode → compose. Reads the
    /// `LayoutResult` from the most recent `record_phase`.
    pub(crate) fn paint_phase(&mut self) -> FrameOutput<'_> {
        let surface = self.display.logical_rect();
        self.forest.ids.commit_rollover();
        let removed = &self.forest.ids.removed;

        let results = &self.layout.result;
        let cascades = self.cascades.run(&self.forest, results);
        self.input.end_frame(cascades);
        let damage = self
            .damage
            .compute(&self.forest, cascades, removed, surface);

        let damage_filter = match &damage {
            DamagePaint::Partial(region) => Some(region),
            DamagePaint::Full | DamagePaint::Skip => None,
        };
        let buffer = self.frontend.build(
            &self.forest,
            results,
            cascades,
            damage_filter,
            &self.display,
        );

        if matches!(damage, DamagePaint::Skip) {
            self.frame_state.mark_submitted();
        } else {
            self.frame_state.mark_pending();
        }
        FrameOutput {
            buffer,
            damage,
            repaint_requested: self.repaint_requested,
            debug_overlay: self.debug_overlay,
            frame_state: self.frame_state.clone(),
        }
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
            tint: Color::WHITE,
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
        let node = self.forest.open_node(element);
        f(self);
        self.forest.close_node();
        node
    }

    pub(crate) fn response_for(&self, id: WidgetId) -> ResponseState {
        let mut state = self.input.response_for(id, &self.cascades.result);
        // Cascade lags one frame; OR this frame's ancestor-disabled so
        // a freshly-disabled subtree paints disabled on its first frame.
        state.disabled |= self.forest.ancestor_disabled();
        state
    }

    // ── Cross-frame state & animation ─────────────────────────────────

    /// Cross-frame state row for `id`, `T::default()` on first
    /// access. Rows for `WidgetId`s not recorded this frame are
    /// evicted in `end_frame`. Panics on type collision at `id`.
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
