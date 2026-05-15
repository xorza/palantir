pub(crate) mod cascade;
pub(crate) mod damage;
pub(crate) mod frame_report;
pub(crate) mod frame_state;
pub(crate) mod state;

use crate::animation::animatable::Animatable;
use crate::animation::paint::PaintAnim;
use crate::animation::{AnimMap, AnimSlot, AnimSpec};
use crate::common::frame_arena::FrameArenaHandle;
use crate::debug_overlay::DebugOverlayConfig;
use crate::forest::Forest;
use crate::forest::element::{Configure, Element, LayoutMode};
use crate::forest::tree::Layer;
use crate::input::{FocusPolicy, InputDelta, InputEvent, InputState, ResponseState};
use crate::layout::Layout;
use crate::layout::layoutengine::LayoutEngine;
use crate::layout::types::display::Display;
use crate::layout::types::justify::Justify;
use crate::layout::types::sizing::Sizing;
use crate::primitives::approx::EPS;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::rect::Rect;
use crate::primitives::spacing::Spacing;
use crate::primitives::widget_id::WidgetId;
use crate::shape::Shape;
use crate::text::FontFamily;
use crate::text::TextShaper;
use crate::ui::cascade::{Cascades, CascadesEngine};
use crate::ui::damage::DamageEngine;
use crate::ui::frame_report::{FrameProcessing, FrameReport, RenderPlan};
use crate::ui::frame_state::FrameState;
use crate::ui::state::StateMap;
use crate::widgets::panel::Panel;
use crate::widgets::text::Text;
use crate::widgets::theme::{TextStyle, Theme};
use std::any::TypeId;
use std::ptr::NonNull;
use std::time::Duration;

/// Fixed substep used by the spring integrator and the `Ui::dt`
/// accumulator. Stability requires `dt·√k < ~1`; 1/240 s keeps the
/// product < 0.3 for `k ≤ 5000`. The `Ui` accumulator spends one
/// step per crossed threshold so each spent step is a single, stable
/// substep.
pub(crate) const FIXED_STEP_DT: f32 = 1.0 / 240.0;

/// Minimum gap between two scheduled repaint wakes. Wakes whose
/// deadline lands within this window of an existing entry collapse
/// to the earlier one — caps host wake-up rate at ~120 Hz under
/// bursts of `request_repaint_after`.
pub(crate) const REPAINT_COALESCE_DT: Duration = Duration::from_nanos(1_000_000_000 / 120);

/// Rects damaged "before the main pass runs" — owner paint-rects of
/// every paint anim whose quantum boundary fell in the
/// `(prev_time, now]` window. Walked by `DamageEngine::compute` and
/// folded into the per-frame damage region alongside the structural
/// diff. First frame (`prev_time == None`) fires every registered
/// anim, mirroring the structural diff's "no prev snapshot ⇒ damage
/// everything" path.
///
/// Free function rather than a `&self` method on [`Ui`] because the
/// call site reaches into `&self.forest`, `&self.layout.cascades`,
/// and `&mut self.damage_engine` field-by-field — a `&self`-borrowed
/// iterator would conflict with the `&mut self.damage_engine` borrow.
fn predamaged_rects<'a>(
    forest: &'a Forest,
    cascades: &'a Cascades,
    prev_time: Option<Duration>,
    now: Duration,
) -> impl Iterator<Item = Rect> + 'a {
    forest.iter_paint_order().flat_map(move |(layer, tree)| {
        let rows = cascades.rows_for(layer);
        tree.paint_anims.entries.iter().filter_map(move |e| {
            let fired = prev_time.is_none_or(|prev| e.anim.next_wake(prev) <= now);
            fired.then(|| rows[e.node.index()].paint_rect)
        })
    })
}

/// Type-erased pointer to caller-owned app state, installed for the
/// duration of [`Ui::frame`]. Retrieved via [`Ui::app`].
#[derive(Clone, Copy)]
struct AppSlot {
    ptr: NonNull<()>,
    type_id: TypeId,
}

