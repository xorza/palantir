pub(crate) mod cascade;
pub mod damage;
pub mod frame_report;
pub(crate) mod frame_state;
pub(crate) mod state;

use crate::InternedStr;
use crate::animation::animatable::Animatable;
use crate::animation::{AnimMap, AnimSlot, AnimSpec};
use crate::common::hash::Hasher;
use crate::common::time::{ANIM_SUBSTEP_DT, coalesce_dt_for_refresh};
use crate::debug_overlay::DebugOverlayConfig;
use crate::forest::Chrome;
use crate::forest::Forest;
use crate::forest::Layer;
use crate::forest::element::{Element, LayoutMode, Salt};
use crate::forest::tree::paint_anims::PaintAnim;
use crate::host_shared::HostShared;
use crate::input::keyboard::{KeyboardEvent, Modifiers};
use crate::input::pointer::PointerEvent;
use crate::input::policy::InputPolicy;
use crate::input::shortcut::Shortcut;
use crate::input::subscriptions::{KeyboardSense, PointerSense};
use crate::input::{FocusPolicy, InputDelta, InputEvent, InputState, ResponseState};
use crate::layout::Layout;
use crate::layout::layoutengine::LayoutEngine;
use crate::layout::support::TextCtx;
use crate::layout::types::display::Display;
use crate::layout::types::sizing::Sizing;
use crate::primitives::approx::EPS;
use crate::primitives::background::Background;
use crate::primitives::image::Image;
use crate::primitives::size::Size;
use crate::renderer::context::RenderContext;
use crate::renderer::image_registry::ImageHandle;

use crate::debug_overlay::record_frame_stats;
use crate::primitives::widget_id::WidgetId;
use crate::shape::Shape;
use crate::ui::cascade::{Cascades, CascadesEngine};
use crate::ui::damage::{Damage, DamageEngine, DamageInput};
use crate::ui::frame_report::{FrameProcessing, FrameReport, RenderPlan};
use crate::ui::frame_state::FrameState;
use crate::ui::state::StateMap;
use crate::widgets::theme::Theme;
use crate::window::{PendingWindow, WindowConfig, WindowToken};
use std::cell::RefMut;
use std::time::Duration;

/// Bitset over wake causes. OR-merged when two requests coalesce
/// onto the same deadline slot, so the frame-entry classifier can see
/// every reason behind a fired wake — used to pick `Full` vs
/// `AnimOnly` processing in [`Ui::frame`]. Bit set, not enum, because
/// a single deadline can legitimately have both bits at once
/// (paint-anim quantum aligning with a widget-scheduled wake).
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) struct WakeReasons(u8);

impl WakeReasons {
    /// Caller asked for a wake via [`Ui::request_repaint_after`] —
    /// state-spring tick, host-driven schedule, widget that owes a
    /// future paint. Requires a full record + measure + arrange +
    /// cascade pass.
    pub(crate) const REAL: Self = Self(1 << 0);
    /// Paint-anim quantum boundary, filed in [`Ui::post_record`] from
    /// `Forest::post_record`'s `min_wake`. On its own, only needs a
    /// damage compute + paint — record/post-record output from the
    /// prior frame is reused as-is.
    pub(crate) const ANIM: Self = Self(1 << 1);

    #[inline]
    pub(crate) fn merge(self, r: Self) -> Self {
        Self(self.0 | r.0)
    }

    /// `true` when the only reason set is `ANIM` — the predicate that
    /// gates [`FrameProcessing::PaintOnly`].
    #[inline]
    pub(crate) fn is_anim_only(self) -> bool {
        self == Self::ANIM
    }
}

/// WindowRenderer-supplied per-frame inputs — monotonic time + active
/// [`Display`]. Single struct so callers pass one argument and
/// `Ui` carries one `Option<FrameStamp>` for prior-frame state
/// instead of two parallel fields. `time` is the host's monotonic
/// clock (driven by the same source between frames); `display`
/// carries the surface size + scale factor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameStamp {
    pub display: Display,
    pub time: Duration,
}

impl FrameStamp {
    pub fn new(display: Display, time: Duration) -> Self {
        Self { display, time }
    }
}

/// One entry on [`Ui::repaint_wakes`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct Wake {
    pub(crate) deadline: Duration,
    pub(crate) reasons: WakeReasons,
}

/// What [`Ui::frame`] should do this frame, decided at entry
/// from fired wake reasons + input state + prior-frame validity.
/// `PaintOnly` and `FullRecord` are mutually exclusive by construction
/// — `paint_only ⇒ !force_full` is encoded in the variant shape
/// instead of relying on two independent bools.
#[derive(Clone, Copy, Debug)]
enum FramePlan {
    /// Skip pre_record / record / finalize / layout / cascade and
    /// reuse the retained tree + cascades from the prior frame. Only
    /// fired by the anim-only fast path.
    PaintOnly,
    /// Run record + (optional) double-layout + finalize. `force_full`
    /// is true when the prior frame's damage snapshot must be
    /// discarded (surface change, missed submit, first frame).
    FullRecord { force_full: bool },
}

