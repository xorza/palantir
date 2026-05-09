pub(crate) mod cascade;
pub(crate) mod damage;
pub(crate) mod debug_overlay;
pub(crate) mod seen_ids;
pub(crate) mod state;

use crate::animation::animatable::Animatable;
use crate::animation::{AnimMap, AnimSlot, AnimSpec};
use crate::input::{InputEvent, InputState, ResponseState};
use crate::layout::LayoutEngine;
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::display::Display;
use crate::primitives::rect::Rect;
use crate::renderer::frontend::{FrameOutput, Frontend};
use crate::shape::Shape;
use crate::text::{TextShaper, TextMeasurer};
use crate::tree::element::Element;
use crate::tree::forest::Forest;
use crate::tree::widget_id::WidgetId;
use crate::tree::{Layer, NodeId};
use crate::ui::cascade::Cascades;
use crate::ui::damage::{Damage, DamagePaint};
use crate::ui::debug_overlay::DebugOverlayConfig;
use crate::ui::seen_ids::SeenIds;
use crate::ui::state::StateMap;
use crate::widgets::scroll::ScrollRegistry;
use crate::widgets::theme::{Surface, Theme};
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

    /// Per-frame `WidgetId` tracker — collision detection,
    /// removed-widget diff, and frame rollover. See [`SeenIds`].
    pub(crate) ids: SeenIds,

    /// Cross-frame `WidgetId → Any` widget state. See [`StateMap`].
    pub(crate) state: StateMap,

    pub(crate) text: TextMeasurer,
    pub(crate) layout: LayoutEngine,
    pub(crate) frontend: Frontend,

    pub(crate) input: InputState,
    pub(crate) cascades: Cascades,
    pub(crate) display: Display,

    /// Per-frame damage state. `Damage::compute` returns
    /// [`DamagePaint`] — `Full`, `Partial(rect)`, or `Skip`.
    pub(crate) damage: Damage,

    /// Scroll widgets registered during recording so `end_frame` can
    /// refresh their `ScrollState` rows after arrange.
    // todo move to tree?
    pub(crate) scrolls: ScrollRegistry,

    /// Seconds elapsed since the previous `run_frame`, clamped to
    /// [`MAX_DT`]. Derived from `now - prev_now` per call (not
    /// accumulated across discard passes). Tests that drive frames via
    /// `begin_frame` directly leave this at `0.0` (frozen time).
    pub(crate) dt: f32,

    /// Current frame's host-supplied timestamp (last `now` passed to
    /// [`Self::run_frame`]). Monotonic. Animation rows store an
    /// absolute `Duration` start-time and read this to compute
    /// elapsed-since-start without re-threading `dt`.
    pub(crate) time: Duration,

    /// Set by [`Self::request_repaint`] during recording; copied into
    /// [`FrameOutput::repaint_requested`] at end-of-frame so the host
    /// can re-arm a redraw even when input is idle. Reset at the top
    /// of each `run_frame` (across both discard + paint passes).
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
            ids: SeenIds::default(),
            state: StateMap::default(),
            text: TextMeasurer::default(),
            layout: LayoutEngine::default(),
            frontend: Frontend::default(),
            input: InputState::new(),
            cascades: Cascades::default(),
            display: Display::default(),
            damage: Damage::default(),
            scrolls: ScrollRegistry::default(),
            dt: 0.0,
            time: Duration::ZERO,
            repaint_requested: false,
            anim: AnimMap::default(),
            debug_overlay: None,
        }
    }

    /// Install a shared shaper handle. Apps construct one [`TextShaper`]
    /// at startup and clone it into both `Ui` and the wgpu backend so they
    /// see the same buffer cache. Tests leave this unset and run on the
    /// deterministic mono fallback.
    pub fn set_text_shaper(&mut self, cosmic: TextShaper) {
        self.text.set_text_shaper(cosmic);
    }

    /// Start recording a frame. A stray `scale_factor` of `0.0` from winit
    /// would collapse the UI to a single physical pixel — assert against it.
    pub(crate) fn begin_frame(&mut self, display: Display) {
        assert!(
            display.scale_factor >= f32::EPSILON,
            "Display::scale_factor must be ≥ f32::EPSILON; got {}",
            display.scale_factor,
        );
        self.display = display;
        self.forest.begin_frame();
        self.ids.begin_frame();
        self.scrolls.begin_frame();
    }

    /// Finalize the just-recorded frame: measure + arrange, rebuild cascades
    /// and hit-index, compute hashes and damage, and encode + compose into
    /// the frontend's `RenderBuffer`. Returns the painted output ready for
    /// `WgpuBackend::submit`. Damage's prev-state is committed here on the
    /// assumption that the host will present this frame — see
    /// [`Self::invalidate_prev_frame`] for the rewind path when that
    /// assumption breaks.
    pub(crate) fn end_frame(&mut self) -> FrameOutput<'_> {
        let surface = self.display.logical_rect();
        self.forest.end_frame(surface);
        let removed = self.ids.end_frame();
        self.text.sweep_removed(removed);
        self.layout.sweep_removed(removed);
        self.state.sweep_removed(removed);
        self.anim.sweep_removed(removed);

        let results = self.layout.run(&self.forest, &mut self.text);

        self.scrolls.refresh(&self.forest, results, &mut self.state);

        let cascades = self.cascades.run(&self.forest, results);
        self.input.end_frame(cascades);
        let damage = self
            .damage
            .compute(&self.forest, cascades, removed, surface);

        let damage_filter = match damage {
            DamagePaint::Partial(r) => Some(r),
            DamagePaint::Full | DamagePaint::Skip => None,
        };
        let buffer = self.frontend.build(
            &self.forest,
            results,
            cascades,
            damage_filter,
            &self.display,
        );

        FrameOutput {
            buffer,
            damage,
            repaint_requested: self.repaint_requested,
            debug_overlay: self.debug_overlay,
            cosmic: self.text.cosmic.as_ref(),
        }
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
        self.repaint_requested = false;

        if self.input.take_action_flag() {
            // Discarded pass: only the input drain matters for pass 2
            // (so widgets see `clicked() == false`). Tree state is
            // wiped by pass 2's begin_frame; damage / encode never ran,
            // so `damage.prev` and the render buffer stay at frame-0's
            // values. Sweeps and state evictions are deferred to pass 2
            // and self-correct. `SeenIds`'s rollover swap lives in
            // `end_frame` (NOT `begin_frame`) so this discarded
            // recording doesn't overwrite the last painted frame's
            // `prev`-snapshot — see the doc comment there.
            self.begin_frame(display);
            build(self);
            self.input.drain_per_frame_queues();
        }

        self.begin_frame(display);
        build(self);
        self.end_frame()
    }

    /// Ask the host to schedule another frame even if no input arrives.
    /// Animation tickers call this each frame they haven't settled;
    /// hosts honor the request via [`FrameOutput::repaint_requested`].
    /// Idempotent within a frame.
    pub fn request_repaint(&mut self) {
        self.repaint_requested = true;
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
        slot: AnimSlot,
        target: T,
        spec: Option<AnimSpec>,
    ) -> T {
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
            .tick(id, slot, target, spec, self.dt);
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

    /// Drop damage's prev-frame snapshot so the next `end_frame` is
    /// forced to return `DamagePaint::Full`. Hosts call this when the
    /// last `end_frame`'s output didn't actually reach the swapchain
    /// — failed surface acquire (Occluded, Timeout, Outdated, Lost,
    /// Validation), surface reconfigure, or any other path that
    /// short-circuits `submit` + `present`. Without the rewind, the
    /// next frame's diff would compare against snapshots from a frame
    /// that was never painted and incorrectly return `Skip`.
    pub fn invalidate_prev_frame(&mut self) {
        self.damage.prev_surface = None;
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
        self.input.response_for(id, &self.cascades.result)
    }

    pub(crate) fn node(
        &mut self,
        mut element: Element,
        surface: Option<Surface>,
        f: impl FnOnce(&mut Ui),
    ) -> NodeId {
        assert!(
            element.id != WidgetId::default(),
            "widget recorded without a `WidgetId` — chain `.id_salt(key)`, \
             `.id(precomputed)`, or `.auto_id()` on the builder before `.show(ui)`. \
             `Foo::new()` no longer derives an id automatically.",
        );
        element.id = self.ids.record(element.id, element.auto_id);
        let chrome = surface.map(|s| {
            element.clip = match s.clip {
                ClipMode::Rounded if s.paint.radius.approx_zero() => ClipMode::Rect,
                mode => mode,
            };
            s.paint
        });
        let node = self.forest.open_node(element, chrome);
        f(self);
        self.forest.close_node();
        node
    }

    pub fn add_shape(&mut self, shape: Shape) {
        if shape.is_noop() {
            return;
        }
        self.forest.add_shape(shape);
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
