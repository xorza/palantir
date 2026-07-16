pub(crate) mod cascade;
pub(crate) mod damage;
pub(crate) mod frame;
pub(crate) mod frame_report;
pub(crate) mod state;

use crate::InternedStr;
use crate::animation::animatable::Animatable;
use crate::animation::{AnimMap, AnimSlot, AnimSpec};
use crate::common::time::{ANIM_SUBSTEP_DT, coalesce_dt_for_refresh};
use crate::debug_overlay::DebugOverlayConfig;
use crate::display::Display;
use crate::forest::Forest;
use crate::forest::element::Element;
use crate::forest::layer::Layer;
use crate::forest::shapes::lower::ChromeInput;
use crate::forest::tree::paint_anims::PaintAnim;
use crate::host::context::HostContext;
use crate::input::keyboard::{KeyboardEvent, Modifiers};
use crate::input::pointer::PointerEvent;
use crate::input::policy::FocusPolicy;
use crate::input::policy::InputPolicy;
use crate::input::response::{InputDelta, ResponseState};
use crate::input::shortcut::Shortcut;
use crate::input::subscriptions::{KeyboardSense, PointerSense};
use crate::input::{InputEvent, InputState};
use crate::layout::Layout;
use crate::layout::engine::LayoutEngine;
use crate::layout::support::TextCtx;
use crate::layout::types::layout_mode::LayoutMode;
use crate::layout::types::sizing::Sizing;
use crate::primitives::approx::EPS;
use crate::primitives::background::Background;
use crate::primitives::image::Image;
use crate::primitives::size::Size;
use crate::primitives::widget_id::WidgetIdMap;
use crate::record_store::RecordStore;
use crate::renderer::gpu_view::{GpuPaint, GpuPaintRef, GpuViewEntry};
use crate::renderer::image_registry::ImageHandle;

use crate::debug_overlay::record_frame_stats;
use crate::primitives::widget_id::WidgetId;
use crate::shape::Shape;
use crate::ui::cascade::{Cascades, CascadesEngine, cascade_fingerprint};
use crate::ui::damage::{Damage, DamageEngine, DamageInput};
use crate::ui::frame::{FramePlan, FrameRuntime, FrameStamp, Wake, WakeReasons};
use crate::ui::frame_report::{FrameProcessing, FrameReport, RenderPlan};
use crate::ui::state::StateMap;
use crate::widgets::theme::Theme;
use crate::window::{
    CursorIcon, PendingWindow, WindowConfig, WindowGeometry, WindowMailbox, WindowToken,
};
use glam::UVec2;
use std::cell::{RefCell, RefMut};
use std::collections::hash_map::Entry;
use std::panic::Location;
use std::rc::Rc;
use std::time::Duration;

/// Recorder + input/response broker. All public coordinates are
/// logical pixels (DIPs); `Display::scale_factor` converts to
/// physical at the wgpu boundary. Frame scheduling state is retained
/// internally.
pub struct Ui {
    pub(crate) forest: Forest,
    pub theme: Theme,
    /// Cross-frame widget state: per-type dense stores keyed by
    /// `WidgetId` (see [`StateMap`]).
    pub(crate) state: StateMap,
    /// Live `GpuView`s, keyed by `WidgetId` — the only `GpuView` bookkeeping on
    /// the `Ui`. [`Self::gpu_view`] upserts an entry (minting the stable backend
    /// `TextureId` once, refreshing the paint callback); the shape records only
    /// the redraw epoch and the encoder looks the view up here by the node's
    /// `WidgetId`. Swept by the same `removed` set as [`StateMap`].
    pub(crate) gpu_views: WidgetIdMap<GpuViewEntry>,
    /// This window's retained record store. Cleared only when a record pass
    /// rebuilds the forest; `PaintOnly` keeps it paired with the retained tree.
    pub(crate) record_store: RecordStore,
    /// App-global shared state cloned from the host at construction: the
    /// render resources (`ctx.shaper` / `ctx.caches` / `ctx.pass_stats`) and
    /// the host state behind it (the live-window set,
    /// read by [`Self::window_open`], and the debug overlay, read at submit
    /// time + by `Ui::frame` for the FPS readout, toggled via
    /// [`Self::debug_overlay_mut`]). One handle so a new shared field
    /// doesn't touch every `Ui` constructor; the wgpu backend clones the
    /// same context (render handles only), so both see one set.
    pub(crate) ctx: HostContext,
    pub(crate) layout_engine: LayoutEngine,
    pub(crate) layout: Layout,
    /// Cascaded clip/disabled/invisible/transform per node + global
    /// hit index. Written by `CascadesEngine::run` in the paint phase
    /// and read by the encoder, input dispatch, and damage compute.
    pub(crate) cascades: Cascades,
    pub(crate) input: InputState,
    /// Selects which "did input arrive?" signal `classify_frame`
    /// consults to gate the full record path. Default
    /// [`InputPolicy::OnDelta`] skips record on inert pointer moves
    /// and scroll-over-nothing; flip to [`InputPolicy::Always`] for
    /// telemetry / custom canvases that need every event.
    pub input_policy: InputPolicy,
    pub(crate) cascades_engine: CascadesEngine,
    pub(crate) display: Display,
    pub(crate) damage_engine: DamageEngine,
    pub(crate) anim: AnimMap,
    /// Retained frame clock, wake queue, repaint/relayout flags, and prior-frame
    /// validity state. Kept separate from the widget engines above.
    pub(crate) frame_runtime: FrameRuntime,
    /// Deferred open/close requests and host-refreshed close, geometry, and
    /// cursor state. Inert in headless contexts without a windowing host.
    pub(crate) window_mailbox: WindowMailbox,
}

impl std::fmt::Debug for Ui {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ui")
            .field("frame_id", &self.frame_runtime.frame_id)
            .field("time", &self.frame_runtime.time)
            .field("display", &self.display)
            .finish_non_exhaustive()
    }
}

/// Construction + host-driven frame lifecycle: `frame` and the private
/// record / clock / classify / cascade / finalize passes it runs. User
/// code never calls these directly — `WindowRenderer` drives them. The widget
/// authoring API lives in the second `impl Ui` block below.
impl Ui {
    /// Per-frame `dt` clamp (seconds). Stalled frames freeze
    /// animation tickers instead of teleporting; the frame runtime's
    /// clock still tracks the host's true time.
    pub(crate) const MAX_DT: f32 = 0.1;

    /// Construct a per-window `Ui` from the host's shared [`HostContext`] and
    /// the window's [`RecordStore`]. Through the context the `Ui` shares the
    /// same `TextShaper` the wgpu backend uses (so layout-time measurement and
    /// render-time shaping hit one buffer cache), the render caches +
    /// GPU-stats, and the app-global host state (live-window set + debug
    /// overlay). The store is shared only with this window's renderer so
    /// retained shape spans remain valid across other windows' record passes.
    pub(crate) fn new(ctx: &HostContext, record_store: RecordStore) -> Self {
        Self {
            ctx: ctx.clone(),
            record_store,
            forest: Default::default(),
            theme: Default::default(),
            state: Default::default(),
            gpu_views: Default::default(),
            layout_engine: Default::default(),
            layout: Default::default(),
            cascades: Default::default(),
            input: Default::default(),
            input_policy: Default::default(),
            cascades_engine: Default::default(),
            display: Default::default(),
            damage_engine: Default::default(),
            anim: Default::default(),
            frame_runtime: Default::default(),
            window_mailbox: Default::default(),
        }
    }