/// Recorder + input/response broker. All public coordinates are
/// logical pixels (DIPs); `Display::scale_factor` converts to
/// physical at the wgpu boundary. See `docs/repaint.md` for the
/// frame-lifecycle rationale.
///
/// `Default` builds a self-contained `Ui` with mono-fallback shaper
/// and a private frame arena. Hosts that need to share the shaper /
/// arena with the wgpu backend use [`Self::new`] instead.
pub struct Ui {
    pub(crate) forest: Forest,
    pub theme: Theme,
    /// App-global host state shared by every window: the live-window set
    /// (read by [`Self::window_open`]) and the debug overlay (read by the
    /// backend at submit time and by `Ui::frame` for the FPS readout,
    /// toggled by [`Self::debug_overlay_mut`]). A cheap clone of the
    /// host's handle, wired in at construction; in headless contexts it's
    /// a private cell with no host writing to it.
    pub(crate) host: HostShared,
    /// Cross-frame widget state: per-type dense stores keyed by
    /// `WidgetId` (see [`StateMap`]).
    pub(crate) state: StateMap,
    /// Shared, GPU-agnostic render resources cloned from the host's
    /// [`RenderContext`] at construction: the font/glyph shaper
    /// (`ctx.shaper`), the per-frame arena (`ctx.frame_arena`), the GPU
    /// resource caches (`ctx.caches`), and the GPU-stats handle
    /// (`ctx.pass_stats`). Held as one bag so a new shared handle doesn't
    /// touch every `Ui` constructor; the wgpu backend clones the same
    /// context, so both see one set. A standalone `Ui::default()` builds a
    /// fresh private context that no backend writes to.
    pub(crate) ctx: RenderContext,
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
    /// Effective per-frame dt fed into the animation integrators
    /// (`AnimMapTyped::tick` / `spring::step`). Real wall-clock dt is
    /// accumulated into [`Self::dt_accum`] and only spent here once
    /// it crosses [`ANIM_SUBSTEP_DT`] — frames that don't spend
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
    /// WindowRenderer-supplied monotonic timestamp for this frame.
    pub(crate) time: Duration,
    /// Time + display from the previous successful frame, or `None`
    /// before the first frame and after
    /// [`DamageEngine::invalidate_prev`] rewinds the snapshot.
    /// Drives `classify_frame` (surface-change detection)
    /// and the paint-anim damage gate
    /// (`anim.next_wake(prev.time) <= now`). Updated at the bottom
    /// of `frame` on every path.
    pub(crate) prev_stamp: Option<FrameStamp>,
    /// Fingerprint of last frame's cascade inputs (all roots'
    /// `subtree_hash` + exact surface + scroll offsets/zoom). When this
    /// frame's fingerprint matches, the cascade output is provably
    /// identical, so `post_record` skips `CascadesEngine::run` and reuses
    /// last frame's `Ui::cascades` (O5 stage 0 — full-frame skip).
    /// `None` before the first cascade run.
    pub(crate) prev_cascade_fp: Option<u64>,
    /// Test-only: did the most recent `post_record` actually run the
    /// cascade, or skip it via [`Self::prev_cascade_fp`]? Pins the O5
    /// stage-0 skip gate (fires on an unchanged frame, not on a change).
    #[cfg(test)]
    pub(crate) dbg_cascade_ran: bool,
    /// EMA of `1/raw_dt` across frames. Zero on the first frame
    /// (no prior `time` to diff against); updated in
    /// [`Self::frame`]. Surfaced by the `frame_stats` debug overlay.
    pub(crate) fps_ema: f32,
    /// Set by [`Self::animate`] when an animation hasn't settled.
    pub(crate) repaint_requested: bool,
    /// Pending wake-up entries (absolute Ui-time, sorted ascending,
    /// dedup'd). Each carries the OR'd set of [`WakeReasons`] that
    /// asked for this deadline — when two requests coalesce into one
    /// slot, their reasons merge so the frame-entry classifier sees
    /// every cause that fired. Survive across frames; [`Self::frame`]
    /// drains the prefix that has fired and reads `fired_reasons` to
    /// pick the [`FrameProcessing`] path. Hosts read the earliest
    /// pending entry off [`FrameReport::repaint_after`] and pair with
    /// `winit::ControlFlow::WaitUntil` (or equivalent).
    pub(crate) repaint_wakes: Vec<Wake>,
    pub(crate) anim: AnimMap,
    /// Submission status of the last *painted* frame. NOT reset in
    /// `pre_record` — `click_on_empty_bg_does_not_force_full`
    /// pins why.
    pub(crate) frame_state: FrameState,
    /// Set by [`Self::request_relayout`]; consumed by
    /// `post_record` to trigger one re-record per
    /// `run_frame`.
    relayout_requested: bool,
    /// Window-open requests filed by [`Self::open_window`] during a
    /// frame. `WinitHost` drains this in `about_to_wait` (where it holds
    /// `&ActiveEventLoop`) and creates the windows synchronously. Not
    /// cleared by `frame` — it persists across the frame boundary until
    /// the host drains it. Retained Vec, capacity reused, so steady
    /// state (no window churn) is alloc-free. Inert in headless contexts
    /// with no `WinitHost` to drain it.
    pub(crate) pending_windows: Vec<PendingWindow>,
    /// Window-close requests filed by [`Self::close_window`]; drained
    /// alongside [`Self::pending_windows`]. Same retained-Vec contract.
    pub(crate) pending_closes: Vec<WindowToken>,
}

impl Default for Ui {
    fn default() -> Self {
        Self {
            forest: Default::default(),
            theme: Default::default(),
            host: Default::default(),
            state: Default::default(),
            ctx: Default::default(),
            layout_engine: Default::default(),
            layout: Default::default(),
            cascades: Default::default(),
            input: Default::default(),
            input_policy: Default::default(),
            cascades_engine: Default::default(),
            display: Default::default(),
            damage_engine: Default::default(),
            dt: 0.0,
            dt_accum: 0.0,
            frame_id: 0,
            time: Duration::ZERO,
            prev_stamp: None,
            prev_cascade_fp: None,
            #[cfg(test)]
            dbg_cascade_ran: false,
            fps_ema: 0.0,
            repaint_requested: false,
            repaint_wakes: Vec::new(),
            anim: Default::default(),
            frame_state: Default::default(),
            relayout_requested: false,
            pending_windows: Vec::new(),
            pending_closes: Vec::new(),
        }
    }
}