/// Recorder + input/response broker. All public coordinates are
/// logical pixels (DIPs); `Display::scale_factor` converts to
/// physical at the wgpu boundary. See `docs/repaint.md` for the
/// frame-lifecycle rationale.
pub struct Ui {
    pub(crate) forest: Forest,
    pub theme: Theme,
    /// Per-frame debug visualizations. Default all-off; flip flags
    /// between frames. `damage_rect` / `dim_undamaged` are read by
    /// the backend at submit time; `frame_stats` is read by
    /// `Ui::frame` itself to auto-inject the FPS readout.
    pub debug_overlay: DebugOverlayConfig,
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
    /// Effective per-frame dt fed into the animation integrators
    /// (`AnimMapTyped::tick` / `spring::step`). Real wall-clock dt is
    /// accumulated into [`Self::dt_accum`] and only spent here once
    /// it crosses [`FIXED_STEP_DT`] — frames that don't spend
    /// see `dt = 0.0` and `tick` short-circuits the advance. Without
    /// this, NoVsync + `repaint_requested` spin the loop at 10s of
    /// kHz, `dt` drops to ~10 µs, and `cur += vel·dt` falls below the
    /// f32 ULP at pixel-scale positions — the spring integrator stalls
    /// short of settle eps and the loop never terminates.
    pub(crate) dt: f32,
    /// Unspent wall-clock dt waiting to cross the fixed-step
    /// threshold. See [`Self::dt`].
    pub(crate) dt_accum: f32,
    /// Bumped once per [`Self::run_frame`], before either pass —
    /// pinned by `run_frame_pass_count_matches_action_trigger`.
    pub(crate) frame_id: u64,
    /// Host-supplied monotonic timestamp for this frame.
    pub(crate) time: Duration,
    /// `time` from the previous successful `frame()` call, or `None`
    /// before the first frame and after [`DamageEngine::invalidate_prev`]
    /// rewinds the snapshot. Drives the cross-frame "did a paint
    /// anim's quantum boundary fall in this gap" check —
    /// `entry.anim.next_wake(prev_time) <= now` fires the rect into
    /// damage. Updated at the end of `frame_inner` on every path.
    pub(crate) prev_time: Option<Duration>,
    /// Logical surface rect from the previous successful frame, or
    /// `None` on first frame / after invalidation. Compared against
    /// the current `display.logical_rect()` in
    /// `should_invalidate_prev` (surface change ⇒ force full) and in
    /// the paint-anim gate (must match for the short-circuit).
    /// Updated at the bottom of `frame_inner` on every path.
    pub(crate) prev_surface: Option<Rect>,
    /// EMA of `1/raw_dt` across frames. Zero on the first frame
    /// (no prior `time` to diff against); updated in
    /// [`Self::frame`]. Surfaced by the `frame_stats` debug overlay.
    pub(crate) fps_ema: f32,
    /// Set by [`Self::animate`] when an animation hasn't settled.
    pub(crate) repaint_requested: bool,
    /// Pending wake-up deadlines (absolute Ui-time, sorted ascending,
    /// dedup'd). Survive across frames — callers schedule once via
    /// [`Self::request_repaint_after`] and the entry stays until its
    /// deadline fires, at which point [`Self::frame`] drains it at the
    /// top of the next frame. Hosts read the earliest pending entry
    /// off [`FrameReport::repaint_after`] and pair with
    /// `winit::ControlFlow::WaitUntil` (or equivalent).
    pub(crate) repaint_wakes: Vec<Duration>,
    pub(crate) anim: AnimMap,
    /// Submission status of the last *painted* frame. NOT reset in
    /// `pre_record` — `click_on_empty_bg_does_not_force_full`
    /// pins why.
    pub(crate) frame_state: FrameState,
    /// Set by [`Self::request_relayout`]; consumed by
    /// `post_record` to trigger one re-record per
    /// `run_frame`.
    relayout_requested: bool,
    /// Ambient caller-owned app state for the current frame. Installed
    /// by [`Self::frame`], cleared by the RAII guard on scope exit
    /// (incl. panic). Retrieved via [`Self::app`].
    app_slot: Option<AppSlot>,
    /// Per-frame bulk geometry arena (mesh verts/indices, polyline
    /// points/colors), shared with the renderer via [`Host`]: `Host`
    /// constructs the canonical [`Rc`] and clones it into `Ui`,
    /// `Frontend`, and `WgpuBackend` so every phase sees the same
    /// bytes. Standalone `new_ui()` builds its own private handle.
    /// `add_shape` calls `borrow_mut()` for the call duration.
    ///
    /// [`Host`]: crate::Host
    pub(crate) frame_arena: FrameArenaHandle,
}

