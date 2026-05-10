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
use crate::input::{InputEvent, InputState, ResponseState};
use crate::layout::LayoutEngine;
use crate::layout::scroll::ScrollLayoutState;
use crate::layout::types::display::Display;
use crate::primitives::mesh::Mesh;
use crate::primitives::rect::Rect;
use crate::renderer::frontend::{FrameOutput, FrameState, Frontend};
use crate::shape::Shape;
use crate::text::TextShaper;
use crate::ui::cascade::Cascades;
use crate::ui::damage::{Damage, DamagePaint};
use crate::ui::debug_overlay::DebugOverlayConfig;
use crate::ui::state::StateMap;
use crate::widgets::theme::Theme;
use std::time::Duration;

/// Hard upper bound on per-frame `dt` derived from `now` deltas in
/// [`Ui::run_frame`]. Anything longer (debugger pause, laptop suspend,
/// dropped vsync) is clamped so animation tickers freeze for one frame
/// instead of teleporting through state. `Ui::time` still tracks the
/// host's true clock; only `Ui::dt` is clamped.
pub(crate) const MAX_DT: f32 = 0.1;

/// Recorder + input/response broker. Lives across frames; rebuilds the tree each frame
/// while persisting input state via [`InputState`].
///
/// All public coordinate inputs and recorded rects are in **logical pixels** (DIPs).
/// `scale_factor` is the conversion to physical pixels; the renderer applies it at
/// upload time. Pointer events from winit are converted at the boundary
/// (`handle_event` / `InputEvent::from_winit`).
pub struct Ui {
    pub(crate) forest: Forest,
    pub theme: Theme,

    /// Cross-frame `WidgetId → Any` widget state. See [`StateMap`].
    pub(crate) state: StateMap,

    pub(crate) text: TextShaper,
    pub(crate) layout: LayoutEngine,
    pub(crate) frontend: Frontend,

    pub(crate) input: InputState,
    pub(crate) cascades: Cascades,
    pub(crate) display: Display,

    /// Per-frame damage state. `Damage::compute` returns
    /// [`DamagePaint`] — `Full`, `Partial(rect)`, or `Skip`.
    pub(crate) damage: Damage,

    /// Seconds elapsed since the previous `run_frame`, clamped to
    /// [`MAX_DT`]. Derived from `now - prev_now` per call (not
    /// accumulated across discard passes). Tests that drive frames via
    /// `begin_frame` directly leave this at `0.0` (frozen time).
    pub(crate) dt: f32,

    /// Monotonically increasing per-[`Self::run_frame`] counter. Bumped
    /// once at the entry to `run_frame`, *before* the (up to two)
    /// record passes. Animation rows tag the frame they last advanced
    /// in so a second `tick` from pass B doesn't double-step the
    /// integrator — pass B reaches a target update, but the dt-driven
    /// advance only fires once per real frame.
    pub(crate) frame_id: u64,

    /// Current frame's host-supplied timestamp (last `now` passed to
    /// [`Self::run_frame`]). Monotonic. Animation rows store an
    /// absolute `Duration` start-time and read this to compute
    /// elapsed-since-start without re-threading `dt`.
    pub(crate) time: Duration,

    /// Set by [`Self::animate`] each frame an animation hasn't
    /// settled; copied into [`FrameOutput::repaint_requested`] at
    /// end-of-frame so the host can re-arm a redraw even when input
    /// is idle. Reset at the top of each `run_frame` (across both
    /// discard + paint passes).
    pub(crate) repaint_requested: bool,

    /// Per-`(WidgetId, AnimSlot)` animation rows. Read/written via
    /// [`Self::animate`]; evicted on the same `removed` sweep as
    /// `StateMap` / text / layout caches.
    pub(crate) anim: AnimMap,

    /// Per-frame debug overlay config. `None` disables the subsystem
    /// entirely; `Some(config)` enables the flagged visualizations.
    /// Copied into [`FrameOutput`] so the wgpu backend draws the
    /// requested overlays onto the swapchain after the
    /// backbuffer→surface copy.
    pub debug_overlay: Option<DebugOverlayConfig>,

    /// Self-healing frame-lifecycle state. Cloned into each
    /// [`FrameOutput`]; `WgpuBackend::submit` marks `Submitted` on
    /// success. The next [`Self::begin_frame`] auto-rewinds
    /// `damage.prev_surface` if the previous frame's state isn't
    /// `Submitted` — i.e. the host dropped or skipped a
    /// `FrameOutput`. Without this, `Damage.prev` would roll forward
    /// against an unpainted backbuffer and partial-repaint the next
    /// frame would smear.
    pub(crate) frame_state: FrameState,