/// Construction + host-driven frame lifecycle: `frame` and the private
/// record / clock / classify / cascade / finalize passes it runs. User
/// code never calls these directly — `WindowRenderer` drives them. The widget
/// authoring API lives in the second `impl Ui` block below.
impl Ui {
    /// Per-frame `dt` clamp (seconds). Stalled frames freeze
    /// animation tickers instead of teleporting; [`Self::time`]
    /// still tracks the host's true clock.
    pub(crate) const MAX_DT: f32 = 0.1;

    /// Construct a per-window `Ui` from the host's shared render resources
    /// and its app-global [`HostShared`]. From `ctx` it clones the same
    /// `TextShaper` the wgpu backend uses (so layout-time measurement and
    /// render-time shaping hit one buffer cache), the same `FrameArena`
    /// the `Frontend` + `WgpuBackend` see (so every phase reads one set of
    /// per-frame mesh / polyline bytes), the render caches, and GPU-stats.
    /// `host` is owned by the windowing host (`WinitHost` / `OffscreenHost`)
    /// — *not* by `RenderContext` — and a clone is threaded in here so
    /// every window's `Ui` shares one live-window set + debug overlay.
    /// [`crate::WindowRenderer::new`] calls this.
    ///
    /// Tests / standalone callers usually want [`Self::default`], which
    /// builds an isolated `Ui` with mono fallback shaper + its own private
    /// arena.
    pub(crate) fn new(ctx: &RenderContext, host: HostShared) -> Self {
        Self {
            ctx: ctx.clone(),
            host,
            ..Self::default()
        }
    }

    // ── Frame lifecycle ───────────────────────────────────────────────