    /// The only public entry point for driving a frame. Runs `record`
    /// once, re-records on action input or `request_relayout`, paints
    /// the last pass. `stamp.time` is monotonic host time;
    /// the retained clock and frame id derive from it.
    pub fn frame(&mut self, stamp: FrameStamp, mut record: impl FnMut(&mut Ui)) -> FrameReport {
        profiling::scope!("Ui::frame");
        // Record payloads are cleared inside `record_pass` (the only path
        // that repopulates it). PaintOnly frames must NOT clear: the
        // live `tree.shapes` from last frame still references record payloads
        // contents by index (gradients, polyline points/colors, mesh
        // verts/indices, interned text spans). Clearing here would
        // leave dangling indices the encoder then dereferences.
        debug_assert!(
            stamp.display.scale_factor >= EPS,
            "Display::scale_factor must be ≥ EPSILON; got {}",
            stamp.display.scale_factor,
        );

        let first_frame = self.frame_runtime.prev_stamp.is_none();
        self.advance_clock(stamp.time);
        // Refresh the input clock so input handlers running before the
        // next frame timestamp double-clicks on this deterministic time.
        self.input.frame_time = self.frame_runtime.time;
        let plan = self.classify_frame(stamp.display);

        self.frame_runtime.repaint_requested = false;
        self.frame_runtime.relayout_requested = false;
        self.display = stamp.display;

        // Pending until the renderer (`WindowRenderer::render_to_texture`)
        // confirms a successful submit. Tests driving `Ui::frame` directly must
        // ack via `ui.frame_runtime.frame_submitted = true` or the next
        // frame's `classify_frame` will force a `Full`.
        self.frame_runtime.frame_submitted = false;

        let processing = match plan {
            FramePlan::PaintOnly => {
                profiling::scope!("Ui::frame.paint_only");
                // PaintOnly skips `record_pass` → skips `post_record`
                // → skips the input cleanup. Under `OnDelta`, an
                // unrouted event can still land here with the sticky
                // arrival flag set even though no queue accepted it.
                self.input.drain_per_frame_queues();
                FrameProcessing::PaintOnly
            }
            FramePlan::FullRecord { .. } => {
                // Cold-start warmup: on the very first frame, cascades
                // is empty, so any `on_input` events delivered between
                // window-open and now hit-tested against nothing
                // (`hovered`/`scroll_target`/etc. stayed None). Run a
                // blackout record pass — input swapped out for an
                // empty `InputState` so widgets see no pointer, no
                // keys, no queued events — purely to build the cascade.
                // Then restore the real input and re-route the held
                // `pointer_pos` against the just-built cascade so the
                // user-visible pass below records against correct
                // hover targets. Without this, frame 1 widgets see
                // None response_rects (one-frame stale on `text_edit`,
                // `scroll`, `radio`, …) and a pointer hovering a
                // button doesn't actually hover it until frame 2.
                if first_frame {
                    profiling::scope!("Ui::record_pass.warmup");
                    let saved_input = std::mem::take(&mut self.input);
                    let _ = self.record_pass(&mut record);
                    self.input = saved_input;
                    self.input.refresh_pointer_targets(&self.cascades);
                    // Discard any relayout/repaint requests issued
                    // during the blackout pass — the warmup is
                    // "pretend it didn't happen" from the gate's
                    // perspective. Without this, a widget that asks
                    // for relayout in its first record (legitimate)
                    // would trigger the existing `double_layout` arm
                    // *on top of* the warmup, giving three record
                    // passes on frame 1 instead of two.
                    self.frame_runtime.relayout_requested = false;
                    self.frame_runtime.repaint_requested = false;
                }
                let action_flag = {
                    profiling::scope!("Ui::record_pass.A");
                    self.record_pass(&mut record)
                };
                let double_layout = action_flag || self.frame_runtime.relayout_requested;
                if double_layout {
                    profiling::scope!(
                        "Ui::record_pass.B",
                        if self.frame_runtime.relayout_requested {
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

                if double_layout {
                    FrameProcessing::DoubleLayout
                } else {
                    FrameProcessing::SingleLayout
                }
            }
        };

        // Damage compute reads `ids.removed` to know which widgets
        // dropped between frames. On `PaintOnly` no widgets were
        // recorded so nothing was removed — pass an empty set
        // instead of stale state from the previous frame.
        let surface = self.display.logical_rect();
        let prev_time = self.frame_runtime.prev_stamp.map(|s| s.time);
        let input = DamageInput {
            forest: &self.forest,
            cascades: &self.cascades,
            surface,
            prev_time,
            now: self.frame_runtime.time,
        };
        let damage = match plan {
            FramePlan::PaintOnly => self.damage_engine.compute_paint_only(input),
            FramePlan::FullRecord { force_full } => {
                self.damage_engine
                    .compute(input, &self.forest.ids.removed, force_full)
            }
        };

        // First-frame contract: no prev snapshot to diff against, so
        // every painting widget is "new" — `damage_engine.compute`
        // must return `Damage::Full`. The walk itself is still
        // load-bearing (seeds `prev` for frame 2's incremental diff)
        // so we keep the call; the assert just pins the invariant.
        debug_assert!(
            !first_frame || matches!(damage, Damage::Full),
            "first frame must produce Damage::Full; got {damage:?}",
        );

        // Skip frames have nothing for the host to submit, so ack
        // here — otherwise `frame_submitted` stays false and the next
        // paint frame's `classify_frame` escalates to `Full`.
        if damage.is_skip() {
            self.frame_runtime.frame_submitted = true;
        }

        // Re-queue the next paint-anim boundary regardless of path.
        // FullRecord rebuilt `paint_anims.entries` during record;
        // PaintOnly retained last frame's. Either way the fold below
        // gives the next quantum boundary — without this, PaintOnly
        // drains the queued ANIM wake without replacing it and the
        // caret freezes until input forces a FullRecord.
        if let Some(min_wake) = self.forest.min_paint_anim_wake(self.frame_runtime.time) {
            self.schedule_wake(min_wake, WakeReasons::ANIM);
        }

        self.frame_runtime.prev_stamp = Some(stamp);

        FrameReport {
            repaint_requested: self.frame_runtime.repaint_requested,
            repaint_after: self.frame_runtime.repaint_wakes.first().map(|w| w.deadline),
            plan: RenderPlan::from_damage(damage, self.theme.window_clear),
            processing,
        }
    }

    /// Advance the per-frame clock: clamp raw dt (stalled frames
    /// freeze tickers instead of teleporting), update the fps EMA,
    /// step the fixed-step `dt`/`dt_accum` integrator, and bump
    /// `time` + `frame_id`. The fixed-step accumulator zeroes `dt`
    /// until it crosses [`ANIM_SUBSTEP_DT`] so spring integrators
    /// don't churn below f32 ULP between vsync ticks.
    fn advance_clock(&mut self, now: Duration) {
        // The fps EMA reads the true delta — clamping would record a
        // multi-second stall as exactly `1/MAX_DT` fps, hiding the
        // hitches the readout exists to surface. Only the animation
        // integrator below wants the clamp.
        let true_dt = now.saturating_sub(self.frame_runtime.time).as_secs_f32();
        let raw_dt = true_dt.min(Self::MAX_DT);
        // EMA over instantaneous fps. First frame: raw_dt is `now`
        // (vs ZERO), giving an absurd reading; skip the update there.
        // Coefficient 0.1 ≈ ~10-frame window — smooth enough that
        // the readout doesn't jitter wildly, fast enough to track
        // real frame-rate drops.
        if self.frame_runtime.frame_id > 0 && true_dt > EPS {
            let inst = 1.0 / true_dt;
            self.frame_runtime.fps_ema = if self.frame_runtime.fps_ema == 0.0 {
                inst
            } else {
                self.frame_runtime.fps_ema * 0.9 + inst * 0.1
            };
        }
        self.frame_runtime.dt_accum += raw_dt;
        self.frame_runtime.dt = if self.frame_runtime.dt_accum >= ANIM_SUBSTEP_DT {
            let spent = self.frame_runtime.dt_accum;
            self.frame_runtime.dt_accum = 0.0;
            spent
        } else {
            0.0
        };
        self.frame_runtime.time = now;
        self.frame_runtime.frame_id += 1;
    }

    /// Drain wakes whose deadline has fired and decide whether this
    /// frame can take the anim-only fast path. Reads `repaint_requested`
    /// and one of the two input sticky bits (selected by
    /// [`Self::input_policy`]) from the prior frame's record; all must
    /// be observed BEFORE the per-frame clear that follows in
    /// `frame`.
    fn classify_frame(&mut self, display: Display) -> FramePlan {
        // `repaint_wakes` is sorted ascending, so fired = prefix slice.
        let fired_count = self
            .frame_runtime
            .repaint_wakes
            .partition_point(|w| w.deadline <= self.frame_runtime.time);
        let fired_reasons = self
            .frame_runtime
            .repaint_wakes
            .drain(..fired_count)
            .fold(WakeReasons::default(), |acc, w| acc.merge(w.reasons));

        let display_changed = self
            .frame_runtime
            .prev_stamp
            .is_some_and(|prev| !prev.display.raster_eq(&display));
        let frame_skipped = !self.frame_runtime.frame_submitted;
        let force_full = display_changed || frame_skipped;
        if force_full {
            tracing::debug!(
                display_changed,
                frame_skipped,
                first_frame = self.frame_runtime.prev_stamp.is_none(),
                "damage.invalidate_prev"
            );
        }

        let input_forces_record = match self.input_policy {
            InputPolicy::Always => self.input.had_input_since_last_frame,
            InputPolicy::OnDelta => self.input.repaint_requested_since_last_frame,
        };
        let paint_only = !force_full
            && self.frame_runtime.prev_stamp.is_some()
            && !self.frame_runtime.repaint_requested
            && !input_forces_record
            // An OS close request is surfaced to the app only during
            // record (`Ui::close_requested` + the `keep_open` veto), so
            // a wants_close frame must take the Full path — PaintOnly
            // would skip the record and the host would close the window
            // with the veto unconsulted (a spinner's every-frame ANIM
            // wake makes that deterministic, a caret blink a race).
            && !self.window_mailbox.wants_close
            && fired_reasons.is_anim_only();
        if paint_only {
            FramePlan::PaintOnly
        } else {
            FramePlan::FullRecord { force_full }
        }
    }

    /// One `pre_record` → user record → drain action flag → `post_record`
    /// cycle. Returns whether the cycle saw action input (which triggers
    /// a second pass in `Ui::frame`).
    fn record_pass(&mut self, record: &mut impl FnMut(&mut Ui)) -> bool {
        {
            profiling::scope!("Ui::pre_record");
            // Arena is per-record-pass storage: tree.shapes records
            // index into it (gradients / polyline points+colors /
            // meshes / interned text). Clear in lockstep with
            // `forest.pre_record` — both refill during user record.
            self.record_store.clear();
            self.forest.pre_record();
            // Subscription set is rebuilt from scratch each full record
            // pass — symmetric to `Sense` on a node. Widgets re-assert
            // during record; ones that didn't run drop their wake.
            // Across silent (PaintOnly / skipped) frames the set
            // persists, which is the whole point: a dormant popup
            // needs `BUTTONS` to still be set when the next click
            // outside lands.
            self.input.subs.clear();
            // Like the subscription set, the cursor request is
            // re-asserted by whoever still wants it this pass; reset
            // here (not per frame) so PaintOnly frames keep the last
            // recorded cursor instead of flickering back to the arrow.
            self.window_mailbox.cursor = CursorIcon::default();
            // Snapshot whether any widget interaction is possible this
            // frame; `response_for` skips its per-button capture scans for
            // every widget when none is (the common idle frame).
            self.input.snapshot_frame_quiescent();
        }
        // Synthetic viewport root for Layer::Main. Without this, the
        // first user-recorded node becomes the root and the layout
        // engine forces its rect to the surface — silently overriding
        // declared `Sizing` / `Sense` on the top-level widget. ZStack +
        // Fill matches the historical "root paints full surface"
        // behavior while letting user roots respect their own sizing.
        let mut viewport = Element::new(LayoutMode::ZStack);
        viewport.size = Sizing::FILL.into();
        // Hard-coded `WidgetId::VIEWPORT` — a frame-stable parent id,
        // so top-level salts/auto ids resolve to `VIEWPORT.with(salt)`
        // like any other parent-scoped id (see `widget_id`).
        self.forest.open_node(WidgetId::VIEWPORT, viewport, None);
        {
            profiling::scope!("Ui::record_user");
            record(self);
        }
        let action_flag = self.input.take_action_flag();
        if self.debug_overlay().frame_stats {
            record_frame_stats(self);
        }
        self.forest.close_node();
        self.post_record();
        action_flag
    }

    /// Shared inserter for [`Self::request_repaint_after`] (REAL) and
    /// paint-anim quantum boundaries (ANIM, filed from
    /// [`Self::post_record`]). Maintains the sorted-ascending
    /// invariant on [`FrameRuntime::repaint_wakes`], coalesces requests within
    /// one display-refresh interval onto the later deadline, and OR-merges
    /// reasons when two requests land on the same slot. Merging is
    /// what lets the frame-entry classifier see a wake that *both* an
    /// anim and a widget asked for as `REAL | ANIM`, which forces the
    /// Full path (correct — the widget needs record).
    fn schedule_wake(&mut self, deadline: Duration, reasons: WakeReasons) {
        let coalesce = coalesce_dt_for_refresh(self.display.refresh_millihertz);
        let near = |existing: Duration| existing.abs_diff(deadline) < coalesce;
        let wakes = &mut self.frame_runtime.repaint_wakes;
        let pos = wakes.partition_point(|w| w.deadline < deadline);
        // Coalesce to the later of (existing, requested) — collapse
        // bursts into a single wake at the back of the window to avoid
        // unnecessary host wakes. pos is at-or-later than deadline (keep
        // its deadline, merge our reasons in — `coalesce` is never zero,
        // so an exact match lands here too); pos-1 is earlier (overwrite
        // with ours, but keep merged reasons).
        if pos < wakes.len() && near(wakes[pos].deadline) {
            wakes[pos].reasons = wakes[pos].reasons.merge(reasons);
            return;
        }
        if pos > 0 && near(wakes[pos - 1].deadline) {
            wakes[pos - 1].deadline = deadline;
            wakes[pos - 1].reasons = wakes[pos - 1].reasons.merge(reasons);
            return;
        }
        wakes.insert(pos, Wake { deadline, reasons });
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
        self.forest.post_record();
        let payloads = self.record_store.borrow();
        let tc = TextCtx {
            bytes: &payloads.fmt_scratch,
            shaper: &self.ctx.shaper,
        };
        self.layout_engine.run(
            &self.forest,
            &tc,
            self.display.logical_rect(),
            &mut self.layout,
        );
        drop(payloads);
        // O5 stage 0: skip the cascade when nothing feeding it changed.
        // The cascade is a pure function of subtree authoring + arranged
        // rects, and the arranged rects are determined by (subtree_hash,
        // exact surface, scroll offset/zoom) — so a matching fingerprint
        // means identical cascade output, and last frame's
        // `Ui::cascades` can be reused verbatim (the tree is rebuilt
        // with identical structure when `subtree_hash` matches, so its
        // NodeId-indexed rows still line up).
        let fp = cascade_fingerprint(
            &self.forest,
            &self.layout_engine.scroll_states,
            self.display,
        );
        let skip = self.frame_runtime.prev_cascade_fp == Some(fp);
        #[cfg(test)]
        {
            self.frame_runtime.dbg_cascade_ran = !skip;
        }
        if skip {
            return;
        }
        self.frame_runtime.prev_cascade_fp = Some(fp);
        self.cascades_engine
            .run(&self.forest, &self.layout, &mut self.cascades);
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
        self.ctx.shaper.sweep_removed(removed);
        self.ctx.shaper.end_frame();
        self.layout_engine.sweep_removed(removed);
        self.state.sweep_removed(removed);
        self.anim.sweep_removed(removed);
        // Evict views whose widget vanished this frame; the backend frees the
        // orphaned texture the next frame it's no longer in `frame_targets`.
        // Guarded like the sweep_removed family — `retain` walks the whole
        // map even when nothing was removed.
        if !removed.is_empty() {
            self.gpu_views.retain(|wid, _| !removed.contains(wid));
        }

        self.input.end_frame(&self.cascades);
    }
}

/// Widget- and host-facing authoring API: input feed, subscriptions,
/// repaint/relayout requests, shape recording, per-widget state, and
/// animation. Distinct from the host-driven frame lifecycle above
/// (`frame` + its private record/cascade/finalize passes), which user
/// code never calls directly.
impl Ui {
    /// Feed an aperture-native input event. Returns an [`InputDelta`]
    /// the host reads to decide whether to request a redraw — pointer
    /// moves over inert surfaces leave `requests_repaint` false so the
    /// host can skip the frame entirely. Animation/tooltip-delay wakes
    /// still drive paints independently via `FrameReport::repaint_after`.
    pub fn on_input(&mut self, event: InputEvent) -> InputDelta {
        self.input.on_input(event, &self.cascades)
    }

    // Wake gates for off-target events. All three are idempotent and
    // cleared pre-record: widgets re-call each active frame, stop
    // calling to drop the wake. See `crate::input::subscriptions`.

    /// Declare interest in off-target pointer events of `flags`.
    pub fn subscribe_pointer(&mut self, flags: PointerSense) {
        self.input.subs.pointer_mask |= flags;
    }

    /// Declare interest in off-focus keyboard categories. Hotkey
    /// recorders, accel-underline UIs, command palettes that record
    /// before focus. Specific chords use [`Self::subscribe_key`].
    pub fn subscribe_keyboard(&mut self, flags: KeyboardSense) {
        self.input.subs.keyboard_mask |= flags;
    }

    /// Declare interest in one specific shortcut (e.g.
    /// `Shortcut::key(Key::Escape)`, `Shortcut::ctrl('K')`).
    /// Duplicate subscribers collapse.
    pub fn subscribe_key(&mut self, sc: Shortcut) {
        self.input.subs.subscribe_key(sc);
    }

    /// Unified pointer event stream captured this frame. Empty when
    /// no [`PointerSense`] subscriber is active. Subscribers `match`
    /// and filter by rect / button.
    pub fn pointer_events(&self) -> &[PointerEvent] {
        &self.input.frame_pointer_events
    }

    /// Unified keyboard event stream this frame —
    /// [`KeyboardEvent::Down`] from `KeyDown` events and
    /// [`KeyboardEvent::Text`] from typed/IME-committed text, in
    /// arrival order. Single buffer for both the focused widget and
    /// global [`KeyboardSense`] / [`Shortcut`] subscribers.
    pub fn keyboard_events(&self) -> &[KeyboardEvent] {
        &self.input.frame_keyboard_events
    }

    /// `true` if any [`KeyboardEvent::Down`] this frame matches
    /// `sc`. Iterates [`Self::keyboard_events`]; for repeat or
    /// stateful logic, iterate directly instead.
    ///
    /// Side-effect: auto-subscribes the chord for wake-up. Without
    /// this, aperture's keyboard wake-gate parks off-focus presses until the
    /// next unrelated
    /// frame, and the caller sees the event one user gesture late.
    /// Pair with the call-it-every-frame discipline that the
    /// subscription system already requires.
    pub fn key_pressed(&mut self, sc: Shortcut) -> bool {
        self.input.subs.subscribe_key(sc);
        self.input.frame_keyboard_events.iter().any(|e| match e {
            KeyboardEvent::Down(kp) => sc.matches(*kp),
            _ => false,
        })
    }

    /// Sugar for `key_pressed(Shortcut::key(Key::Escape))`.
    /// Used by [`crate::widgets::context_menu::ContextMenu`] to
    /// dismiss on Esc.
    pub fn escape_pressed(&mut self) -> bool {
        use crate::input::keyboard::Key;
        self.key_pressed(Shortcut::key(Key::Escape))
    }

    /// Re-record this frame after measure runs (for widgets that
    /// realize their record-time inputs were stale). Capped at one
    /// re-record per `run_frame`.
    pub fn request_relayout(&mut self) {
        self.frame_runtime.relayout_requested = true;
    }

    /// Monotonic time of the current frame, accumulated from the
    /// per-frame `dt`s the host feeds in. Starts at zero on the first
    /// frame and only moves forward. Read-only on purpose: the clock is
    /// host-driven, and a direct write would desync it from the wake
    /// queue. Use for time-driven animation that needs a continuous
    /// clock rather than a tween toward a fixed target; pair with
    /// [`Self::request_repaint`] to keep the host awake. (Shape-level
    /// continuous motion like `Spinner`'s rides `PaintAnim` instead —
    /// sampled at encode time, no record-time clock read.)
    pub fn now(&self) -> Duration {
        self.frame_runtime.time
    }

    /// Request the mouse cursor shown for this window. Per record pass,
    /// last writer wins — record order is z-order, so the topmost
    /// interested widget's request lands. Reset to
    /// [`CursorIcon::Default`] at the top of every record pass; a widget
    /// that still wants a non-default cursor re-requests it each frame
    /// (typically off its hover/drag response). The host applies it
    /// after the frame, only on change; ignored in headless contexts.
    pub fn set_cursor(&mut self, cursor: CursorIcon) {
        self.window_mailbox.cursor = cursor;
    }

    /// Ask the host to schedule another frame after this one. Cleared
    /// at the top of every `frame`; widgets/showcases that need
    /// continuous animation call this each frame to keep the host
    /// awake.
    #[track_caller]
    pub fn request_repaint(&mut self) {
        let caller = Location::caller();
        tracing::trace!(
            target: "aperture.repaint",
            "request_repaint @ {}:{} (frame={})",
            caller.file(),
            caller.line(),
            self.frame_runtime.frame_id,
        );
        self.frame_runtime.repaint_requested = true;
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
        let caller = Location::caller();
        tracing::trace!(
            target: "aperture.repaint",
            "request_repaint_after({:?}) @ {}:{} (frame={})",
            after,
            caller.file(),
            caller.line(),
            self.frame_runtime.frame_id,
        );
        let deadline = self.frame_runtime.time.saturating_add(after);
        self.schedule_wake(deadline, WakeReasons::REAL);
    }

    /// Open a new top-level OS window addressed by `token`. The window
    /// gets its own independent UI tree; [`App::update`](crate::App::update)
    /// and [`App::record`](crate::App::record) receive its `token`, and you can later poke it via
    /// [`HostHandle::request_repaint`](crate::HostHandle::request_repaint)
    /// or close it with [`Self::close_window`].
    ///
    /// Creation is deferred, not inline: the request is queued and the
    /// host (`WinitHost`) creates the real window on the event-loop
    /// thread right after this frame, so it's safe to call mid-record.
    /// Idempotent within a frame — record passes replay (cold-start
    /// warmup, double-layout pass B), so repeat calls for one `token`
    /// collapse to a single request with the last `config` winning. A
    /// `token` already in use by a live window is ignored with a
    /// warning. No-op in headless contexts (no host to drain the queue).
    ///
    /// `token` is yours to define — an enum discriminant, an index, a
    /// document-id hash. It must be unique across live windows. `config`
    /// is the backend-agnostic [`WindowConfig`] (title + size); the
    /// window inherits the app-global GPU settings from startup.
    pub fn open_window(&mut self, token: WindowToken, config: WindowConfig) {
        if let Some(p) = self
            .window_mailbox
            .pending_windows
            .iter_mut()
            .find(|p| p.token == token)
        {
            p.config = config;
            return;
        }
        self.window_mailbox
            .pending_windows
            .push(PendingWindow { token, config });
    }

    /// Request that the window addressed by `token` close. Deferred like
    /// [`Self::open_window`] — the host removes it after this frame. The
    /// last window closing exits the event loop. No-op if `token` names
    /// no live window, or in headless contexts.
    pub fn close_window(&mut self, token: WindowToken) {
        self.window_mailbox.pending_closes.push(token);
    }

    /// `true` for the single frame where the OS asked to close this window
    /// (titlebar X). The window auto-closes after this frame **unless** you
    /// call [`Self::keep_open`] — so a simple app needs no close handling
    /// at all (X just works), while an app that wants a "save changes?"
    /// prompt vetoes the auto-close and shows a dialog:
    ///
    /// ```ignore
    /// if ui.close_requested() && self.has_unsaved_changes() {
    ///     ui.keep_open();               // veto this frame's auto-close
    ///     self.show_quit_dialog = true; // remember to prompt
    /// }
    /// // …later, on the dialog's "Discard"/"Save" button:
    /// ui.close_window(win);             // close for real
    /// ```
    ///
    /// Always `false` in headless / offscreen contexts (no OS window).
    pub fn close_requested(&self) -> bool {
        self.window_mailbox.wants_close
    }

    /// Veto the auto-close pending from this frame's [`Self::close_requested`].
    /// The window stays open past this frame; close it for real later with
    /// [`Self::close_window`]. A no-op when no close was requested.
    pub fn keep_open(&mut self) {
        self.window_mailbox.close_vetoed = true;
    }

    /// This window's live geometry for persist-and-restore across launches.
    /// A computed view, not stored state: the logical inner size comes from
    /// [`Self::display`] (the single source of truth for surface size), and
    /// the physical outer position + maximized flag from the host-refreshed
    /// window-manager facts. Feed it back through
    /// [`WindowConfig::position`](crate::WindowConfig) / `inner_size` /
    /// `maximized` on the next launch to reopen where the user left off.
    /// `outer_position` is `None` on platforms that don't report it
    /// (Wayland). All-zero / `None` in headless contexts.
    pub fn window_geometry(&self) -> WindowGeometry {
        let logical = self.display.logical_size();
        WindowGeometry {
            inner_size: UVec2::new(
                (logical.w.round() as u32).max(1),
                (logical.h.round() as u32).max(1),
            ),
            outer_position: self.window_mailbox.position,
            maximized: self.window_mailbox.maximized,
        }
    }

    /// Mutable handle to this app's debug overlay; the guard derefs to
    /// `&mut DebugOverlayConfig`, so write fields straight on it
    /// (`ui.debug_overlay_mut().damage_rect = true`). The overlay is
    /// app-global: the write is visible to every window at once, and the
    /// host repaints idle windows so it shows everywhere — not just the
    /// window that handled the key. Drop the guard before other `Ui`
    /// calls; the `&mut self` borrow enforces that.
    pub fn debug_overlay_mut(&mut self) -> RefMut<'_, DebugOverlayConfig> {
        self.ctx.debug_overlay_mut()
    }

    /// This app's current debug overlay. Read by the backend at submit
    /// time and by `Ui::frame` to drive the FPS readout.
    pub(crate) fn debug_overlay(&self) -> DebugOverlayConfig {
        self.ctx.debug_overlay()
    }

    /// Whether a window addressed by `token` is currently live. Reflects
    /// the set as of this frame's *start*, so a window opened or closed
    /// earlier *this* frame isn't reflected until the next one (the host
    /// drains [`Self::open_window`] / [`Self::close_window`] between
    /// frames). Use it as the source of truth for "is this window up?"
    /// instead of mirroring the state in app code — a window the user
    /// closed via its titlebar drops out of this set automatically.
    pub fn window_open(&self, token: WindowToken) -> bool {
        self.ctx.window_open(token)
    }

    pub fn add_shape(&mut self, shape: Shape<'_>) {
        self.forest.add_shape(shape, &self.record_store);
    }

    /// Upload an image and get back an owning [`ImageHandle`]. **Hold the
    /// handle** to keep the GPU texture resident — dropping the last
    /// clone frees it; there is no `unregister`. Reference it in
    /// [`Shape::Image`] every frame (`clone` it where it needs to live).
    /// The CPU bytes are dropped right after the upload.
    pub fn register_image(&self, image: Image) -> ImageHandle {
        self.ctx.caches.images.register(image)
    }

    /// Record a `GpuView` for widget `id`: upsert it into [`Self::gpu_views`]
    /// — minting the stable backend `TextureId` once (on first sight) and
    /// refreshing the app `paint` callback each frame — then append a
    /// [`ShapeRecord::GpuView`] carrying the view's `epoch` to the active node
    /// (the encoder recovers id + paint from the map by `id`).
    ///
    /// `repaint` is the widget's per-frame dirty flag. When set, the epoch
    /// bumps to the current frame id, so the shape hash changes and the view
    /// repaints; when clear, the epoch is held stable, so the damage diff
    /// treats the view as unchanged and the encoder culls it (skipping its GPU
    /// paint and reusing last frame's pixels). First sight always paints (the
    /// texture doesn't exist yet). The entry rides the map's `removed` sweep
    /// when the widget disappears.
    pub(crate) fn gpu_view(
        &mut self,
        id: WidgetId,
        paint: Rc<RefCell<dyn GpuPaint>>,
        repaint: bool,
    ) {
        let frame_id = self.frame_runtime.frame_id;
        let entry = match self.gpu_views.entry(id) {
            Entry::Occupied(e) => {
                let entry = e.into_mut();
                entry.paint = GpuPaintRef(paint);
                // Bump only on a repaint request; held stable otherwise so a
                // static view stays undamaged (culled, its paint skipped).
                if repaint {
                    entry.epoch = frame_id;
                }
                entry
            }
            // First sight always paints — the texture doesn't exist yet.
            // The shared id source is disjoint from `self.gpu_views`.
            Entry::Vacant(e) => e.insert(GpuViewEntry {
                texture_id: self.ctx.caches.texture_ids.reserve(),
                paint: GpuPaintRef(paint),
                epoch: frame_id,
            }),
        };
        self.forest.add_gpu_view(entry.epoch);
    }

    /// Format `args` directly into the record-pass text storage and return
    /// a frame-local [`InternedStr`]. Pass the returned value to
    /// any widget that takes `impl Into<InternedStr>`
    /// (Text/Button/MenuItem) — the bytes are already in the destination
    /// buffer, so lowering is zero-copy and steady-state authoring of
    /// dynamic labels skips per-call `String` allocations.
    ///
    /// **Record-pass-scoped.** The next record pass invalidates the
    /// handle, including a settling pass inside the same [`Self::frame`]
    /// call. Reuse then panics during text lowering. PaintOnly starts no
    /// record pass, so its retained tree and handles remain valid. For
    /// persistent strings store the original `String` / `&'static str`
    /// and convert it into `InternedStr` on every pass.
    #[must_use]
    pub fn fmt(&mut self, args: std::fmt::Arguments<'_>) -> InternedStr {
        self.record_store.intern_fmt(args)
    }

    /// Copy `s` into the record-pass text storage and return a frame-local
    /// [`InternedStr`]. Format-less twin of
    /// [`Self::fmt`] for plain `&str` borrows whose lifetime doesn't
    /// reach `'static` — turns a per-frame `String` allocation into a
    /// memcpy into the retained `fmt_scratch` buffer. Same
    /// frame-scoped invalidation rules as [`Self::fmt`].
    #[must_use]
    pub fn intern(&mut self, s: &str) -> InternedStr {
        self.record_store.intern_str(s)
    }

    /// Append `shape` to the active node and register `anim` against
    /// it. The encoder samples `anim` at paint time and folds the
    /// resulting `PaintMod` into the shape's brush; `post_record`
    /// folds the anim's `next_wake` into `repaint_wakes` so the
    /// caller doesn't manage scheduling. Drops silently if the shape
    /// itself was noop-collapsed (zero stroke + transparent fill,
    /// etc.) — `PaintAnim` can't make a zero shape paintable.
    pub(crate) fn add_shape_animated(&mut self, shape: Shape<'_>, anim: PaintAnim) {
        self.forest
            .add_shape_animated(shape, anim, &self.record_store);
    }

    /// Record `body` as a side layer placed at `anchor` (top-left
    /// position). `size = None` makes the body's "available" extend
    /// from `anchor` to the surface bottom-right; `size = Some(s)`
    /// caps it at `s`, still clamped to the surface so an oversized
    /// cap can't bleed past the viewport. The root's own `Sizing`
    /// (Hug/Fill/Fixed) then governs the painted size within that
    /// available. Recordable from the `Main` baseline or nested inside a
    /// higher-ranked side layer's body (a tooltip raised from a popup or
    /// modal). The nested layer must sit strictly above the current scope
    /// in `Layer::PAINT_ORDER`, else it would paint under its parent.
    pub fn layer(
        &mut self,
        layer: Layer,
        anchor: glam::Vec2,
        size: Option<Size>,
        body: impl FnOnce(&mut Ui),
    ) {
        self.forest.push_layer(layer, anchor, size);
        body(self);
        self.forest.pop_layer();
    }

    /// Resolve `element`'s stable [`WidgetId`] for this frame — the id the
    /// matching [`Self::node`] call records into the tree. This is the
    /// public entry a widget author calls first: resolve once, read
    /// [`Self::response_for`] / per-widget [`Self::state_mut`] against the
    /// returned id (theme picking off the prior frame, animation slots,
    /// sub-id derivation), then hand the *same* id to [`Self::node`]. Every
    /// built-in widget follows this resolve-once-then-`node` shape.
    ///
    /// The egui `make_persistent_id` analogue: an [`crate::Configure::id_salt`]
    /// salt *and* a `#[track_caller]` auto id both resolve to
    /// `parent.with(id)` (so identity tracks tree position, not global
    /// record order, keeping per-site state stable across frames and
    /// sibling reorders); only an explicit `.id(id)` resolves verbatim.
    /// Parent context is the most-recently-opened node in the current layer
    /// — `Layer::Main`'s synthetic viewport counts as a parent with a
    /// frame-stable id, so widgets get stable ids with no layer carve-out.
    ///
    /// **Eagerly disambiguates** via `SeenIds`: a salt colliding with a
    /// sibling already recorded this frame is bumped to a fresh occurrence
    /// slot, so the returned id matches what the tree, cascade, and
    /// `response_for` will see.
    ///
    /// **Contract**: follow with exactly one [`Self::node`] opening a node
    /// with this id — the `SeenIds` slot reserved here pairs with the next
    /// opened node, so resolving twice without an intervening `node` drifts
    /// the occurrence counter. Child nodes built with an explicit
    /// `.id(parent.with("x"))` carry a verbatim id and may pass it straight
    /// to `node` without a second resolve.
    #[inline]
    pub fn widget_id(&mut self, element: &Element) -> WidgetId {
        let salt = element.salt;
        let raw_id = salt.resolve(self.forest.current_parent_id());
        self.forest.ids.resolve(raw_id, salt.is_explicit())
    }

    /// Open a node, optionally with paint chrome, run its body, and
    /// close it. `chrome` is `None` for the common layout-only / text-
    /// leaf / chrome-less path and `Some(bg)` when the widget paints a
    /// background — container widgets resolve an explicit-or-theme
    /// `Option<Background>` and pass `chrome.as_ref()`. Taken as
    /// `Option<&Background>` (an 8-byte niche-encoded pointer, not the
    /// 168 B `Background` by value) so the chrome travels as one pointer
    /// per hop down `Forest::open_node` → `Tree::open_node` →
    /// `shapes::lower::background`, and the no-chrome path is just a
    /// perfectly-predicted `None` branch.
    ///
    /// `id` must be the [`Self::widget_id`] resolution of `element.salt`
    /// (or, for a child built with an explicit `.id(parent.with("x"))`,
    /// that verbatim id). Disambiguation already happened there, so this
    /// is the final id verbatim — no further `SeenIds` work here.
    ///
    /// Public so library users can author their own widgets — see
    /// `examples/custom_widget.rs`.
    pub fn node<R>(
        &mut self,
        id: WidgetId,
        element: Element,
        chrome: Option<&Background>,
        f: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        let chrome = chrome.map(|bg| ChromeInput {
            bg,
            store: &self.record_store,
        });
        self.forest.open_node(id, element, chrome);
        let r = f(self);
        self.forest.close_node();
        r
    }

    /// Snapshot of input/cascade state for a widget. `rect` and
    /// `disabled` are from the previous frame's cascade; the interaction
    /// fields (`pressed`, `hovered`, `drag_started`, `drag_delta`, …) are
    /// computed against this frame's input state.
    ///
    /// **Read it during the frame's record** — as every widget does. The
    /// interaction half is gated on a `frame_quiescent` snapshot taken
    /// once at record-pass start, so a read taken *between* frames would
    /// reflect the previous frame's input, not events fed since. Reading
    /// earlier in the same record than the widget's own node is fine —
    /// e.g. baking a drag delta into a widget's position before recording it.
    pub fn response_for(&self, id: WidgetId) -> ResponseState {
        let mut state = self.input.response_for(id, &self.cascades);
        // Cascade lags one frame; OR this frame's ancestor-disabled so
        // a freshly-disabled subtree paints disabled on its first frame.
        state.disabled |= self.forest.current_scratch().ancestor_disabled();
        // The interaction half was routed against the stale cascade, so
        // without this a freshly-disabled widget reports hovered /
        // clicked alongside disabled for one frame — a combination the
        // steady-state hit index can never produce (disabled entries
        // carry `Sense::NONE`), and one that lets a click land on
        // just-disabled UI.
        if state.disabled {
            state = ResponseState {
                rect: state.rect,
                layout_rect: state.layout_rect,
                transform: state.transform,
                disabled: true,
                focused: state.focused,
                ..ResponseState::default()
            };
        }
        state
    }

    /// Cross-frame state row for `id`, `T::default()` on first
    /// access. Rows for `WidgetId`s not recorded this frame are
    /// evicted in `finalize_frame`, once per `Ui::frame` after the
    /// final record pass. Type collisions at one `id` are NOT
    /// detected — each `T` lives in its own store, so two call sites
    /// using different types at the same id silently coexist (see the
    /// `state` module doc).
    pub fn state_mut<S: Default + 'static>(&mut self, id: WidgetId) -> &mut S {
        self.state.get_or_insert_with(id, S::default)
    }

    /// Read-only peek at the cross-frame state row for `id`. `None` if
    /// nothing has been stored for `(id, T)` yet — does not allocate or
    /// mutate. Use this on the `&Ui` side (probes, hit-test helpers,
    /// "is this menu open?" checks) where `state_mut`'s `&mut Ui`
    /// receiver would be a needless borrow upgrade.
    pub fn try_state<S: 'static>(&self, id: WidgetId) -> Option<&S> {
        self.state.try_get::<S>(id)
    }

