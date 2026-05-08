pub(crate) mod cascade;
pub(crate) mod damage;
pub(crate) mod seen_ids;
pub(crate) mod state;

use crate::input::{InputEvent, InputState, ResponseState};
use crate::layout::LayoutEngine;
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::display::Display;
use crate::primitives::rect::Rect;
use crate::renderer::frontend::{FrameOutput, Frontend};
use crate::shape::Shape;
use crate::text::{SharedCosmic, TextMeasurer};
use crate::tree::element::Element;
use crate::tree::forest::Forest;
use crate::tree::widget_id::WidgetId;
use crate::tree::{Layer, NodeId};
use crate::ui::cascade::Cascades;
use crate::ui::damage::{Damage, DamagePaint};
use crate::ui::seen_ids::SeenIds;
use crate::ui::state::StateMap;
use crate::widgets::scroll::ScrollRegistry;
use crate::widgets::theme::{Surface, Theme};

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
        }
    }

    /// Install a shared shaper handle. Apps construct one [`SharedCosmic`]
    /// at startup and clone it into both `Ui` and the wgpu backend so they
    /// see the same buffer cache. Tests leave this unset and run on the
    /// deterministic mono fallback.
    pub fn set_cosmic(&mut self, cosmic: SharedCosmic) {
        self.text.set_cosmic(cosmic);
    }

    /// Start recording a frame. A stray `scale_factor` of `0.0` from winit
    /// would collapse the UI to a single physical pixel — assert against it.
    pub fn begin_frame(&mut self, display: Display) {
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
    pub fn end_frame(&mut self) -> FrameOutput<'_> {
        let surface = self.display.logical_rect();
        self.forest.end_frame(surface);
        let removed = self.ids.end_frame();
        self.text.sweep_removed(removed);
        self.layout.sweep_removed(removed);
        self.frontend.sweep_removed(removed);
        self.state.sweep_removed(removed);

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

        FrameOutput { buffer, damage }
    }

    /// Record + finalize a frame, settling state mutations in a single
    /// host call.
    ///
    /// Runs `build` once. If the frame contained input that could have
    /// mutated user state (any click / press / key / text / scroll),
    /// discards the recording, snapshots damage's prev-frame state, and
    /// runs `build` a second time. The second pass sees drained input
    /// queues, so widgets read `clicked() == false` everywhere and the
    /// recording reflects post-mutation state. Only the second pass is
    /// painted.
    ///
    /// Idle frames (animation tick, occlusion change, host repaint
    /// without input) run a single pass — same cost as the bare
    /// `begin_frame` + `end_frame` path.
    ///
    /// `build` runs up to twice per call, so it must be `FnMut`. Most
    /// build closures wrap a free function and trivially satisfy this.
    ///
    /// See `docs/repaint.md` for the full design rationale.
    pub fn run_frame(
        &mut self,
        display: Display,
        mut build: impl FnMut(&mut Ui),
    ) -> FrameOutput<'_> {
        if self.input.take_action_flag() {
            // Discarded pass: only the input drain matters for pass 2
            // (so widgets see `clicked() == false`). Tree state is
            // wiped by pass 2's begin_frame; damage / encode never ran,
            // so `damage.prev` and the render buffer stay at frame-0's
            // values. Sweeps and state evictions are deferred to pass 2
            // and self-correct.
            self.begin_frame(display);
            build(self);
            self.input.drain_per_frame_queues();
        }

        self.begin_frame(display);
        build(self);
        self.end_frame()
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