    /// The only public entry point for driving a frame. Runs `record`
    /// once, re-records on action input or `request_relayout`, paints
    /// the last pass. `stamp.time` is monotonic host time;
    /// `Ui::{dt,time,frame_id}` derive from it. See `docs/repaint.md`.
    pub fn frame(&mut self, stamp: FrameStamp, mut record: impl FnMut(&mut Ui)) -> FrameReport {
        profiling::scope!("Ui::frame");
        // Frame arena is cleared inside `record_pass` (the only path
        // that repopulates it). PaintOnly frames must NOT clear: the
        // live `tree.shapes` from last frame still references arena
        // contents by index (gradients, polyline points/colors, mesh
        // verts/indices, interned text spans). Clearing here would
        // leave dangling indices the encoder then dereferences.
        assert!(
            stamp.display.scale_factor >= EPS,
            "Display::scale_factor must be ≥ EPSILON; got {}",
            stamp.display.scale_factor,
        );

        let first_frame = self.prev_stamp.is_none();
        self.advance_clock(stamp.time);
        // Refresh the input clock so input handlers running before the
        // next frame timestamp double-clicks on this deterministic time.
        self.input.frame_time = self.time;
        let plan = self.classify_frame(stamp.display);

        self.repaint_requested = false;
        self.relayout_requested = false;

        if let FramePlan::FullRecord { force_full: true } = plan {
            self.damage_engine.invalidate_prev();
            self.prev_stamp = None;
        }
        self.display = stamp.display;

        // Pending until the renderer (`WindowRenderer::render`) confirms a
        // successful submit. Tests driving `Ui::frame` directly must
        // ack via `ui.frame_state.mark_submitted()` or the next
        // frame's `classify_frame` will force a `Full`.
        self.frame_state.mark_pending();

        let processing = match plan {
            FramePlan::PaintOnly => {
                profiling::scope!("Ui::frame.paint_only");
                // PaintOnly skips `record_pass` → skips `post_record`
                // → skips the per-frame input drain. Under `OnDelta`
                // a frame can land here with `had_input_since_last_frame`
                // true (e.g. pointer move over inert surface) and
                // per-frame accumulators populated (scroll over a
                // non-scroll widget). Drain them: nothing recorded this
                // frame can react, and leaving them set would either
                // pin the sticky bit forever or fire stale scroll the
                // next time a real scroll widget appears under the
                // pointer.
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
                    self.relayout_requested = false;
                    self.repaint_requested = false;
                }
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
        let prev_time = self.prev_stamp.map(|s| s.time);
        let input = DamageInput {
            forest: &self.forest,
            cascades: &self.cascades,
            surface,
            prev_time,
            now: self.time,
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
        assert!(
            !first_frame || matches!(damage, Damage::Full),
            "first frame must produce Damage::Full; got {damage:?}",
        );

        // Skip frames have nothing for the host to submit, so ack
        // here — otherwise `frame_state` stays `Pending` and the next
        // paint frame's `classify_frame` escalates to `Full`.
        if damage.is_skip() {
            self.frame_state.mark_submitted();
        }

        // Re-queue the next paint-anim boundary regardless of path.
        // FullRecord rebuilt `paint_anims.entries` during record;
        // PaintOnly retained last frame's. Either way the fold below
        // gives the next quantum boundary — without this, PaintOnly
        // drains the queued ANIM wake without replacing it and the
        // caret freezes until input forces a FullRecord.
        let min_wake = self.forest.min_paint_anim_wake(self.time);
        if min_wake != Duration::MAX {
            self.schedule_wake(min_wake, WakeReasons::ANIM);
        }

        self.prev_stamp = Some(stamp);

        FrameReport {
            repaint_requested: self.repaint_requested,
            repaint_after: self.repaint_wakes.first().map(|w| w.deadline),
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
        let raw_dt = now
            .saturating_sub(self.time)
            .as_secs_f32()
            .min(Self::MAX_DT);
        // EMA over instantaneous fps. First frame: raw_dt is `now`
        // (vs ZERO), giving an absurd reading; skip the update there.
        // Coefficient 0.1 ≈ ~10-frame window — smooth enough that
        // the readout doesn't jitter wildly, fast enough to track
        // real frame-rate drops.
        if self.frame_id > 0 && raw_dt > EPS {
            let inst = 1.0 / raw_dt;
            self.fps_ema = if self.fps_ema == 0.0 {
                inst
            } else {
                self.fps_ema * 0.9 + inst * 0.1
            };
        }
        self.dt_accum += raw_dt;
        self.dt = if self.dt_accum >= ANIM_SUBSTEP_DT {
            let spent = self.dt_accum;
            self.dt_accum = 0.0;
            spent
        } else {
            0.0
        };
        self.time = now;
        self.frame_id += 1;
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
            .repaint_wakes
            .partition_point(|w| w.deadline <= self.time);
        let fired_reasons = self
            .repaint_wakes
            .drain(..fired_count)
            .fold(WakeReasons::default(), |acc, w| acc.merge(w.reasons));

        let display_changed = self
            .prev_stamp
            .is_some_and(|prev| prev.display.logical_rect() != display.logical_rect());
        let frame_skipped = !self.frame_state.was_last_submitted();
        let force_full = display_changed || frame_skipped;
        if force_full {
            tracing::debug!(
                display_changed,
                frame_skipped,
                first_frame = self.prev_stamp.is_none(),
                "damage.invalidate_prev"
            );
        }

        let input_forces_record = match self.input_policy {
            InputPolicy::Always => self.input.had_input_since_last_frame,
            InputPolicy::OnDelta => self.input.repaint_requested_since_last_frame,
        };
        let paint_only = !force_full
            && self.prev_stamp.is_some()
            && !self.repaint_requested
            && !input_forces_record
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
            self.ctx.frame_arena.clear();
            self.forest.pre_record();
            // Subscription set is rebuilt from scratch each full record
            // pass — symmetric to `Sense` on a node. Widgets re-assert
            // during record; ones that didn't run drop their wake.
            // Across silent (PaintOnly / skipped) frames the set
            // persists, which is the whole point: a dormant popup
            // needs `BUTTONS` to still be set when the next click
            // outside lands.
            self.input.subs.clear();
            // Snapshot the theme's default line height once per frame
            // for `InputState::response_for` to consume — avoids
            // repeating the `line_height_for` multiply on every
            // per-widget response_for call.
            self.input.frame_line_px = self
                .theme
                .text
                .line_height_for(self.theme.text.font_size_px);
        }
        // Synthetic viewport root for Layer::Main. Without this, the
        // first user-recorded node becomes the root and the layout
        // engine forces its rect to the surface — silently overriding
        // declared `Sizing` / `Sense` on the top-level widget. ZStack +
        // Fill matches the historical "root paints full surface"
        // behavior while letting user roots respect their own sizing.
        let mut viewport = Element::new(LayoutMode::ZStack);
        viewport.size = Sizing::FILL.into();
        // Hard-coded `WidgetId::VIEWPORT` — `make_persistent_id` skips
        // this id as a parent, so user `id_salt("k")` at the top level
        // resolves bare instead of `VIEWPORT.with(salt)`.
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
    /// invariant on [`Self::repaint_wakes`], coalesces requests within
    /// one display-refresh interval onto the later deadline, and OR-merges
    /// reasons when two requests land on the same slot. Merging is
    /// what lets the frame-entry classifier see a wake that *both* an
    /// anim and a widget asked for as `REAL | ANIM`, which forces the
    /// Full path (correct — the widget needs record).
    fn schedule_wake(&mut self, deadline: Duration, reasons: WakeReasons) {
        let pos = match self
            .repaint_wakes
            .binary_search_by_key(&deadline, |w| w.deadline)
        {
            Ok(i) => {
                self.repaint_wakes[i].reasons = self.repaint_wakes[i].reasons.merge(reasons);
                return;
            }
            Err(pos) => pos,
        };
        let coalesce = coalesce_dt_for_refresh(self.display.refresh_millihertz);
        let near = |existing: Duration| existing.abs_diff(deadline) < coalesce;
        // Coalesce to the later of (existing, requested) — collapse
        // bursts into a single wake at the back of the window to avoid
        // unnecessary host wakes. pos-1 is earlier than deadline
        // (overwrite with ours, but keep merged reasons); pos is later
        // (keep its deadline, merge our reasons in).
        if pos < self.repaint_wakes.len() && near(self.repaint_wakes[pos].deadline) {
            self.repaint_wakes[pos].reasons = self.repaint_wakes[pos].reasons.merge(reasons);
            return;
        }
        if pos > 0 && near(self.repaint_wakes[pos - 1].deadline) {
            self.repaint_wakes[pos - 1].deadline = deadline;
            self.repaint_wakes[pos - 1].reasons =
                self.repaint_wakes[pos - 1].reasons.merge(reasons);
            return;
        }
        self.repaint_wakes.insert(pos, Wake { deadline, reasons });
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
        let arena = self.ctx.frame_arena.inner();
        let tc = TextCtx {
            bytes: &arena.fmt_scratch,
            shaper: &self.ctx.shaper,
        };
        self.layout_engine.run(
            &self.forest,
            &tc,
            self.display.logical_rect(),
            &mut self.layout,
        );
        drop(arena);
        // O5 stage 0: skip the cascade when nothing feeding it changed.
        // The cascade is a pure function of subtree authoring + arranged
        // rects, and the arranged rects are determined by (subtree_hash,
        // exact surface, scroll offset/zoom) — so a matching fingerprint
        // means identical cascade output, and last frame's
        // `Ui::cascades` can be reused verbatim (the tree is rebuilt
        // with identical structure when `subtree_hash` matches, so its
        // NodeId-indexed rows still line up).
        let fp = self.cascade_fingerprint();
        if self.prev_cascade_fp == Some(fp) {
            #[cfg(test)]
            {
                self.dbg_cascade_ran = false;
            }
            return;
        }
        #[cfg(test)]
        {
            self.dbg_cascade_ran = true;
        }
        self.prev_cascade_fp = Some(fp);
        self.cascades_engine
            .run(&self.forest, &self.layout, &mut self.cascades);
    }

    /// Fingerprint of everything the cascade reads, cheaply. Equal
    /// fingerprints across two frames ⇒ identical cascade output (see
    /// [`Self::prev_cascade_fp`]). Folds:
    /// - the exact surface (a sub-quantum resize can hit the measure
    ///   cache yet still re-arrange, so the *exact* rect must be here);
    /// - every root's `subtree_hash`, which already captures all cascade
    ///   authoring — transforms (`PanelExtras`), clip/disabled/focusable
    ///   (`attrs`), visibility, shapes, chrome;
    /// - scroll `offset`/`zoom`, the one cross-frame arrange input that
    ///   lives in `LayoutEngine.scroll_states`, not in `subtree_hash`.
    fn cascade_fingerprint(&self) -> u64 {
        use std::hash::Hasher as _;
        let mut h = Hasher::new();
        h.write_u32(self.display.physical.x);
        h.write_u32(self.display.physical.y);
        h.write_u32(self.display.scale_factor.to_bits());
        for (_layer, tree) in self.forest.iter_paint_order() {
            for slot in &tree.roots {
                h.write_u64(tree.rollups.subtree[slot.first_node.idx()].0);
            }
        }
        // XOR fold so map iteration order doesn't matter.
        let mut scroll_fold = 0u64;
        for (wid, st) in self.layout_engine.scroll_states.iter() {
            let mut sh = Hasher::new();
            sh.write_u64(wid.0);
            sh.write_u32(st.offset.x.to_bits());
            sh.write_u32(st.offset.y.to_bits());
            sh.write_u32(st.zoom.to_bits());
            scroll_fold ^= sh.finish();
        }
        h.finish() ^ scroll_fold
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

        self.input.post_record(&self.cascades);
    }
}

/// Widget- and host-facing authoring API: input feed, subscriptions,
/// repaint/relayout requests, shape recording, per-widget state, and
/// animation. Distinct from the host-driven frame lifecycle above
/// (`frame` + its private record/cascade/finalize passes), which user
/// code never calls directly.
impl Ui {
    /// Feed a palantir-native input event. Returns an [`InputDelta`]
    /// the host reads to decide whether to request a redraw — pointer
    /// moves over inert surfaces leave `requests_repaint` false so the
    /// host can skip the frame entirely. Animation/tooltip-delay wakes
    /// still drive paints independently via `FrameReport::repaint_after`.
    pub fn on_input(&mut self, event: InputEvent) -> InputDelta {
        self.input.on_input(event, &self.cascades)
    }

    // ── Subscriptions ─────────────────────────────────────────────
    //
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

    // ── Event readers ────────────────────────────────────────────

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

    // ── Keyboard convenience queries ──────────────────────────────

    /// `true` if any [`KeyboardEvent::Down`] this frame matches
    /// `sc`. Iterates [`Self::keyboard_events`]; for repeat or
    /// stateful logic, iterate directly instead.
    ///
    /// Side-effect: auto-subscribes the chord for wake-up. Without
    /// this, palantir's keyboard wake-gate ([`crate::input::InputState`]'s
    /// `KeyDown` arm) parks off-focus presses until the next unrelated
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
        self.relayout_requested = true;
    }

    /// Monotonic time of the current frame, accumulated from the
    /// per-frame `dt`s the host feeds in. Starts at zero on the first
    /// frame and only moves forward. Read-only on purpose: the clock is
    /// host-driven, and a direct write would desync it from the wake
    /// queue. Use for time-driven animation that needs a continuous
    /// clock rather than a tween toward a fixed target (e.g. `Spinner`);
    /// pair with [`Self::request_repaint`] to keep the host awake.
    pub fn now(&self) -> Duration {
        self.time
    }

    /// Ask the host to schedule another frame after this one. Cleared
    /// at the top of every `frame`; widgets/showcases that need
    /// continuous animation call this each frame to keep the host
    /// awake.
    #[track_caller]
    pub fn request_repaint(&mut self) {
        let caller = std::panic::Location::caller();
        tracing::trace!(
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
        self.schedule_wake(deadline, WakeReasons::REAL);
    }

    /// Open a new top-level OS window addressed by `token`. The window
    /// gets its own independent UI tree; [`App::frame`](crate::App::frame)
    /// is called for it with `token`, and you can later poke it via
    /// [`HostHandle::request_repaint`](crate::HostHandle::request_repaint)
    /// or close it with [`Self::close_window`].
    ///
    /// Creation is deferred, not inline: the request is queued and the
    /// host (`WinitHost`) creates the real window on the event-loop
    /// thread right after this frame, so it's safe to call mid-record.
    /// Idempotent within a frame is *not* guaranteed — call once per
    /// window you want; a `token` already in use is ignored with a
    /// warning. No-op in headless contexts (no host to drain the queue).
    ///
    /// `token` is yours to define — an enum discriminant, an index, a
    /// document-id hash. It must be unique across live windows. `config`
    /// is the backend-agnostic [`WindowConfig`] (title + size); the
    /// window inherits the app-global GPU settings from startup.
    pub fn open_window(&mut self, token: WindowToken, config: WindowConfig) {
        self.pending_windows.push(PendingWindow { token, config });
    }

    /// Request that the window addressed by `token` close. Deferred like
    /// [`Self::open_window`] — the host removes it after this frame. The
    /// last window closing exits the event loop. No-op if `token` names
    /// no live window, or in headless contexts.
    pub fn close_window(&mut self, token: WindowToken) {
        self.pending_closes.push(token);
    }

    /// Mutable handle to this app's debug overlay; the guard derefs to
    /// `&mut DebugOverlayConfig`, so write fields straight on it
    /// (`ui.debug_overlay_mut().damage_rect = true`). The overlay is
    /// app-global: the write is visible to every window at once, and the
    /// host repaints idle windows so it shows everywhere — not just the
    /// window that handled the key. Drop the guard before other `Ui`
    /// calls; the `&mut self` borrow enforces that.
    pub fn debug_overlay_mut(&mut self) -> RefMut<'_, DebugOverlayConfig> {
        self.host.debug_overlay_mut()
    }

    /// This app's current debug overlay. Read by the backend at submit
    /// time and by `Ui::frame` to drive the FPS readout.
    pub(crate) fn debug_overlay(&self) -> DebugOverlayConfig {
        self.host.debug_overlay()
    }

    /// Whether a window addressed by `token` is currently live. Reflects
    /// the set as of this frame's *start*, so a window opened or closed
    /// earlier *this* frame isn't reflected until the next one (the host
    /// drains [`Self::open_window`] / [`Self::close_window`] between
    /// frames). Use it as the source of truth for "is this window up?"
    /// instead of mirroring the state in app code — a window the user
    /// closed via its titlebar drops out of this set automatically.
    pub fn window_open(&self, token: WindowToken) -> bool {
        self.host.window_open(token)
    }

    // ── Recording (widget-facing) ─────────────────────────────────────

    pub fn add_shape(&mut self, shape: Shape<'_>) {
        self.forest
            .add_shape(shape, &self.ctx.frame_arena, &self.ctx.caches.gradients);
    }

    /// Upload an image and get back an owning [`ImageHandle`]. **Hold the
    /// handle** to keep the GPU texture resident — dropping the last
    /// clone frees it; there is no `unregister`. Reference it in
    /// [`Shape::Image`] every frame (`clone` it where it needs to live).
    /// The CPU bytes are dropped right after the upload.
    pub fn register_image(&self, image: Image) -> ImageHandle {
        self.ctx.caches.images.register(image)
    }

    /// Format `args` directly into the per-frame text arena and return
    /// an [`InternedStr::Interned`] handle. Pass the returned value to
    /// any widget that takes `impl Into<InternedStr>`
    /// (Text/Button/MenuItem) — the bytes are already in the destination
    /// buffer, so lowering is zero-copy and steady-state authoring of
    /// dynamic labels skips per-call `String` allocations.
    ///
    /// **Frame-scoped.** The handle is invalidated by the next
    /// [`Self::frame`] call (the arena clears at frame start). Don't
    /// store it in `state_mut::<T>(...)` or any cross-frame state — the
    /// span will silently point at the wrong bytes next frame. For
    /// persistent strings store the original `String` / `&'static str`
    /// and `.into()` it back into `InternedStr` each frame. The type
    /// signature can't express this constraint, so the borrow checker
    /// won't catch a misuse — `#[must_use]` is a hint that the result
    /// is meant to be consumed in the same frame.
    #[must_use]
    pub fn fmt(&mut self, args: std::fmt::Arguments<'_>) -> InternedStr {
        self.ctx.frame_arena.intern_fmt(args)
    }

    /// Copy `s` into the per-frame text arena and return an
    /// [`InternedStr::Interned`] handle. Format-less twin of
    /// [`Self::fmt`] for plain `&str` borrows whose lifetime doesn't
    /// reach `'static` — turns a per-frame `String` allocation into a
    /// memcpy into the retained `fmt_scratch` buffer. Same
    /// frame-scoped invalidation rules as [`Self::fmt`].
    #[must_use]
    pub fn intern(&mut self, s: &str) -> InternedStr {
        self.ctx.frame_arena.intern_str(s)
    }

    /// Append `shape` to the active node and register `anim` against
    /// it. The encoder samples `anim` at paint time and folds the
    /// resulting `PaintMod` into the shape's brush; `post_record`
    /// folds the anim's `next_wake` into `repaint_wakes` so the
    /// caller doesn't manage scheduling. Drops silently if the shape
    /// itself was noop-collapsed (zero stroke + transparent fill,
    /// etc.) — `PaintAnim` can't make a zero shape paintable.
    pub(crate) fn add_shape_animated(&mut self, shape: Shape<'_>, anim: PaintAnim) {
        self.forest.add_shape_animated(
            shape,
            anim,
            &self.ctx.frame_arena,
            &self.ctx.caches.gradients,
        );
    }

    /// Record `body` as a side layer placed at `anchor` (top-left
    /// position). `size = None` makes the body's "available" extend
    /// from `anchor` to the surface bottom-right; `size = Some(s)`
    /// caps it at `s`, still clamped to the surface so an oversized
    /// cap can't bleed past the viewport. The root's own `Sizing`
    /// (Hug/Fill/Fixed) then governs the painted size within that
    /// available. Must be called at top-level (no node open) —
    /// egui-style: finish the `Main` scope first, then layer.
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

    /// Resolve a [`Salt`] into the `WidgetId` that will be recorded
    /// into the tree by the matching `ui.node` call. The egui-equivalent
    /// — same name, same role:
    /// `ui.make_persistent_id(salt)` returns `parent.with(salt)` (or
    /// just the salt for `Salt::Auto`/`Salt::Verbatim`) so persistent
    /// state keys stay stable across frames.
    ///
    /// **Eagerly disambiguates** via `SeenIds::resolve` — if the salt
    /// collides with a sibling already recorded this frame, the id
    /// gets bumped to a fresh occurrence slot. Same contract as
    /// `Forest::open_node` used to apply: the returned id matches
    /// what the tree, cascade, and `response_for` will see. Widgets
    /// can use the returned id for **everything** — pre-node
    /// `response_for` (theme picking off prior frame), sub-id
    /// derivation, animation slots, `state_mut`, and post-node
    /// `response_for`.
    ///
    /// **Contract**: must be followed by exactly one `ui.node` opening
    /// a node with this id (carried via
    /// `element.salt`). The `SeenIds` slot reserved here is paired
    /// with the next opened node; calling `make_persistent_id` twice
    /// without an intervening `ui.node` will drift the occurrence
    /// counter from the actual node-record list.
    ///
    /// Parent context is the most-recently-opened node in the current
    /// layer. `Layer::Main`'s synthetic viewport (`Ui::record_pass`'s
    /// implicit ZStack) shows up here like any other parent — its
    /// `Salt::Auto` id is stable across frames (one
    /// `#[track_caller]` site), so root salts resolve to a fixed
    /// `viewport_id.with(salt)`. Widgets stay agnostic; they get
    /// stable ids without a Main-vs-other-layer carve-out.
    pub(crate) fn make_persistent_id(&mut self, salt: Salt) -> WidgetId {
        let scratch = self.forest.current_scratch();
        let tree = self.forest.current_tree();
        let parent = scratch
            .open_frames
            .last()
            .map(|f| tree.records.widget_id()[f.node.idx()]);
        let raw_id = salt.resolve(parent);
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
    /// `FrameArena::lower_background`, and the no-chrome path is just a
    /// perfectly-predicted `None` branch.
    ///
    /// `id` must be the [`Self::make_persistent_id`] resolution of
    /// `element.salt`. Disambiguation already happened there, so this is
    /// the final id verbatim — no further `SeenIds` work here.
    pub(crate) fn node<R>(
        &mut self,
        id: WidgetId,
        element: Element,
        chrome: Option<&Background>,
        f: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        let chrome = chrome.map(|bg| Chrome {
            bg,
            arena: &self.ctx.frame_arena,
            atlas: &self.ctx.caches.gradients,
        });
        self.forest.open_node(id, element, chrome);
        let r = f(self);
        self.forest.close_node();
        r
    }

    /// Snapshot of input/cascade state for a widget. `rect` and
    /// `disabled` are from the previous frame's cascade; everything
    /// else (`pressed`, `hovered`, `drag_started`, `drag_delta`) is
    /// computed against the current frame's input state and so is
    /// safe to read before this frame's record runs — useful for
    /// e.g. baking drag deltas into a widget's position before
    /// recording it.
    pub fn response_for(&self, id: WidgetId) -> ResponseState {
        // Wheel-line → pixels uses `InputState::frame_line_px`, the
        // once-per-frame snapshot of the theme's default line height
        // (populated by `record_pass`). Per-widget call here avoids
        // redoing the multiply and stays consistent if the theme is
        // swapped mid-frame.
        let mut state = self.input.response_for(id, &self.cascades);
        // Cascade lags one frame; OR this frame's ancestor-disabled so
        // a freshly-disabled subtree paints disabled on its first frame.
        state.disabled |= self.forest.current_scratch().ancestor_disabled();
        state
    }

    // ── Cross-frame state & animation ─────────────────────────────────

    /// Cross-frame state row for `id`, `T::default()` on first
    /// access. Rows for `WidgetId`s not recorded this frame are
    /// evicted in `post_record`. Panics on type collision at `id`.
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
        let r = self
            .anim
            .typed_mut::<V>()
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
/// `TextShaper::*`, `tessellate_polyline_for_bench`) stay in their
/// own modules.
#[cfg(any(test, feature = "internals"))]
pub mod test_support {
    #![allow(dead_code)]
    use crate::FrameStamp;
    use crate::animation::animatable::Animatable;
    use crate::forest::Layer;
    use crate::forest::tree::NodeId;
    use crate::input::InputEvent;
    use crate::input::pointer::PointerButton;
    use crate::layout::scroll::ScrollLayoutState;
    use crate::primitives::rect::Rect;
    use crate::renderer::frontend::cmd_buffer::RenderCmdBuffer;
    use crate::renderer::frontend::encoder::encode;
    use crate::text::TextShaper;
    use crate::ui::damage::Damage;
    use crate::ui::damage::region::DamageRegion;
    use crate::ui::frame_report::RenderPlan;
    use crate::ui::*;
    use glam::{UVec2, Vec2};
    use std::time::Duration;

    impl Ui {
        // ── forest ──────────────────────────────────────────────

        /// `Layer::Main` node whose `widget_id` matches `id`. Panics if absent.
        pub fn node_for_widget_id(&self, id: WidgetId) -> NodeId {
            let tree = self.forest.tree(Layer::Main);
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
        pub fn for_test() -> Self {
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
            let ctx = RenderContext::new(SHARED.with(|c| c.clone()));
            let mut ui = Self::new(&ctx, HostShared::default());
            ui.mark_warm_for_test();
            ui
        }

        /// `Ui` pre-stamped with display dimensions; no user frame
        /// driven yet. Pre-marked as warm.
        pub fn for_test_at(size: UVec2) -> Self {
            let mut ui = Self {
                display: Display::from_physical(size, 1.0),
                ..Self::default()
            };
            ui.mark_warm_for_test();
            ui
        }

        /// `Ui` with cosmic shaper, pre-stamped with display dimensions.
        pub fn for_test_at_text(size: UVec2) -> Self {
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
        /// snapshot all stay at fresh-construction defaults. The
        /// damage engine still treats the first user frame as fully
        /// dirty (`prev` map is empty), so `Damage::Full` is still the
        /// observed first-user-frame outcome.
        fn mark_warm_for_test(&mut self) {
            self.prev_stamp = Some(FrameStamp::new(self.display, Duration::ZERO));
            self.frame_state.mark_submitted();
        }

        /// One frame at `size`, time frozen at zero.
        pub fn run_at(&mut self, size: UVec2, record: impl FnMut(&mut Ui)) {
            let display = Display::from_physical(size, 1.0);
            self.frame(FrameStamp::new(display, Duration::ZERO), record);
        }

        /// `run_at` then mark the frame as submitted (suppress next-frame auto-rewind to `Full`).
        pub fn run_at_acked(&mut self, size: UVec2, record: impl FnMut(&mut Ui)) {
            self.run_at(size, record);
            self.frame_state.mark_submitted();
        }

        /// Ack the just-run frame as presented — mirrors what the host
        /// does after a successful submit, so the next [`Self::frame`]
        /// doesn't auto-escalate to `Full`. For tests and for benches
        /// that drive `frame` + a standalone
        /// [`crate::renderer::frontend::Frontend::build_for_test`]
        /// instead of going through `WindowRenderer` (the `frame/*_cpu` arms).
        pub fn mark_frame_submitted(&mut self) {
            self.frame_state.mark_submitted();
        }

        /// Wrap UUT inside a Fill HStack so the panel can express its own measured size.
        pub fn under_outer<F: FnMut(&mut Ui) -> NodeId>(
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

        // ── input ────────────────────────────────────────────────

        pub fn click_at(&mut self, pos: Vec2) {
            self.on_input(InputEvent::PointerMoved(pos));
            self.on_input(InputEvent::PointerPressed(PointerButton::Left));
            self.on_input(InputEvent::PointerReleased(PointerButton::Left));
        }

        pub fn press_at(&mut self, pos: Vec2) {
            self.on_input(InputEvent::PointerMoved(pos));
            self.on_input(InputEvent::PointerPressed(PointerButton::Left));
        }

        pub fn release_left(&mut self) {
            self.on_input(InputEvent::PointerReleased(PointerButton::Left));
        }

        pub fn secondary_click_at(&mut self, pos: Vec2) {
            self.on_input(InputEvent::PointerMoved(pos));
            self.on_input(InputEvent::PointerPressed(PointerButton::Right));
            self.on_input(InputEvent::PointerReleased(PointerButton::Right));
        }

        // ── layout cache ────────────────────────────────────────

        /// Drop every measure-cache entry, forcing full re-measure next frame.
        pub fn clear_measure_cache(&mut self) {
            let cache = &mut self.layout_engine.cache;
            cache.nodes.clear();
            cache.hugs.clear();
            cache.text_shapes_arena.clear();
            cache.snapshots.clear();
        }

        /// Scroll-state row for `id` (inserting default if absent).
        pub fn scroll_state(&mut self, id: WidgetId) -> &mut ScrollLayoutState {
            self.layout_engine.scroll_states.entry(id).or_default()
        }

        // ── cascade ─────────────────────────────────────────────

        /// Run only the cascade pass against the just-finished frame.
        pub fn run_cascades(&mut self) {
            self.cascades_engine
                .run(&self.forest, &self.layout, &mut self.cascades);
        }

        // ── damage ──────────────────────────────────────────────

        /// Rebuild the post-collapse damage region from `DamageEngine`'s
        /// last-frame pass-1 buffer. Doesn't mutate state.
        pub fn damage_region(&self) -> DamageRegion {
            DamageRegion::collapse_from(
                &self.damage_engine.raw_rects,
                self.damage_engine.budget_px,
                self.display.logical_rect(),
            )
        }

        /// Damage rects produced by the most recent `post_record`.
        pub fn damage_rect_count(&self) -> usize {
            self.damage_region().iter_rects().count()
        }

        /// Subtree-skip jumps the last damage diff performed.
        pub fn damage_subtree_skips(&self) -> u32 {
            self.damage_engine.subtree_skips
        }

        /// Live entries in the `paint_snaps` arena (sum of every
        /// `NodeSnapshot::paint_span.len`, including orphaned tail).
        pub fn damage_shape_snaps_len(&self) -> usize {
            self.damage_engine.arena.len()
        }

        /// Count of orphaned `Paint` entries in the arena —
        /// drives the compaction trigger.
        pub fn damage_shape_snaps_orphaned(&self) -> u32 {
            self.damage_engine.arena.orphaned()
        }

        /// Times `compact_shape_snaps` has run on this engine.
        /// Used by benches to verify the compaction path was actually
        /// exercised and to count compactions over a measurement
        /// window.
        pub fn damage_compactions_run(&self) -> u32 {
            self.damage_engine.arena.compactions_run()
        }

        /// `"skip"` / `"partial"` / `"full"` — the frame's final paint decision.
        pub fn damage_paint_kind(&self) -> &'static str {
            match Damage::new(self.display.logical_rect(), self.damage_region()) {
                Damage::Skip => "skip",
                Damage::Full => "full",
                Damage::Partial(_) => "partial",
            }
        }

        // ── animation ───────────────────────────────────────────

        /// Animation rows currently allocated for `T`, or 0 if no typed map exists.
        pub fn anim_row_count<T: Animatable>(&mut self) -> usize {
            self.anim.try_typed_mut::<T>().map_or(0, |t| t.rows.len())
        }

        // ── encoder ─────────────────────────────────────────────

        pub fn encode_cmds(&self) -> RenderCmdBuffer {
            self.encode_cmds_filtered(None)
        }

        pub fn encode_cmds_filtered(&self, filter: Option<Rect>) -> RenderCmdBuffer {
            self.encode_cmds_with_region(filter.map(DamageRegion::from))
        }

        /// Multi-rect variant; each rect is fed through `DamageRegion::add` so merge policy applies.
        pub fn encode_cmds_with_rects(&self, rects: &[Rect]) -> RenderCmdBuffer {
            let region = if rects.is_empty() {
                None
            } else {
                let mut r = DamageRegion::default();
                for rect in rects {
                    r.add(*rect);
                }
                Some(r)
            };
            self.encode_cmds_with_region(region)
        }

        fn encode_cmds_with_region(&self, region: Option<DamageRegion>) -> RenderCmdBuffer {
            let clear = self.theme.window_clear;
            let plan = match region {
                Some(region) => RenderPlan::Partial { clear, region },
                None => RenderPlan::Full { clear },
            };
            let mut cmds = RenderCmdBuffer::default();
            let arena = self.ctx.frame_arena.inner();
            encode(self, &arena, plan, &mut cmds);
            cmds
        }
    }
}

#[cfg(test)]
mod tests;