    /// Advance an animation row keyed by `(id, slot)` and return the
    /// current value. `spec = None` snaps to `target` and drops any
    /// stale row without requesting a repaint — the canonical
    /// "no animation" path. See `src/animation/animations.md`.
    // Generic and reached through cross-module widget helpers. Keep the
    // dominant no-map/no-spec return in the widget's block so a static theme
    // doesn't pay an outlined call plus a large `V` return-slot handoff.
    #[inline(always)]
    pub fn animate<V: Animatable>(
        &mut self,
        id: WidgetId,
        slot: impl Into<AnimSlot>,
        target: V,
        spec: Option<AnimSpec>,
    ) -> V {
        // Hottest path: no spec, no typed map for `V` ever allocated.
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
            if let Some(typed) = self.anim.try_typed_mut::<V>() {
                typed.rows.remove(&(id, slot));
            }
            return target;
        };
        let r = self.anim.typed_mut::<V>().tick(
            id,
            slot,
            target,
            spec,
            self.frame_runtime.dt,
            self.frame_runtime.frame_id,
        );
        if !r.settled {
            self.frame_runtime.repaint_requested = true;
        }
        r.current
    }

    /// Currently focused widget id, or `None`.
    pub fn focused_id(&self) -> Option<WidgetId> {
        self.input.focused
    }

    /// True when keyboard focus sits on `ancestor` or any widget
    /// recorded inside its subtree — per the most recent cascade run,
    /// i.e. one frame of lag, the same timing as [`Self::response_for`].
    /// `false` when nothing is focused or `ancestor` wasn't recorded.
    /// Layers are separate trees, so focus on a popup never counts as
    /// within the popup's anchor. Lets a caller that skips recording
    /// off-screen subtrees keep the one holding an in-progress edit
    /// alive without enumerating every focusable widget it contains.
    pub fn focus_within(&self, ancestor: WidgetId) -> bool {
        self.input
            .focused
            .is_some_and(|f| self.cascades.is_within(f, ancestor))
    }

    /// True when the pointer's hover target is `ancestor` or any widget
    /// recorded inside its subtree — the hover sibling of
    /// [`Self::focus_within`], same cascade timing and layer caveats.
    /// Prefer this over testing `Self::pointer_pos` against a rect for
    /// "is the pointer on me" styling: it's occlusion-aware (a panel
    /// stacked on top wins the pointer), and because it's a pure
    /// function of the hover *target*, its value can only change when
    /// the target changes — which is exactly when a repaint is already
    /// scheduled, so no `MOVE` subscription is needed to stay fresh.
    pub fn hover_within(&self, ancestor: WidgetId) -> bool {
        self.input
            .hovered
            .is_some_and(|h| self.cascades.is_within(h, ancestor))
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
    ///
    /// `&mut` because reading it auto-asserts a [`PointerSense::MOVE`]
    /// subscription: output derived from the raw pointer may change on
    /// any move, so moves must keep triggering repaints even when the
    /// hover target doesn't change — otherwise pointer-derived paint
    /// (e.g. a proximity highlight) goes stale on screen until an
    /// unrelated event forces a frame. Like every subscription, it
    /// lapses as soon as a record pass stops reading. Note the same
    /// hazard exists for `ResponseState::pointer_local`, which can't
    /// observe reads — paint derived from it should read this getter
    /// instead.
    pub fn pointer_pos(&mut self) -> Option<glam::Vec2> {
        self.subscribe_pointer(PointerSense::MOVE);
        self.input.pointer_pos
    }

    /// Currently-held modifier keys. State persists across frames —
    /// only `ModifiersChanged` events mutate it. Read at the start of
    /// a drag/click to gate behavior (Cmd+LMB shortcuts, etc.).
    pub fn modifiers(&self) -> Modifiers {
        self.input.modifiers
    }

    pub fn focus_policy(&self) -> FocusPolicy {
        self.input.focus_policy
    }

    /// Set the press-on-non-focusable behavior. See [`FocusPolicy`].
    pub fn set_focus_policy(&mut self, p: FocusPolicy) {
        self.input.focus_policy = p;
    }
}