impl Ui {
    /// Per-frame `dt` clamp (seconds). Stalled frames freeze
    /// animation tickers instead of teleporting; [`Self::time`]
    /// still tracks the host's true clock.
    pub(crate) const MAX_DT: f32 = 0.1;

    /// Construct with an explicit shaper *and* a shared frame-arena
    /// handle. The same `TextShaper` handle must reach the wgpu
    /// backend so layout-time measurement and render-time shaping
    /// hit one buffer cache; the same `FrameArenaHandle` must reach
    /// `Frontend` and `WgpuBackend` so every phase sees the same
    /// per-frame mesh / polyline bytes. [`crate::Host::new`] wires
    /// both at construction time.
    ///
    /// Tests / standalone callers usually want [`Self::default`],
    /// which builds an isolated `Ui` with mono fallback shaper + its
    /// own private arena.
    pub fn new(text: TextShaper, frame_arena: FrameArenaHandle) -> Self {
        Self {
            forest: Forest::default(),
            theme: Theme::default(),
            debug_overlay: DebugOverlayConfig::default(),
            state: StateMap::default(),
            text,
            layout_engine: LayoutEngine::default(),
            layout: Layout::default(),
            input: InputState::new(),
            cascades_engine: CascadesEngine::default(),
            display: Display::default(),
            damage_engine: DamageEngine::default(),
            dt: 0.0,
            dt_accum: 0.0,
            frame_id: 0,
            time: Duration::ZERO,
            prev_time: None,
            prev_surface: None,
            fps_ema: 0.0,
            anim: AnimMap::default(),
            frame_state: FrameState::default(),
            relayout_requested: false,
            repaint_requested: false,
            repaint_wakes: Vec::new(),
            app_slot: None,
            frame_arena,
        }
    }

