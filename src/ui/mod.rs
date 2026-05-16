pub mod cascade;
pub mod damage;
pub mod frame_report;
pub mod frame_state;
pub(crate) mod frame_stats;
pub(crate) mod state;

use crate::animation::animatable::Animatable;
use crate::animation::paint::PaintAnim;
use crate::animation::{AnimMap, AnimSlot, AnimSpec};
use crate::common::frame_arena::FrameArenaHandle;
use crate::common::time::{ANIM_SUBSTEP_DT, REPAINT_COALESCE_DT};
use crate::debug_overlay::DebugOverlayConfig;
use crate::forest::Forest;
use crate::forest::element::{Element, LayoutMode};
use crate::forest::tree::Layer;
use crate::input::keyboard::KeyboardEvent;
use crate::input::pointer::PointerEvent;
use crate::input::policy::InputPolicy;
use crate::input::shortcut::Shortcut;
use crate::input::subscriptions::{KeyboardSense, PointerSense};
use crate::input::{FocusPolicy, InputDelta, InputEvent, InputState, ResponseState};
use crate::layout::Layout;
use crate::layout::layoutengine::LayoutEngine;
use crate::layout::types::display::Display;
use crate::layout::types::sizing::Sizing;
use crate::primitives::approx::EPS;
use crate::primitives::background::Background;
use crate::renderer::caches::RenderCaches;

use crate::primitives::widget_id::WidgetId;
use crate::shape::Shape;
use crate::text::TextShaper;
use crate::ui::cascade::CascadesEngine;
use crate::ui::damage::DamageEngine;
use crate::ui::frame_report::{FrameProcessing, FrameReport, RenderPlan};
use crate::ui::frame_state::FrameState;
use crate::ui::frame_stats::record_frame_stats;
use crate::ui::state::StateMap;
use crate::widgets::theme::Theme;
use std::any::TypeId;
use std::ptr::NonNull;
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

/// Host-supplied per-frame inputs — monotonic time + active
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