/// Central gated reach-in surface for tests and benches. All test/bench
/// helpers that operate on `Ui` live here as methods on `Ui` (or as
/// free constructors), so a single `use crate::ui::test_support::*;`
/// brings in the entire surface. Items that don't touch `Ui` (e.g.
/// `TextShaper::*`) stay in their own modules.
#[cfg(any(test, feature = "internals"))]
pub(crate) mod test_support {
    #![allow(dead_code)]
    use crate::animation::animatable::Animatable;
    use crate::forest::layer::Layer;
    use crate::forest::tree::node::NodeId;
    use crate::input::InputEvent;
    use crate::input::pointer::PointerButton;
    use crate::layout::scroll::ScrollLayoutState;
    use crate::primitives::rect::Rect;
    use crate::renderer::frontend::cmd_buffer::RenderCmdBuffer;
    use crate::renderer::frontend::encoder::encode;
    use crate::text::TextShaper;
    use crate::ui::damage::Damage;
    use crate::ui::damage::region::DamageRegion;
    use crate::ui::frame::FrameStamp;
    use crate::ui::frame_report::{RenderKind, RenderPlan};
    use crate::ui::*;
    use glam::{UVec2, Vec2};
    use std::time::Duration;

    /// Standalone `Ui` with a mono-fallback shaper and private record store.
    /// Shipping code receives its `Ui` from a host and must not construct a
    /// disconnected one.
    impl Default for Ui {
        fn default() -> Self {
            Self::new(&HostContext::default(), RecordStore::default())
        }
    }

