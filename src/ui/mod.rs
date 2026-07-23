pub(crate) mod frame;
pub(crate) mod frame_report;
mod frame_stats;
pub(crate) mod resources;
pub(crate) mod state;

use crate::animation::animatable::Animatable;
use crate::animation::{AnimMap, AnimSlot, AnimSpec};
use crate::app::App;
use crate::diagnostics::DebugOverlayConfig;
use crate::display::Display;
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
use crate::layout::types::overlay::OverlayPosition;
use crate::layout::types::sizing::Sizing;
use crate::primitives::approx::EPS;
use crate::primitives::background::Background;
use crate::primitives::image::Image;
use crate::primitives::size::Size;
use crate::primitives::widget_id::WidgetIdMap;
use crate::renderer::frontend::FrameScene;
use crate::renderer::gpu_view::{GpuPaint, GpuPaintRef, GpuViewEntry};
use crate::renderer::image_registry::{ImageHandle, RegisterImageError};
use crate::renderer::plan::RenderPlan;
use crate::scene::Forest;
use crate::scene::element::Element;
use crate::scene::layer::Layer;
use crate::scene::tree::paint_anims::PaintAnim;
use crate::scene::tree::recording::Placement;
use crate::{InternedStr, TextInput};

use crate::primitives::widget_id::WidgetId;
use crate::scene::cascade::{Cascades, CascadesEngine, cascade_fingerprint};
use crate::scene::damage::{Damage, DamageEngine, DamageInput};
use crate::shape::Shape;
use crate::ui::frame::{FrameClassifyInput, FrameInput, FramePlan, FrameRuntime, WakeReasons};
use crate::ui::frame_report::{FrameProcessing, FrameReport};
use crate::ui::resources::UiResources;
use crate::ui::state::StateMap;
use crate::widgets::theme::Theme;
use crate::window::{
    CursorIcon, PendingWindow, WindowConfig, WindowFrameState, WindowGeometry, WindowRequests,
    WindowToken,
};
use glam::UVec2;
use std::cell::{RefCell, RefMut};
use std::collections::hash_map::Entry;
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
    /// App-global capabilities available to the recorder.
    pub(crate) resources: UiResources,
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
    /// Recorder-to-host requests retained across frames.
    pub(crate) window_requests: WindowRequests,
    /// Host-to-recorder facts refreshed before each windowed frame.
    pub(crate) window_frame: WindowFrameState,
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
/// code never calls these directly — `WindowDriver` drives them. The widget
/// authoring API lives in the second `impl Ui` block below.
impl Ui {
    pub(crate) fn frame_scene(&self) -> FrameScene<'_> {
        FrameScene {
            forest: &self.forest,
            layout: &self.layout,
            cascades: &self.cascades,
            payloads: self.forest.record_store.payloads.borrow(),
            text: &self.resources.text,
            gpu_views: &self.gpu_views,
            display: self.display,
            time: self.frame_runtime.time,
        }
    }

    /// Construct a per-window `Ui` from its app-global capabilities. Each `Ui`
    /// creates its own [`Forest`], whose retained record payloads
    /// remain isolated from other windows' record passes.
    pub(crate) fn new(resources: UiResources) -> Self {
        Self {
            resources,
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
            window_requests: Default::default(),
            window_frame: Default::default(),
        }
    }

    /// Drive one application frame for `win`. Runs [`App::update`] once on a
    /// fully recorded frame, then replays [`App::record`] for cold-start
    /// warmup, action input, or `request_relayout`. Paint-only frames skip
    /// both hooks. `stamp.time` is monotonic host time.
    pub(crate) fn frame<T: App>(
        &mut self,
        input: FrameInput,
        win: WindowToken,
        app: &mut T,
    ) -> FrameReport {
        profiling::scope!("Ui::frame");
        let FrameInput {
            stamp,
            damage_baseline_valid,
        } = input;
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
        self.frame_runtime.advance_clock(stamp.time);
        // Refresh the input clock so input handlers running before the
        // next frame timestamp double-clicks on this deterministic time.
        self.input.frame_time = self.frame_runtime.time;
        let plan = self.frame_runtime.classify_frame(FrameClassifyInput {
            display: stamp.display,
            damage_baseline_valid,
            input_policy: self.input_policy,
            had_input: self.input.had_input_since_last_frame,
            input_requested_repaint: self.input.repaint_requested_since_last_frame,
            close_requested: self.window_frame.close_requested,
        });

        self.frame_runtime.repaint_requested = false;
        self.frame_runtime.relayout_requested = false;
        self.display = stamp.display;

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
                app.update(win, self);
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
                    let _ = self.record_pass(win, app);
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
                    self.record_pass(win, app)
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
                    // request — caps relayout at one retry per frame.
                    self.input.drain_per_frame_queues();
                    let _ = self.record_pass(win, app);
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

        // Re-queue the next paint-anim boundary regardless of path.
        // FullRecord rebuilt `paint_anims.entries` during record;
        // PaintOnly retained last frame's. Either way the fold below
        // gives the next quantum boundary — without this, PaintOnly
        // drains the queued ANIM wake without replacing it and the
        // caret freezes until input forces a FullRecord.
        if let Some(min_wake) = self.forest.min_paint_anim_wake(self.frame_runtime.time) {
            self.frame_runtime.schedule_wake(
                min_wake,
                WakeReasons::ANIM,
                self.display.refresh_millihertz,
            );
        }

        self.frame_runtime.prev_stamp = Some(stamp);

        FrameReport {
            repaint_requested: self.frame_runtime.repaint_requested,
            repaint_after: self.frame_runtime.repaint_wakes.first().map(|w| w.deadline),
            plan: RenderPlan::from_damage(damage, self.theme.window_clear),
            processing,
        }
    }

    /// One `pre_record` → user record → drain action flag → `post_record`
    /// cycle. Returns whether the cycle saw action input (which triggers
    /// a second pass in `Ui::frame`).
    fn record_pass<T: App>(&mut self, win: WindowToken, app: &mut T) -> bool {
        {
            profiling::scope!("Ui::pre_record");
            // `forest.pre_record` clears both its trees and the retained
            // payloads their shape records index into.
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
            self.window_requests.cursor = CursorIcon::default();
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
        let mut viewport = Element::zstack();
        viewport.size = Sizing::FILL.into();
        // Hard-coded `WidgetId::VIEWPORT` — a frame-stable parent id,
        // so top-level salts/auto ids resolve to `VIEWPORT.with(salt)`
        // like any other parent-scoped id (see `widget_id`).
        self.forest.open_node(WidgetId::VIEWPORT, viewport, None);
        {
            profiling::scope!("Ui::record_user");
            app.record(win, self);
        }
        let action_flag = self.input.take_action_flag();
        if self.resources.diagnostics.overlay.borrow().frame_stats {
            frame_stats::record(self);
        }
        self.forest.close_node();
        self.post_record();
        action_flag
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
        let payloads = self.forest.record_store.payloads.borrow();
        let text_bytes = payloads.text_bytes();
        let tc = TextCtx {
            bytes: &text_bytes,
            shaper: &self.resources.text,
        };
        self.layout_engine.run(
            &self.forest,
            &tc,
            self.display.logical_rect(),
            &mut self.layout,
        );
        drop(text_bytes);
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
            .run(&self.forest, &self.layout, self.display, &mut self.cascades);
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
        self.resources.text.end_frame();
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
    /// re-record per frame.
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
        self.window_requests.cursor = cursor;
    }

    /// Ask the host to schedule another frame after this one. Cleared
    /// at the top of every `frame`; widgets/showcases that need
    /// continuous animation call this each frame to keep the host
    /// awake.
    pub fn request_repaint(&mut self) {
        tracing::trace!(
            target: "aperture.repaint",
            frame = self.frame_runtime.frame_id,
            "request_repaint",
        );
        self.frame_runtime.repaint_requested = true;
    }

    /// Schedule a one-shot wake at `now + after`. The entry persists
    /// across frames; the frame lifecycle drains entries whose deadline
    /// has fired at the top of each frame. Duplicate deadlines collapse
    /// (sorted + dedup'd), so re-requesting the same wake is a no-op.
    ///
    /// Callers don't need to re-request each frame. To cancel, schedule
    /// nothing else — the wake will fire once, the next frame will run
    /// briefly, and the queue drains.
    pub fn request_repaint_after(&mut self, after: Duration) {
        tracing::trace!(
            target: "aperture.repaint",
            ?after,
            frame = self.frame_runtime.frame_id,
            "request_repaint_after",
        );
        let deadline = self.frame_runtime.time.saturating_add(after);
        self.frame_runtime.schedule_wake(
            deadline,
            WakeReasons::REAL,
            self.display.refresh_millihertz,
        );
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
    /// warning. No-op in headless contexts; the offscreen host discards
    /// the replayed request after rendering.
    ///
    /// `token` is yours to define — an enum discriminant, an index, a
    /// document-id hash. It must be unique across live windows. `config`
    /// is the backend-agnostic [`WindowConfig`] (title + size); the
    /// window inherits the app-global GPU settings from startup.
    pub fn open_window(&mut self, token: WindowToken, config: WindowConfig) {
        if let Some(p) = self
            .window_requests
            .commands
            .opens
            .iter_mut()
            .find(|p| p.token == token)
        {
            p.config = config;
            return;
        }
        self.window_requests
            .commands
            .opens
            .push(PendingWindow { token, config });
    }

    /// Request that the window addressed by `token` close. Deferred like
    /// [`Self::open_window`] — the host removes it after this frame. The
    /// last window closing exits the event loop. No-op if `token` names
    /// no live window, or in headless contexts.
    pub fn close_window(&mut self, token: WindowToken) {
        self.window_requests.commands.closes.push(token);
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
        self.window_frame.close_requested
    }

    /// Veto the auto-close pending from this frame's [`Self::close_requested`].
    /// The window stays open past this frame; close it for real later with
    /// [`Self::close_window`]. A no-op when no close was requested.
    pub fn keep_open(&mut self) {
        self.window_requests.close_vetoed = true;
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
            outer_position: self.window_frame.position,
            maximized: self.window_frame.maximized,
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
        self.resources.diagnostics.overlay.borrow_mut()
    }

    /// Whether a window addressed by `token` is currently live. Reflects
    /// the set as of this frame's *start*, so a window opened or closed
    /// earlier *this* frame isn't reflected until the next one (the host
    /// drains [`Self::open_window`] / [`Self::close_window`] between
    /// frames). Use it as the source of truth for "is this window up?"
    /// instead of mirroring the state in app code — a window the user
    /// closed via its titlebar drops out of this set automatically.
    pub fn window_open(&self, token: WindowToken) -> bool {
        self.resources.windows.contains(token)
    }

    /// Attach a paint primitive to the active node. Direct text contributes to
    /// layout only on a leaf; container-owned text is an overlay shaped against
    /// that container's final padded width.
    pub fn add_shape<'a>(&mut self, shape: impl Into<Shape<'a>>) {
        self.forest.add_shape(shape.into());
    }

    /// Upload an image and get back an owning [`ImageHandle`]. **Hold the
    /// handle** to keep the GPU texture resident — dropping the last
    /// clone frees it; there is no `unregister`. Reference it in
    /// [`Shape::Image`] every frame (`clone` it where it needs to live).
    /// The CPU bytes are dropped right after the upload.
    ///
    /// # Errors
    ///
    /// Returns an error when an image axis exceeds the selected device's 2D
    /// texture limit. A rejected image is never queued for upload. Standalone
    /// CPU recorders have no device limit and retain the original dimensions.
    pub fn register_image(&self, image: Image) -> Result<ImageHandle, RegisterImageError> {
        self.resources.images.register(image)
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
                texture_id: self.resources.texture_ids.reserve(),
                paint: GpuPaintRef(paint),
                epoch: frame_id,
            }),
        };
        self.forest.add_gpu_view(entry.epoch);
    }

    /// Format `args` directly into the record-pass text storage and return
    /// an arena-backed [`InternedStr`]. Pass the returned value to
    /// any text-taking widget. The bytes are already in the destination
    /// buffer, so same-arena lowering is zero-copy and steady-state authoring
    /// of dynamic labels skips per-call `String` allocations.
    ///
    /// Retaining this handle keeps its source arena alive; lowering it in a
    /// later pass or another window copies its exact bytes into that record
    /// store before recording the span. Persistent application text should
    /// stay in its source `String` and be passed to widgets by reference.
    #[must_use]
    pub fn fmt(&mut self, args: std::fmt::Arguments<'_>) -> InternedStr {
        self.forest.record_store.intern_fmt(args)
    }

    /// Normalize borrowed, owned, or already-interned text into an
    /// [`InternedStr`]. Borrowed and owned inputs are copied into the
    /// record-pass text arena; an [`InternedStr`] passes through unchanged.
    /// Format-less twin of [`Self::fmt`] with the same retention rules.
    #[must_use]
    pub fn intern<'a>(&mut self, text: impl Into<TextInput<'a>>) -> InternedStr {
        match text.into() {
            TextInput::Borrowed(text) => self.forest.record_store.intern_str(text),
            TextInput::Owned(text) => self.forest.record_store.intern_str(&text),
            TextInput::Interned(text) => text,
        }
    }

    /// Append `shape` to the active node and register `anim` against
    /// it. The encoder samples `anim` at paint time and folds the
    /// resulting `PaintMod` into the shape's brush; `post_record`
    /// folds the anim's `next_wake` into `repaint_wakes` so the
    /// caller doesn't manage scheduling. Drops silently if the shape
    /// itself was noop-collapsed (zero stroke + transparent fill,
    /// etc.) — `PaintAnim` can't make a zero shape paintable.
    pub(crate) fn add_shape_animated<'a>(&mut self, shape: impl Into<Shape<'a>>, anim: PaintAnim) {
        self.forest.add_shape_animated(shape.into(), anim);
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
        self.placed_layer(layer, Placement::fixed(anchor, size), body);
    }

    pub(crate) fn overlay_layer(
        &mut self,
        layer: Layer,
        position: OverlayPosition,
        body: impl FnOnce(&mut Ui),
    ) {
        self.placed_layer(layer, Placement::overlay(position), body);
    }

    fn placed_layer(&mut self, layer: Layer, placement: Placement, body: impl FnOnce(&mut Ui)) {
        self.forest.push_layer(layer, placement);
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

    /// Mutable peek at an existing cross-frame state row. `None` if
    /// `(id, T)` has never been stored; unlike [`Self::state_mut`], this
    /// does not allocate a typed store or insert a default row.
    pub fn try_state_mut<S: 'static>(&mut self, id: WidgetId) -> Option<&mut S> {
        self.state.try_get_mut::<S>(id)
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
    /// lapses as soon as a record pass stops reading. Use
    /// [`Self::pointer_local`] when the output should be relative to a
    /// widget.
    pub fn pointer_pos(&mut self) -> Option<glam::Vec2> {
        self.subscribe_pointer(PointerSense::MOVE);
        self.input.pointer_pos
    }

    /// Current pointer position in `id`'s pre-transform local logical
    /// coordinates. `None` when the pointer is off-surface or the
    /// widget did not arrange in the previous frame.
    ///
    /// Reading automatically subscribes the record pass to
    /// [`PointerSense::MOVE`], keeping pointer-local paint reactive
    /// while the cursor moves within one hover target.
    pub fn pointer_local(&mut self, id: WidgetId) -> Option<glam::Vec2> {
        self.subscribe_pointer(PointerSense::MOVE);
        self.input.pointer_local_for(id, &self.cascades)
    }

    /// Currently-held modifier keys. State persists across frames; only
    /// `ModifiersChanged` events mutate it.
    ///
    /// Reading automatically subscribes the record pass to
    /// [`KeyboardSense::MODIFIER`], so modifier-dependent paint updates
    /// on both press and release without another input event.
    pub fn modifiers(&mut self) -> Modifiers {
        self.subscribe_keyboard(KeyboardSense::MODIFIER);
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

/// Standalone CPU recorder with isolated resources and mono-fallback text.
///
/// Use [`crate::OffscreenHost`] when frames must be rendered or share
/// resources with a GPU backend.
impl Default for Ui {
    fn default() -> Self {
        Self::new(UiResources::default())
    }
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod test_support;

#[cfg(test)]
mod tests;