/// What [`Ui::frame_inner`] should do this frame, decided at entry
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
///
/// `Default` builds a self-contained `Ui` with mono-fallback shaper
/// and a private frame arena. Hosts that need to share the shaper /
/// arena with the wgpu backend use [`Self::new`] instead.
#[derive(Default)]
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
    /// Host-supplied monotonic timestamp for this frame.
    pub(crate) time: Duration,
    /// Time + display from the previous successful frame, or `None`
    /// before the first frame and after
    /// [`DamageEngine::invalidate_prev`] rewinds the snapshot.
    /// Drives `classify_frame` (surface-change detection)
    /// and the paint-anim damage gate
    /// (`anim.next_wake(prev.time) <= now`). Updated at the bottom
    /// of `frame_inner` on every path.
    pub(crate) prev_stamp: Option<FrameStamp>,
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
    /// Ambient caller-owned app state for the current frame. Installed
    /// by [`Self::frame`], cleared by the RAII guard on scope exit
    /// (incl. panic). Retrieved via [`Self::app`].
    app_slot: Option<AppSlot>,
    /// Per-frame bulk geometry arena (mesh verts/indices, polyline
    /// points/colors), shared with the renderer via [`Host`]: `Host`
    /// constructs the canonical [`Rc`] and clones it into `Ui`,
    /// `Frontend`, and `WgpuBackend` so every phase sees the same
    /// bytes. Standalone `Ui::for_test()` builds its own private handle.
    /// `add_shape` calls `borrow_mut()` for the call duration.
    ///
    /// [`Host`]: crate::Host
    pub(crate) frame_arena: FrameArenaHandle,
    /// Cross-frame GPU resource caches (image registry + gradient
    /// atlas) shared with the wgpu backend. Users call
    /// `ui.caches.images.register(key, image)` to stage bytes once,
    /// then reference the returned handle in [`Shape::Image`] every
    /// frame; gradient atlas registration is internal (driven from
    /// shape lowering, not user code).
    pub caches: RenderCaches,
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
    pub fn new(text: TextShaper, frame_arena: FrameArenaHandle, caches: RenderCaches) -> Self {
        Self {
            text,
            frame_arena,
            caches,
            ..Self::default()
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
    /// Callers without app state pass `&mut ()`. `stamp.time` is
    /// monotonic host time; `Ui::{dt,time,frame_id}` derive from it.
    /// See `docs/repaint.md`.
    pub fn frame<T: 'static>(
        &mut self,
        stamp: FrameStamp,
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
        g.ui.frame_inner(stamp, &mut record)
    }

    fn frame_inner(&mut self, stamp: FrameStamp, mut record: impl FnMut(&mut Ui)) -> FrameReport {
        profiling::scope!("Ui::frame");
        assert!(
            stamp.display.scale_factor >= EPS,
            "Display::scale_factor must be ≥ EPSILON; got {}",
            stamp.display.scale_factor,
        );

        self.advance_clock(stamp.time);
        let plan = self.classify_frame(stamp.display);

        self.repaint_requested = false;
        self.relayout_requested = false;

        if let FramePlan::FullRecord { force_full: true } = plan {
            self.damage_engine.invalidate_prev();
            self.prev_stamp = None;
        }
        self.display = stamp.display;

        // Pending until the renderer (`Host::render`) confirms a
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
        let damage = match plan {
            FramePlan::PaintOnly => self.damage_engine.compute_paint_only(
                &self.forest,
                &self.layout.cascades,
                surface,
                prev_time,
                self.time,
            ),
            FramePlan::FullRecord { force_full } => self.damage_engine.compute(
                &self.forest,
                &self.layout.cascades,
                &self.forest.ids.removed,
                surface,
                force_full,
                prev_time,
                self.time,
            ),
        };

        // Skip frames have nothing for the host to submit, so ack
        // here — otherwise `frame_state` stays `Pending` and the next
        // paint frame's `classify_frame` escalates to `Full`.
        if damage.is_none() {
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
    /// `frame_inner`.
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
            self.forest.pre_record();
            // Subscription set is rebuilt from scratch each full record
            // pass — symmetric to `Sense` on a node. Widgets re-assert
            // during record; ones that didn't run drop their wake.
            // Across silent (PaintOnly / skipped) frames the set
            // persists, which is the whole point: a dormant popup
            // needs `BUTTONS` to still be set when the next click
            // outside lands.
            self.input.subs.clear();
        }
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
            record_frame_stats(self);
        }
        self.forest.close_node();
        self.post_record();
        action_flag
    }

    /// Feed a palantir-native input event. Returns an [`InputDelta`]
    /// the host reads to decide whether to request a redraw — pointer
    /// moves over inert surfaces leave `requests_repaint` false so the
    /// host can skip the frame entirely. Animation/tooltip-delay wakes
    /// still drive paints independently via `FrameReport::repaint_after`.
    pub fn on_input(&mut self, event: InputEvent) -> InputDelta {
        self.input.on_input(event, &self.layout.cascades)
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
    /// `Shortcut::key(Key::Escape)`, `Shortcut::cmd('K')`).
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
    pub fn key_pressed(&self, sc: Shortcut) -> bool {
        self.input.frame_keyboard_events.iter().any(|e| match e {
            KeyboardEvent::Down(kp) => sc.matches(*kp),
            _ => false,
        })
    }

    /// Sugar for `key_pressed(Shortcut::key(Key::Escape))`.
    /// Used by [`crate::widgets::context_menu::ContextMenu`] to
    /// dismiss on Esc.
    pub fn escape_pressed(&self) -> bool {
        use crate::input::keyboard::Key;
        self.key_pressed(Shortcut::key(Key::Escape))
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
        self.schedule_wake(deadline, WakeReasons::REAL);
    }

    /// Shared inserter for [`Self::request_repaint_after`] (REAL) and
    /// paint-anim quantum boundaries (ANIM, filed from
    /// [`Self::post_record`]). Maintains the sorted-ascending
    /// invariant on [`Self::repaint_wakes`], coalesces requests within
    /// [`REPAINT_COALESCE_DT`] onto the later deadline, and OR-merges
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
        let near = |existing: Duration| existing.abs_diff(deadline) < REPAINT_COALESCE_DT;
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
        let arena = self.frame_arena.borrow();
        let tc = crate::layout::support::TextCtx {
            bytes: &arena.fmt_scratch,
            shaper: &self.text,
        };
        self.layout_engine.run(
            &self.forest,
            &tc,
            self.display.logical_rect(),
            &mut self.layout,
        );
        drop(arena);
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
        self.forest
            .add_shape(shape, &mut arena, &self.caches.gradients);
    }

    /// Format `args` directly into the per-frame text arena and return
    /// an [`InternedStr::Interned`] handle. Pass the returned value to
    /// any widget that takes `impl Into<InternedStr<'static>>`
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
    pub fn fmt(&mut self, args: std::fmt::Arguments<'_>) -> crate::InternedStr<'static> {
        let mut arena = self.frame_arena.borrow_mut();
        let start = arena.fmt_scratch.len();
        std::fmt::Write::write_fmt(&mut arena.fmt_scratch, args).unwrap();
        let end = arena.fmt_scratch.len();
        let bytes = &arena.fmt_scratch.as_str()[start..end];
        let hash = crate::common::frame_arena::FrameArena::hash_text(bytes);
        crate::InternedStr::Interned {
            span: crate::Span::new(start as u32, (end - start) as u32),
            hash,
        }
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
        self.forest
            .add_shape_animated(shape, anim, &mut arena, &self.caches.gradients);
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
        size: Option<crate::primitives::size::Size>,
        body: impl FnOnce(&mut Ui),
    ) {
        self.forest.push_layer(layer, anchor, size);
        body(self);
        self.forest.pop_layer();
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
        {
            let mut arena = self.frame_arena.borrow_mut();
            self.forest
                .open_node_with_chrome(element, chrome, &mut arena, &self.caches.gradients);
        }
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
    use super::*;
    use crate::FrameStamp;
    use crate::animation::animatable::Animatable;
    use crate::common::frame_arena::FrameArenaHandle;
    use crate::forest::tree::{Layer, NodeId};
    use crate::input::InputEvent;
    use crate::input::pointer::PointerButton;
    use crate::layout::scroll::ScrollLayoutState;
    use crate::primitives::rect::Rect;
    use crate::renderer::frontend::cmd_buffer::RenderCmdBuffer;
    use crate::renderer::frontend::encoder::encode;
    use crate::ui::damage::Damage;
    use crate::ui::damage::region::DamageRegion;
    use crate::ui::frame_report::RenderPlan;
    use glam::{UVec2, Vec2};
    use std::time::Duration;

    impl Ui {
        /// `Ui` with the mono-fallback shaper — predictable 8 px/char widths.
        pub fn for_test() -> Self {
            Self::default()
        }

        /// `Ui` with a thread-shared cosmic shaper (font DB built once per thread).
        pub fn for_test_text() -> Self {
            thread_local! {
                static SHARED: TextShaper = TextShaper::with_bundled_fonts();
            }
            Self::new(
                SHARED.with(|c| c.clone()),
                FrameArenaHandle::default(),
                RenderCaches::default(),
            )
        }

        /// `Ui` pre-stamped with display dimensions; no frame driven yet.
        pub fn for_test_at(size: UVec2) -> Self {
            let mut ui = Self::for_test();
            ui.display = Display::from_physical(size, 1.0);
            ui
        }

        /// `Ui` with cosmic shaper, pre-stamped with display dimensions.
        pub fn for_test_at_text(size: UVec2) -> Self {
            let mut ui = Self::for_test_text();
            ui.display = Display::from_physical(size, 1.0);
            ui
        }

        /// One frame at `size`, time frozen at zero.
        pub fn run_at(&mut self, size: UVec2, record: impl FnMut(&mut Ui)) {
            let display = Display::from_physical(size, 1.0);
            self.frame(FrameStamp::new(display, Duration::ZERO), &mut (), record);
        }

        /// `run_at` then mark the frame as submitted (suppress next-frame auto-rewind to `Full`).
        pub fn run_at_acked(&mut self, size: UVec2, record: impl FnMut(&mut Ui)) {
            self.run_at(size, record);
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
            self.cascades_engine.run(&self.forest, &mut self.layout);
        }

        // ── damage ──────────────────────────────────────────────

        /// Rebuild the post-collapse damage region from `DamageEngine`'s
        /// last-frame pass-1 buffer. Doesn't mutate state.
        pub fn damage_region(&self) -> DamageRegion {
            DamageRegion::collapse_from(&self.damage_engine.raw_rects, self.damage_engine.budget_px)
        }

        /// Damage rects produced by the most recent `post_record`.
        pub fn damage_rect_count(&self) -> usize {
            self.damage_region().iter_rects().count()
        }

        /// Subtree-skip jumps the last damage diff performed.
        pub fn damage_subtree_skips(&self) -> u32 {
            self.damage_engine.subtree_skips
        }

        /// `"skip"` / `"partial"` / `"full"` — the frame's final paint decision.
        pub fn damage_paint_kind(&self) -> &'static str {
            match Damage::new(self.display.logical_rect(), self.damage_region()) {
                Damage::None => "skip",
                Damage::Full => "full",
                Damage::Partial(_) => "partial",
            }
        }

        // ── frame state ─────────────────────────────────────────

        /// Simulate a successful submit so the next frame doesn't auto-rewind to `Full`.
        pub fn mark_frame_submitted(&self) {
            self.frame_state.mark_submitted();
        }

        // ── animation ───────────────────────────────────────────

        /// Animation rows currently allocated for `T`, or 0 if no typed map exists.
        pub fn anim_row_count<T: Animatable>(&mut self) -> usize {
            self.anim.try_typed_mut::<T>().map_or(0, |t| t.rows.len())
        }

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
            let arena = self.frame_arena.borrow();
            encode(self, &arena, plan, &mut cmds);
            cmds
        }
    }
}

#[cfg(test)]
mod tests;