    impl Ui {
        /// `Layer::Main` node whose `widget_id` matches `id`. Panics if absent.
        pub(crate) fn node_for_widget_id(&self, id: WidgetId) -> NodeId {
            let tree = &self.forest.trees[Layer::Main];
            let idx = tree
                .records
                .widget_id()
                .iter()
                .position(|w| *w == id)
                .unwrap_or_else(|| panic!("no node found for widget_id {id:?}"));
            NodeId(idx as u32)
        }
    }

    impl Ui {
        /// `Ui` with the mono-fallback shaper — predictable 8 px/char
        /// widths. Pre-marked as warm: see [`Self::mark_warm_for_test`].
        pub(crate) fn for_test() -> Self {
            let mut ui = Self::default();
            ui.mark_warm_for_test();
            ui
        }

        /// `Ui` with a thread-shared cosmic shaper (font DB built once
        /// per thread). Pre-marked as warm: see
        /// [`Self::mark_warm_for_test`].
        pub fn for_test_text() -> Self {
            thread_local! {
                static SHARED: TextShaper = TextShaper::with_bundled_fonts();
            }
            let ctx = HostContext::new(SHARED.with(|c| c.clone()));
            let mut ui = Self::new(&ctx, RecordStore::default());
            ui.mark_warm_for_test();
            ui
        }