    /// Set by [`Self::request_relayout`] when a widget realizes (after
    /// measure) that its record-time decisions used stale state and
    /// the frame should be re-recorded. `run_frame` consumes this
    /// after the record phase: if true, it discards pass A, runs
    /// `build` again, re-runs the record phase, then paints. Capped
    /// at one re-record per `run_frame` — the second pass paints
    /// regardless of what it requests.
    pub(crate) relayout_requested: bool,
}

impl Default for Ui {
    fn default() -> Self {
        Self::new()
    }
}

impl Ui {
    pub fn new() -> Self {
        Self {
            forest: Forest::default(),
            theme: Theme::default(),
            state: StateMap::default(),
            text: TextShaper::default(),
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

    /// Install a shared shaper handle. Apps construct one [`TextShaper`]
    /// at startup and clone it into both `Ui` and the wgpu backend so they
    /// see the same buffer cache. Tests leave this unset and run on the
    /// deterministic mono fallback.
    pub fn set_text_shaper(&mut self, shaper: TextShaper) {
        self.text = shaper;
    }

    /// Start recording a frame. A stray `scale_factor` of `0.0` from winit
    /// would collapse the UI to a single physical pixel — assert against it.
    ///
    /// Single detection point for "the world changed since last
    /// `end_frame`". Three triggers all funnel into the same reset
    /// (clear `damage.prev`, set `prev_surface = None`):
    ///
    /// 1. **Display changed** — host passed a different size or scale
    ///    than last frame.
    /// 2. **Frame skipped** — previous `FrameOutput` wasn't marked
    ///    `Submitted` (surface acquire failed, host dropped, panic in
    ///    error arm).
    /// 3. **First frame** — `prev_surface` is `None` by default.
    ///
    /// `Damage::compute` reads the post-reset state (`prev_surface ==
    /// None`) and short-circuits to `DamagePaint::Full`. The
    /// auto-rewind covers the surface-error and dropped-frame cases,
    /// so hosts don't need any explicit "invalidate" call.
    ///
    /// `frame_state` is **not** reset here. It encodes the submission
    /// status of the *last painted* frame (`mark_pending` in
    /// `end_frame`, `mark_submitted` by the host or by the `Skip`
    /// path) — strictly cross-frame. `run_frame`'s discarded pre-pass
    /// calls `begin_frame` a second time without an intervening
    /// `end_frame`; touching `frame_state` here would make pass 2
    /// misread that as "host dropped pass 1's frame" and force `Full`
    /// repaint on every input event (including clicks on empty bg).
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
        // Drop any leftover relayout request from a previous frame's
        // pass A that didn't get consumed. Belt-and-suspenders —
        // current `run_frame` always consumes via `mem::replace`, but
        // a future restructure that adds new entry points shouldn't
        // smuggle a stale flag across frames.
        self.relayout_requested = false;
    }

    /// Record-derived half of the frame lifecycle: finalize per-node hashes,
    /// diff against the last painted frame's seen-ids, sweep evicted
    /// state, run measure/arrange, refresh per-widget state rows.
    /// Returns `true` when a widget called [`Self::request_relayout`]
    /// during refresh — the caller (ie. `run_frame`) should discard
    /// this pass, re-record, and run the record phase again before
    /// paint.
    ///
    /// `SeenIds::diff_for_sweep` (NOT `commit_rollover`) is what runs
    /// here — the rollover commit is paint-phase work. Diffing without
    /// committing means a discarded pass A still has access to the
    /// true last-painted frame as its `prev` reference, so `removed`
    /// is correct in every pass and damage always diffs against the
    /// painted frame, not against pass A's discarded tree.
    pub(crate) fn end_frame_record_phase(&mut self) -> bool {
        let surface = self.display.logical_rect();
        self.forest.end_frame(surface);
        // Diff vs last-painted, sweep caches BEFORE layout.run so the
        // measure cache compaction sees a consistent live-set.
        self.forest.ids.diff_for_sweep();
        let removed = &self.forest.ids.removed;
        self.text.sweep_removed(removed);
        self.layout.sweep_removed(removed);
        self.state.sweep_removed(removed);
        self.anim.end_frame(removed);

        self.layout.run(&self.forest, &self.text);

        std::mem::replace(&mut self.relayout_requested, false)
    }

    /// Paint-derived half of `end_frame`: commit the seen-id rollover
    /// (this pass becomes the next frame's `prev`), then cascade /
    /// hit-index / damage diff / encode / compose. Reads the
    /// `LayoutResult` produced by the most recent
    /// [`Self::end_frame_record_phase`] call (still owned by
    /// `self.layout`). Damage is computed exactly once per frame.
    pub(crate) fn end_frame_paint_phase(&mut self) -> FrameOutput<'_> {
        let surface = self.display.logical_rect();
        // record_phase already populated `forest.ids.removed` against
        // the still-untouched last-painted `prev`. Commit the rollover
        // here — pass A's `removed` survives via the field; we just
        // need to slide curr → prev for next frame's diff.
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

    /// Signal that record-time decisions made this pass were based on
    /// stale measure-time state, and the framework should re-record
    /// the frame after measure has run with fresh inputs. Capped at
    /// one re-record per `run_frame` — calling this during the second
    /// pass is a no-op (paints anyway, accepting one frame of settle).
    pub fn request_relayout(&mut self) {
        self.relayout_requested = true;
    }

    /// Record + finalize a frame, settling state mutations in a single
    /// host call. The only public entry point for driving a frame —
    /// hosts call this once per redraw.
    ///
    /// `now` is monotonic time since a host-defined epoch (typically
    /// `Instant::now() - start_instant`, i.e. `start.elapsed()`). `Ui`
    /// stores it as [`Self::time`] and derives [`Self::dt`] (clamped to
    /// [`MAX_DT`] seconds) for animation tickers. The first call's
    /// `dt` is `now - Duration::ZERO` clamped — pass `Duration::ZERO`
    /// or a freshly-captured `start.elapsed()` to keep it small.
    ///
    /// Runs `build` once. If the frame contained input that could have
    /// mutated user state (any click / press / key / text / scroll),
    /// discards the recording, snapshots damage's prev-frame state, and
    /// runs `build` a second time. The second pass sees drained input
    /// queues, so widgets read `clicked() == false` everywhere and the
    /// recording reflects post-mutation state. Only the second pass is
    /// painted. `now` is applied once across both passes — the
    /// discarded pass observes the same clock the painted pass does.
    ///
    /// Idle frames (animation tick, occlusion change, host repaint
    /// without input) run a single pass.
    ///
    /// `build` runs up to twice per call, so it must be `FnMut`. Most
    /// build closures wrap a free function and trivially satisfy this.
    ///
    /// See `docs/repaint.md` for the full design rationale.
    pub fn run_frame(
        &mut self,
        display: Display,
        now: Duration,
        mut build: impl FnMut(&mut Ui),
    ) -> FrameOutput<'_> {
        let raw_dt = now.saturating_sub(self.time);
        self.dt = raw_dt.as_secs_f32().min(MAX_DT);
        self.time = now;
        // Bump frame_id once per host frame, NOT per pass. Pass B's
        // anim ticks see the same id as pass A's and short-circuit the
        // integrator step instead of double-advancing.
        self.frame_id = self.frame_id.wrapping_add(1);
        self.repaint_requested = false;

        // Pass A: record + measure + refresh. We always run the
        // record phase here so widgets that called
        // `Ui::request_relayout` get their signal picked up. If pass
        // A also drains an input action OR a relayout was requested,
        // pass B runs (record again with the post-drain / post-flag
        // state, then paint). Otherwise pass A's record carries
        // straight into the paint phase below.
        self.begin_frame(display);
        build(self);
        let action_flag = self.input.take_action_flag();
        let needs_relayout = self.end_frame_record_phase();
        if action_flag {
            self.input.drain_per_frame_queues();
        }
        if action_flag || needs_relayout {
            // Pass B. Re-record with drained input / corrected state,
            // re-run record phase. This is also a hard cap on
            // relayout — pass B's `request_relayout` is ignored.
            self.begin_frame(display);
            build(self);
            self.end_frame_record_phase();
            self.relayout_requested = false;
        }
        self.end_frame_paint_phase()
    }