    /// Borrow the app state installed by the enclosing [`Self::frame`].
    /// Panics if no slot is installed or `T` doesn't match the installed
    /// type — both are caller bugs, not runtime conditions.
    pub fn app<T: 'static>(&mut self) -> &mut T {
        let slot = self
            .app_slot
            .expect("Ui::app called with no app state installed");
        assert!(
            slot.type_id == TypeId::of::<T>(),
            "Ui::app::<T>() type mismatch — installed type differs from requested",
        );
        // SAFETY: `frame` borrows `state: &mut T` for its full duration;
        // `Ui::app` reborrows through `&mut self` so the returned
        // `&mut T` can't alias another `Ui` access. The Guard restores
        // `prev` on drop (incl. panic), so the pointer is live whenever
        // the slot is `Some`.
        unsafe { slot.ptr.cast::<T>().as_mut() }
    }

    // ── Frame lifecycle ───────────────────────────────────────────────

    /// The only public entry point for driving a frame. Installs
    /// `state` as ambient app state visible to deep widgets via
    /// [`Self::app::<T>()`] for the duration of the call (RAII-restored
    /// on scope exit, incl. panic). Runs `record` once, re-records on
    /// action input or `request_relayout`, paints the last pass.
    /// Callers without app state pass `&mut ()`. `now` is monotonic
    /// host time; `Ui::{dt,time,frame_id}` derive from it. See
    /// `docs/repaint.md`.
    pub fn frame<T: 'static>(
        &mut self,
        display: Display,
        now: Duration,
        state: &mut T,
        mut record: impl FnMut(&mut Ui),
    ) -> FrameReport {
        // The frame arena is shared via Rc so the renderer sees the
        // same bytes. Clear it once at the top of the record cycle;
        // capacity is retained.
        self.frame_arena.borrow_mut().clear();
        // Install `state` as the ambient app slot for this frame; RAII
        // guard restores the prior slot on scope exit (incl. panic) so
        // nested frames stack cleanly.
        struct Guard<'a> {
            ui: &'a mut Ui,
            prev: Option<AppSlot>,
        }
        impl Drop for Guard<'_> {
            fn drop(&mut self) {
                self.ui.app_slot = self.prev;
            }
        }
        let prev = self.app_slot.replace(AppSlot {
            ptr: NonNull::from(state).cast(),
            type_id: TypeId::of::<T>(),
        });
        let g = Guard { ui: self, prev };
        g.ui.frame_inner(display, now, &mut record)
    }

    fn frame_inner(
        &mut self,
        display: Display,
        now: Duration,
        mut record: impl FnMut(&mut Ui),
    ) -> FrameReport {
        profiling::scope!("Ui::frame");
        assert!(
            display.scale_factor >= EPS,
            "Display::scale_factor must be ≥ EPSILON; got {}",
            display.scale_factor,
        );

        let raw_dt = now
            .saturating_sub(self.time)
            .as_secs_f32()
            .min(Self::MAX_DT);
        // EMA over instantaneous fps. First frame: raw_dt is `now` (vs
        // ZERO), giving an absurd reading the first frame; skip the
        // update there. Coefficient 0.1 ≈ ~10-frame window — smooth
        // enough that the readout doesn't jitter wildly, fast enough
        // to track real frame-rate drops.
        if self.frame_id > 0 && raw_dt > EPS {
            let inst = 1.0 / raw_dt;
            self.fps_ema = if self.fps_ema == 0.0 {
                inst
            } else {
                self.fps_ema * 0.9 + inst * 0.1
            };
        }
        self.dt_accum += raw_dt;
        // Fixed-step accumulator. Spend the whole bucket once we
        // cross the threshold (spring substeps it internally); leave
        // `dt = 0.0` on frames that don't cross so `tick`
        // short-circuits without churning the integrator below f32
        // precision.
        self.dt = if self.dt_accum >= FIXED_STEP_DT {
            let spent = self.dt_accum;
            self.dt_accum = 0.0;
            spent
        } else {
            0.0
        };
        self.time = now;
        self.frame_id += 1;
        self.repaint_requested = false;
        // Drop wakes whose deadline has fired (this frame is at or
        // past them). Sorted-ascending invariant means a prefix slice.
        let fired = self.repaint_wakes.partition_point(|&d| d <= self.time);
        self.repaint_wakes.drain(..fired);
        self.relayout_requested = false;

        let force_full = self.should_invalidate_prev(display);
        if force_full {
            self.damage_engine.invalidate_prev();
            self.prev_time = None;
            self.prev_surface = None;
        }
        self.display = display;

        // Pending until the renderer (`Host::render`) confirms a
        // successful submit. Tests driving `Ui::frame` directly must
        // ack via `ui.frame_state.mark_submitted()` or the next
        // frame's `should_invalidate_prev` will force a `Full`.
        self.frame_state.mark_pending();

        let action_flag = {
            profiling::scope!("Ui::record_pass.A");
            self.record_pass(&mut record)
        };
        let double_layout = action_flag || self.relayout_requested;
        if double_layout {
            profiling::scope!(
                "Ui::record_pass.B",
                if self.relayout_requested {
                    "relayout"
                } else {
                    "action"
                }
            );
            // Pass B paints, regardless of any further re-record
            // request — caps relayout at one retry per `run_frame`.
            self.input.drain_per_frame_queues();
            let _ = self.record_pass(&mut record);
        }

        self.finalize_frame();

        let damage = self.damage_engine.compute(
            &self.forest,
            &self.layout.cascades,
            &self.forest.ids.removed,
            self.display.logical_rect(),
            force_full,
            (!force_full)
                .then(|| {
                    predamaged_rects(
                        &self.forest,
                        &self.layout.cascades,
                        self.prev_time,
                        self.time,
                    )
                })
                .into_iter()
                .flatten(),
        );

        // Skip frames have nothing for the host to submit, so ack
        // here — otherwise `frame_state` stays `Pending` and the next
        // paint frame's `should_invalidate_prev` escalates to `Full`.
        if damage.is_none() {
            self.frame_state.mark_submitted();
        }

        self.prev_time = Some(self.time);
        self.prev_surface = Some(self.display.logical_rect());

        FrameReport {
            repaint_requested: self.repaint_requested,
            repaint_after: self.repaint_wakes.first().copied(),
            plan: RenderPlan::from_damage(damage, self.theme.window_clear),
            processing: if double_layout {
                FrameProcessing::DoubleLayout
            } else {
                FrameProcessing::SingleLayout
            },
        }
    }

    /// Should we discard the last painted frame's damage snapshot? True
    /// on first frame, on a surface-rect change, or when the host
    /// failed to confirm submission of the last frame.
    fn should_invalidate_prev(&self, new_display: Display) -> bool {
        let new_surface = new_display.logical_rect();
        let display_changed = self.prev_surface.is_some_and(|prev| prev != new_surface);
        let frame_skipped = !self.frame_state.was_last_submitted();
        let invalidate = display_changed || frame_skipped;
        if invalidate {
            tracing::debug!(
                display_changed,
                frame_skipped,
                first_frame = self.prev_surface.is_none(),
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
        // Synthetic viewport root for Layer::Main. Without this, the
        // first user-recorded node becomes the root and the layout
        // engine forces its rect to the surface — silently overriding
        // declared `Sizing` / `Sense` on the top-level widget. ZStack +
        // Fill matches the historical "root paints full surface"
        // behavior while letting user roots respect their own sizing.
        let mut viewport = Element::new(LayoutMode::ZStack);
        viewport.size = Sizing::FILL.into();
        self.forest.open_node(viewport);
        {
            profiling::scope!("Ui::record_user");
            record(self);
        }
        let action_flag = self.input.take_action_flag();
        if self.debug_overlay.frame_stats {
            self.record_frame_stats();
        }
        self.forest.close_node();
        self.post_record();
        action_flag
    }

    /// Append the `frame_stats` readout into `Layer::Debug` pinned to
    /// the top-right of the viewport, wrapped in a semi-transparent
    /// black chrome so it stays legible against any background.
    /// Records every frame so the text changes (and damage picks up
    /// the small rect), which keeps the FPS readout ticking on
    /// otherwise-idle frames.
    fn record_frame_stats(&mut self) {
        let label = format!("f {} · {:>4.0} fps", self.frame_id, self.fps_ema);
        let style = TextStyle {
            family: FontFamily::Mono,
            color: Color::rgb(1.0, 0.2, 0.2),
            font_size_px: 12.0,
            ..self.theme.text
        };
        let chrome = Background {
            fill: Color::linear_rgba(0.0, 0.0, 0.0, 0.75).into(),
            ..Default::default()
        };
        self.layer(Layer::Debug, glam::Vec2::ZERO, None, |ui| {
            Panel::hstack()
                .size((Sizing::FILL, Sizing::Hug))
                .justify(Justify::End)
                .show(ui, |ui| {
                    Panel::hstack()
                        .background(chrome)
                        .size((Sizing::Hug, Sizing::Hug))
                        .padding(Spacing::xy(4.0, 2.0))
                        .show(ui, |ui| {
                            Text::new(label).style(style).show(ui);
                        });
                });
        });
    }

    /// Feed a palantir-native input event. Returns an [`InputDelta`]
    /// the host reads to decide whether to request a redraw — pointer
    /// moves over inert surfaces leave `requests_repaint` false so the
    /// host can skip the frame entirely. Animation/tooltip-delay wakes
    /// still drive paints independently via `FrameReport::repaint_after`.
    pub fn on_input(&mut self, event: InputEvent) -> InputDelta {
        self.input.on_input(event, &self.layout.cascades)
    }

    /// Re-record this frame after measure runs (for widgets that
    /// realize their record-time inputs were stale). Capped at one
    /// re-record per `run_frame`.
    pub fn request_relayout(&mut self) {
        self.relayout_requested = true;
    }

    /// Ask the host to schedule another frame after this one. Cleared
    /// at the top of every `frame`; widgets/showcases that need
    /// continuous animation call this each frame to keep the host
    /// awake.
    #[track_caller]
    pub fn request_repaint(&mut self) {
        let caller = std::panic::Location::caller();
        tracing::info!(
            target: "palantir.repaint",
            "request_repaint @ {}:{} (frame={})",
            caller.file(),
            caller.line(),
            self.frame_id,
        );
        self.repaint_requested = true;
    }

    /// Schedule a one-shot wake at `now + after`. The entry persists
    /// across frames; [`Self::frame`] drains entries whose deadline
    /// has fired at the top of each frame. Duplicate deadlines collapse
    /// (sorted + dedup'd), so re-requesting the same wake is a no-op.
    ///
    /// Callers don't need to re-request each frame. To cancel, schedule
    /// nothing else — the wake will fire once, the next frame will run
    /// briefly, and the queue drains.
    #[track_caller]
    pub fn request_repaint_after(&mut self, after: Duration) {
        let caller = std::panic::Location::caller();
        tracing::trace!(
            target: "palantir.repaint",
            "request_repaint_after({:?}) @ {}:{} (frame={})",
            after,
            caller.file(),
            caller.line(),
            self.frame_id,
        );
        let deadline = self.time.saturating_add(after);
        let pos = match self.repaint_wakes.binary_search(&deadline) {
            Ok(_) => return,
            Err(pos) => pos,
        };
        let near = |existing: Duration| existing.abs_diff(deadline) < REPAINT_COALESCE_DT;
        // Coalesce to the later of (existing, requested) — collapse
        // bursts into a single wake at the back of the window to avoid
        // unnecessary host wakes. pos-1 is earlier than deadline
        // (overwrite with ours); pos is later (skip insert).
        if pos < self.repaint_wakes.len() && near(self.repaint_wakes[pos]) {
            return;
        }
        if pos > 0 && near(self.repaint_wakes[pos - 1]) {
            self.repaint_wakes[pos - 1] = deadline;
            return;
        }
        self.repaint_wakes.insert(pos, deadline);
    }

    fn pre_record(&mut self) {
        profiling::scope!("Ui::pre_record");
        self.forest.pre_record();
    }

    /// Record-half of `frame`: finalize hashes, run measure / arrange,
    /// then cascade. Cascade runs here (not in `finalize_frame`) so
    /// pass B of a `request_relayout` frame reads pass A's arranged
    /// rects via [`Self::response_for`] like steady-state frames do.
    /// Stale cache entries (for widgets recorded last frame but
    /// absent this pass) are tolerated through `layout.run` — they
    /// can't match live keys — and reaped once in `finalize_frame`
    /// against the final pass's id set.
    fn post_record(&mut self) {
        profiling::scope!("Ui::post_record");
        let min_wake = self.forest.post_record(self.time);
        if min_wake != Duration::MAX {
            self.request_repaint_after(min_wake.saturating_sub(self.time));
        }
        self.layout_engine.run(
            &self.forest,
            self.display.logical_rect(),
            &self.text,
            &mut self.layout,
        );
        self.cascades_engine.run(&self.forest, &mut self.layout);
    }

    /// Paint-half of `frame`: diff seen ids against the last painted
    /// frame, fan the `removed` set out to per-widget caches, run
    /// input/damage against the final pass's cascade. Sweep runs
    /// here (once per `frame`) rather than per `post_record` so a
    /// widget that vanishes in pass A but returns in pass B keeps
    /// its state across the discard.
    fn finalize_frame(&mut self) {
        profiling::scope!("Ui::finalize_frame");
        let removed = self.forest.ids.rollover();
        self.text.sweep_removed(removed);
        self.layout_engine.sweep_removed(removed);
        self.state.sweep_removed(removed);
        self.anim.sweep_removed(removed);

        self.input.post_record(&self.layout.cascades);
    }

    // ── Recording (widget-facing) ─────────────────────────────────────

    pub fn add_shape(&mut self, shape: Shape<'_>) {
        let mut arena = self.frame_arena.borrow_mut();
        self.forest.add_shape(shape, &mut arena);
    }

    /// Append `shape` to the active node and register `anim` against
    /// it. The encoder samples `anim` at paint time and folds the
    /// resulting `PaintMod` into the shape's brush; `post_record`
    /// folds the anim's `next_wake` into `repaint_wakes` so the
    /// caller doesn't manage scheduling. Drops silently if the shape
    /// itself was noop-collapsed (zero stroke + transparent fill,
    /// etc.) — `PaintAnim` can't make a zero shape paintable.
    pub fn add_shape_animated(&mut self, shape: Shape<'_>, anim: PaintAnim) {
        let mut arena = self.frame_arena.borrow_mut();
        self.forest.add_shape_animated(shape, anim, &mut arena);
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

    /// Open a node with no paint chrome — the common path for layout-only
    /// containers, text leaves, and chrome-less Frames. Avoids passing
    /// a 232-byte `Option<Background>` through the call chain.
    pub(crate) fn node(&mut self, element: Element, f: impl FnOnce(&mut Ui)) {
        self.forest.open_node(element);
        f(self);
        self.forest.close_node();
    }

    /// Open a node with a paint chrome. Widgets that always set chrome
    /// (`Button`, `MenuItem`) or that resolved a theme fallback to
    /// `Some(_)` call this; others use [`Self::node`] to skip the
    /// 232-byte chrome arg.
    pub(crate) fn node_with_chrome(
        &mut self,
        element: Element,
        chrome: Background,
        f: impl FnOnce(&mut Ui),
    ) {
        self.forest.open_node_with_chrome(element, chrome);
        f(self);
        self.forest.close_node();
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

    /// Read-only peek at the cross-frame state row for `id`. `None` if
    /// nothing has been stored for `(id, T)` yet — does not allocate or
    /// mutate. Use this on the `&Ui` side (probes, hit-test helpers,
    /// "is this menu open?" checks) where `state_mut`'s `&mut Ui`
    /// receiver would be a needless borrow upgrade.
    pub fn try_state<T: 'static>(&self, id: WidgetId) -> Option<&T> {
        self.state.try_get::<T>(id)
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
        // Hottest path: no spec, no typed map for `T` ever allocated.
        // Skip the `slot.into()`, filter closure, and TypeId-keyed
        // HashMap probe — they're per-widget per-frame on a widget
        // that never animates (the dominant case in static UIs).
        if self.anim.by_type.is_empty() && spec.is_none_or(|s| s.is_instant()) {
            return target;
        }
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

    /// Active `Display` (physical surface size + scale factor). Read
    /// by example/demo code that wants to inject synthetic input
    /// coordinates without threading window dimensions through itself.
    pub fn display(&self) -> Display {
        self.display
    }

    /// Programmatically set or clear focus. Bypasses [`FocusPolicy`].
    pub fn request_focus(&mut self, id: Option<WidgetId>) {
        self.input.focused = id;
    }

    /// Current pointer position in logical pixels (surface space), or
    /// `None` if the pointer has left the surface.
    pub fn pointer_pos(&self) -> Option<glam::Vec2> {
        self.input.pointer_pos
    }

    pub fn focus_policy(&self) -> FocusPolicy {
        self.input.focus_policy
    }

    /// Set the press-on-non-focusable behavior. See [`FocusPolicy`].
    pub fn set_focus_policy(&mut self, p: FocusPolicy) {
        self.input.focus_policy = p;
    }

    /// `true` if Escape was pressed this frame. Used by
    /// [`crate::widgets::context_menu::ContextMenu`] to dismiss on Esc;
    /// can also be read by host code for modal-style behaviors.
    pub fn escape_pressed(&self) -> bool {
        use crate::input::keyboard::Key;
        self.input.frame_keys.iter().any(|k| k.key == Key::Escape)
    }

    /// `true` if any keypress this frame matches `s`. Reads the same
    /// `frame_keys` buffer that [`Self::escape_pressed`] uses — single
    /// canonical entrypoint so widgets stop reaching for
    /// `ui.input.frame_keys` directly.
    pub fn shortcut_pressed(&self, s: crate::input::shortcut::Shortcut) -> bool {
        self.input.frame_keys.iter().any(|kp| s.matches(*kp))
    }
}

#[cfg(test)]
mod tests;