        /// `Ui` pre-stamped with display dimensions; no user frame
        /// driven yet. Pre-marked as warm.
        pub(crate) fn for_test_at(size: UVec2) -> Self {
            let mut ui = Self {
                display: Display::from_physical(size, 1.0),
                ..Self::default()
            };
            ui.mark_warm_for_test();
            ui
        }

        /// `Ui` with cosmic shaper, pre-stamped with display dimensions.
        pub(crate) fn for_test_at_text(size: UVec2) -> Self {
            let mut ui = Self::for_test_text();
            ui.display = Display::from_physical(size, 1.0);
            ui.mark_warm_for_test();
            ui
        }

        /// Synthesize a "previous submitted frame" sentinel so the
        /// cold-start warmup record pass in `Ui::frame` doesn't
        /// fire on the first user `run_at`. The warmup runs the user
        /// closure twice (blackout pass + real pass); most tests assert
        /// against one record pass worth of work, so they want the
        /// warm-state behavior. Tests that need to exercise true
        /// cold-start behavior should construct `Ui::default()`
        /// directly and skip this. No real frame is run here —
        /// `frame_id`, `time`, the per-`StateMap`, cascades, and damage
        /// snapshot all stay at fresh-construction defaults. First
        /// user-frame damage depends on the constructor: `for_test` /
        /// `for_test_text` keep the default 0×0 display, so the first
        /// `run_at` is a display change ⇒ `Damage::Full`; `for_test_at`
        /// / `for_test_at_text` pre-stamp a matching display, so the
        /// first frame classifies by coverage like any other (small
        /// content ⇒ `Partial`, from the all-Vacant walk).
        pub(crate) fn mark_warm_for_test(&mut self) {
            self.frame_runtime.prev_stamp = Some(FrameStamp::new(self.display, Duration::ZERO));
            self.frame_runtime.frame_submitted = true;
        }