    /// Advance an animation row keyed by `(id, slot)`, returning the
    /// current interpolated value. Generic over `T: Animatable` —
    /// implemented for `f32`, `Vec2`, `Color`. First touch snaps
    /// `current = target` (no animation on appearance). Subsequent
    /// calls detect retarget and ease/spring toward the new target,
    /// requesting a repaint each frame until settled.
    ///
    /// `slot` lets a single widget animate multiple values
    /// independently (hover, press, focus, custom). Define slots as
    /// `const` next to the widget's state struct.
    ///
    /// See `src/animation/animations.md` for the full design.
    /// Advance an animation row keyed by `(id, slot)`, returning the
    /// current interpolated value. Generic over `T: Animatable`
    /// (`f32`, `Vec2`, `Color`).
    ///
    /// `spec`:
    /// - `Some(s)` — tick toward `target` per spec; ease/spring math,
    ///   first-touch snap, retarget detection, settle check, repaint
    ///   request all run as expected.
    /// - `None` — snap to `target`, drop any stale row, do **not**
    ///   request a repaint. `None` is the API-level signal "this
    ///   caller didn't ask for motion" (typically from
    ///   `theme.button.anim: Option<AnimSpec>`).
    ///
    /// Slot lets one widget animate multiple values independently;
    /// declare slot consts next to the widget's state struct.
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