        /// One frame at `size`, time frozen at zero.
        pub(crate) fn run_at(&mut self, size: UVec2, record: impl FnMut(&mut Ui)) {
            let display = Display::from_physical(size, 1.0);
            self.frame(FrameStamp::new(display, Duration::ZERO), record);
        }

        /// `run_at` then mark the frame as submitted (suppress next-frame auto-rewind to `Full`).
        pub(crate) fn run_at_acked(&mut self, size: UVec2, record: impl FnMut(&mut Ui)) {
            self.run_at(size, record);
            self.frame_runtime.frame_submitted = true;
        }

        /// Ack the just-run frame as presented — mirrors what the host
        /// does after a successful submit, so the next [`Self::frame`]
        /// doesn't auto-escalate to `Full`. For tests and for benches
        /// that drive `frame` + a standalone
        /// [`crate::renderer::frontend::Frontend::build_for_test`]
        /// instead of going through `WindowRenderer` (the `frame/*_cpu` arms).
        pub(crate) fn mark_frame_submitted(&mut self) {
            self.frame_runtime.frame_submitted = true;
        }

        /// Wrap UUT inside a Fill HStack so the panel can express its own measured size.
        pub(crate) fn under_outer<F: FnMut(&mut Ui) -> NodeId>(
            &mut self,
            surface: UVec2,
            mut f: F,
        ) -> NodeId {
            use crate::forest::element::Configure;
            use crate::layout::types::sizing::Sizing;
            use crate::widgets::panel::Panel;
            let mut inner = None;
            self.run_at(surface, |ui| {
                Panel::hstack()
                    .auto_id()
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        inner = Some(f(ui));
                    });
            });
            inner.unwrap()
        }

        pub(crate) fn click_at(&mut self, pos: Vec2) {
            self.on_input(InputEvent::PointerMoved(pos));
            self.on_input(InputEvent::PointerPressed(PointerButton::Left));
            self.on_input(InputEvent::PointerReleased(PointerButton::Left));
        }

        pub(crate) fn press_at(&mut self, pos: Vec2) {
            self.on_input(InputEvent::PointerMoved(pos));
            self.on_input(InputEvent::PointerPressed(PointerButton::Left));
        }

        pub(crate) fn release_left(&mut self) {
            self.on_input(InputEvent::PointerReleased(PointerButton::Left));
        }

        pub(crate) fn secondary_click_at(&mut self, pos: Vec2) {
            self.on_input(InputEvent::PointerMoved(pos));
            self.on_input(InputEvent::PointerPressed(PointerButton::Right));
            self.on_input(InputEvent::PointerReleased(PointerButton::Right));
        }

        /// Drop every measure-cache entry, forcing full re-measure next frame.
        pub(crate) fn clear_measure_cache(&mut self) {
            self.layout_engine.cache.clear();
        }

        /// Scroll-state row for `id` (inserting default if absent).
        pub(crate) fn scroll_state(&mut self, id: WidgetId) -> &mut ScrollLayoutState {
            self.layout_engine.scroll_states.entry(id).or_default()
        }

        /// Run only the cascade pass against the just-finished frame.
        pub(crate) fn run_cascades(&mut self) {
            self.cascades_engine
                .run(&self.forest, &self.layout, &mut self.cascades);
        }

        /// Rebuild the post-collapse damage region from `DamageEngine`'s
        /// last-frame pass-1 buffer. Doesn't mutate state.
        pub(crate) fn damage_region(&self) -> DamageRegion {
            DamageRegion::collapse_from(
                &self.damage_engine.raw_rects,
                self.damage_engine.budget_px,
                self.display.logical_rect(),
            )
        }

        /// Damage rects produced by the most recent `post_record`.
        pub(crate) fn damage_rect_count(&self) -> usize {
            self.damage_region().iter_rects().count()
        }

        /// Subtree-skip jumps the last damage diff performed.
        pub(crate) fn damage_subtree_skips(&self) -> u32 {
            self.damage_engine.subtree_skips
        }

        /// Live entries in the `paint_snaps` arena (sum of every
        /// `NodeSnapshot::paint_span.len`, including orphaned tail).
        pub(crate) fn damage_shape_snaps_len(&self) -> usize {
            self.damage_engine.arena.len()
        }

        /// Count of orphaned `Paint` entries in the arena —
        /// drives the compaction trigger.
        pub(crate) fn damage_shape_snaps_orphaned(&self) -> u32 {
            self.damage_engine.arena.orphaned()
        }

        /// Times `compact_shape_snaps` has run on this engine.
        /// Used by benches to verify the compaction path was actually
        /// exercised and to count compactions over a measurement
        /// window.
        pub(crate) fn damage_compactions_run(&self) -> u32 {
            self.damage_engine.arena.compactions_run()
        }

        /// `"skip"` / `"partial"` / `"full"` — the frame's final paint decision.
        pub(crate) fn damage_paint_kind(&self) -> &'static str {
            match Damage::new(self.damage_region()) {
                Damage::Skip => "skip",
                Damage::Full => "full",
                Damage::Partial(_) => "partial",
            }
        }

        /// Animation rows currently allocated for `T`, or 0 if no typed map exists.
        pub(crate) fn anim_row_count<T: Animatable>(&mut self) -> usize {
            self.anim.try_typed_mut::<T>().map_or(0, |t| t.rows.len())
        }

        pub(crate) fn encode_cmds(&self) -> RenderCmdBuffer {
            self.encode_cmds_filtered(None)
        }

        pub(crate) fn encode_cmds_filtered(&self, filter: Option<Rect>) -> RenderCmdBuffer {
            self.encode_cmds_with_region(filter.map(DamageRegion::from))
        }

        /// Multi-rect variant; each rect is fed through `DamageRegion::add` so merge policy applies.
        pub(crate) fn encode_cmds_with_rects(&self, rects: &[Rect]) -> RenderCmdBuffer {
            let region = (!rects.is_empty()).then(|| DamageRegion::from_rects(rects));
            self.encode_cmds_with_region(region)
        }

        fn encode_cmds_with_region(&self, region: Option<DamageRegion>) -> RenderCmdBuffer {
            let clear = self.theme.window_clear;
            let kind = match region {
                Some(region) => RenderKind::Partial { region },
                None => RenderKind::Full,
            };
            let plan = RenderPlan { clear, kind };
            let mut cmds = RenderCmdBuffer::default();
            let payloads = self.record_store.borrow();
            encode(self, &payloads, plan, &mut cmds);
            cmds
        }
    }
}

#[cfg(test)]
mod tests;