    /// Feed a palantir-native input event. Hosts mirror this with their
    /// own redraw-scheduling — palantir doesn't track a repaint gate,
    /// since whether to call `window.request_redraw()` is a host
    /// concern (winit ↔ ui boundary).
    pub fn on_input(&mut self, event: InputEvent) {
        self.input.on_input(event, &self.cascades.result);
    }

    /// Borrow the cross-frame state row for `id`, creating it via
    /// `T::default()` on first access. Use for scroll offset, focus
    /// flags, animation progress — anything keyed to a `WidgetId` that
    /// must survive between frames. Rows are dropped at `end_frame`
    /// for any `WidgetId` that wasn't recorded this frame.
    ///
    /// Panics if a row already exists for `id` with a different `T`
    /// — that's a `WidgetId` collision, not a runtime condition.
    pub fn state_mut<T: Default + 'static>(&mut self, id: WidgetId) -> &mut T {
        self.state.get_or_insert_with(id, T::default)
    }

    /// Mutable access to the scroll state row for the widget at
    /// `id`. Inserts a default row on first access. The widget
    /// reads/writes the snapshot at record time (offset clamp,
    /// reservation guess, bar geometry); the layout's scroll driver
    /// writes the layout-derived fields during measure + arrange.
    /// State lives on [`LayoutEngine::scroll_states`] (not `StateMap`)
    /// so the layout subsystem owns its own concern.
    ///
    /// Keyed internally by the inner viewport's id (`id.with("__viewport")`)
    /// because that's the WidgetId the layout subsystem sees on the
    /// `LayoutMode::Scroll` node — callers stay on the public outer
    /// id and this hop is invisible.
    pub(crate) fn scroll_state(&mut self, id: WidgetId) -> &mut ScrollLayoutState {
        self.layout.scroll_states.entry(id).or_default()
    }

    /// Currently focused widget id, or `None`. Read by editable widgets
    /// to decide whether to drain `frame_keys` / `frame_text` this frame.
    pub fn focused_id(&self) -> Option<WidgetId> {
        self.input.focused
    }

    /// Programmatically set or clear focus. Use for autofocus on mount
    /// (`Some(id)`) and for explicit dismissal like Escape-to-blur
    /// (`None`). Bypasses [`crate::FocusPolicy`] — both policies allow
    /// programmatic clear.
    pub fn request_focus(&mut self, id: Option<WidgetId>) {
        self.input.focused = id;
    }

    /// Set the press-on-non-focusable behavior. See [`crate::FocusPolicy`].
    pub fn set_focus_policy(&mut self, p: crate::FocusPolicy) {
        self.input.focus_policy = p;
    }

    pub fn focus_policy(&self) -> crate::FocusPolicy {
        self.input.focus_policy
    }

    pub(crate) fn response_for(&self, id: WidgetId) -> ResponseState {
        let mut state = self.input.response_for(id, &self.cascades.result);
        // Cascade lags by a frame; OR in any open ancestor's
        // `disabled=true` from this frame's recording so a widget
        // appearing inside a freshly-disabled subtree paints disabled
        // on its first frame instead of animating from alive.
        state.disabled |= self.forest.ancestor_disabled();
        state
    }

    pub(crate) fn node(&mut self, element: Element, f: impl FnOnce(&mut Ui)) -> NodeId {
        // Pure plumbing. `Forest::open_node` resolves the widget id
        // (collision detect + auto-id disambiguation) and pushes
        // chrome/clip/radius into their per-tree columns; `Configure`
        // setters already populated everything else on `element`.
        let node = self.forest.open_node(element);
        f(self);
        self.forest.close_node();
        node
    }

    pub fn add_shape(&mut self, shape: Shape<'_>) {
        if shape.is_noop() {
            return;
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
            tint: crate::primitives::color::Color {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            },
        });
    }

    /// Record `body` as a side layer — its first widget becomes a new
    /// root tagged with `layer`, anchored at `anchor` (caller-supplied
    /// screen rect, typically a trigger widget's last-frame
    /// `Response.state.rect`).
    ///
    /// Must be called at top-level recording (no node currently open).
    /// Records pre-order contiguity is load-bearing for child iteration
    /// — interleaving a popup mid-`Panel::show` would split the
    /// surrounding panel's subtree across non-adjacent index ranges.
    /// V1 requires the egui-style pattern: record `Main` content first,
    /// then call `ui.layer(...)` after the outer scope closes.
    pub fn layer<R>(&mut self, layer: Layer, anchor: Rect, body: impl FnOnce(&mut Ui) -> R) -> R {
        self.forest.push_layer(layer, anchor);
        let result = body(self);
        self.forest.pop_layer();
        result
    }
}

#[cfg(test)]
mod tests;
