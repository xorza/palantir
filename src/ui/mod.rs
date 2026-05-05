pub(crate) mod cascade;
pub(crate) mod damage;
pub(crate) mod seen_ids;
pub(crate) mod state;

use crate::input::{InputEvent, InputState, ResponseState};
use crate::layout::LayoutEngine;
use crate::layout::types::display::Display;
use crate::renderer::frontend::{FrameOutput, Frontend};
use crate::shape::Shape;
use crate::text::{SharedCosmic, TextMeasurer};
use crate::tree::element::Element;
use crate::tree::widget_id::WidgetId;
use crate::tree::{NodeId, Tree};
use crate::ui::cascade::Cascades;
use crate::ui::damage::Damage;
use crate::ui::seen_ids::SeenIds;
use crate::ui::state::StateMap;
use crate::widgets::scroll::{ScrollNode, ScrollState};
use crate::widgets::theme::Theme;

/// Recorder + input/response broker. Lives across frames; rebuilds the tree each frame
/// while persisting input state via [`InputState`].
///
/// All public coordinate inputs and recorded rects are in **logical pixels** (DIPs).
/// `scale_factor` is the conversion to physical pixels; the renderer applies it at
/// upload time. Pointer events from winit are converted at the boundary
/// (`handle_event` / `InputEvent::from_winit`).
pub struct Ui {
    pub(crate) tree: Tree,
    pub theme: Theme,

    /// Per-frame `WidgetId` tracker — collision detection,
    /// removed-widget diff, and frame rollover. See [`SeenIds`].
    pub(crate) ids: SeenIds,

    /// Cross-frame `WidgetId → Any` state. See [`StateMap`].
    pub(crate) state: StateMap,

    pub(crate) input: InputState,
    pub(crate) layout_engine: LayoutEngine,
    pub(crate) cascades: Cascades,
    pub(crate) display: Display,
    pub(crate) text: TextMeasurer,

    /// Defaults to `true` so the first frame always renders. Cleared by
    /// `end_frame`, set by `on_input` / `request_repaint`.
    repaint_requested: bool,

    /// Per-frame damage state. `Damage::compute` returns the filtered
    /// damage rect (`None` ⇒ full repaint, `Some(r)` ⇒ partial).
    pub(crate) damage: Damage,

    pub(crate) frontend: Frontend,

    /// Scroll widgets registered during recording so `end_frame` can
    /// refresh their `ScrollState` rows after arrange. Capacity-retained
    /// across frames, cleared at `begin_frame`.
    pub(crate) scroll_nodes: Vec<ScrollNode>,
}

impl Default for Ui {
    fn default() -> Self {
        Self::new()
    }
}

impl Ui {
    pub fn new() -> Self {
        Self {
            tree: Tree::default(),
            theme: Theme::default(),
            ids: SeenIds::default(),
            state: StateMap::default(),
            input: InputState::new(),
            layout_engine: LayoutEngine::new(),
            cascades: Cascades::default(),
            display: Display::default(),
            text: TextMeasurer::new(),
            // First frame must render so the host has something to
            // present. Subsequent idle frames flip back to `false`.
            repaint_requested: true,
            damage: Damage::default(),
            frontend: Frontend::default(),
            scroll_nodes: Vec::new(),
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
        self.tree.begin_frame();
        self.ids.begin_frame();
        self.scroll_nodes.clear();
    }

    /// Finalize the just-recorded frame: measure + arrange, rebuild cascades
    /// and hit-index, compute hashes and damage, and encode + compose into
    /// the frontend's `RenderBuffer`. Returns the painted output ready for
    /// `WgpuBackend::submit`. Clears the repaint gate.
    pub fn end_frame(&mut self) -> FrameOutput<'_> {
        let surface = self.display.logical_rect();
        // Hashes are pure functions of recorded inputs and don't depend on
        // layout output, so we compute them up front. Layout reads them to
        // skip text reshape for unchanged Text nodes; damage reads them after.
        self.tree.end_frame();
        let removed = self.ids.end_frame();
        self.text.sweep_removed(removed);
        self.layout_engine.sweep_removed(removed);
        self.frontend.sweep_removed(removed);
        self.state.sweep_removed(removed);

        let layout = self
            .layout_engine
            .run(&self.tree, self.tree.root(), surface, &mut self.text);

        // Refresh each registered scroll widget's state row with the
        // freshly-arranged viewport + measured content height. Read here
        // (post-arrange, pre-cascade) so next frame's record clamps with
        // up-to-date numbers; the current frame's pan already used last
        // frame's clamp.
        for s in self.scroll_nodes.iter().copied() {
            assert!(
                s.node.index() < layout.rect.len(),
                "scroll_nodes entry references node {} past tree length {}",
                s.node.index(),
                layout.rect.len(),
            );
            let viewport = layout.rect[s.node.index()].size;
            let content = layout.scroll_content[s.node.index()];
            let row = self
                .state
                .get_or_insert_with::<ScrollState, _>(s.id, Default::default);
            row.viewport = viewport;
            row.content = content;
            // End-frame re-clamp: pairs with the record-time clamp in
            // `Scroll::show`, which only had last frame's numbers.
            let max_x = (content.w - viewport.w).max(0.0);
            let max_y = (content.h - viewport.h).max(0.0);
            row.offset.x = row.offset.x.clamp(0.0, max_x);
            row.offset.y = row.offset.y.clamp(0.0, max_y);
        }

        let cascades = self.cascades.run(&self.tree, layout);
        self.input.end_frame(cascades);
        let damage = self.damage.compute(&self.tree, cascades, removed, surface);

        self.repaint_requested = false;
        let buffer = self
            .frontend
            .build(&self.tree, layout, cascades, damage, &self.display);

        FrameOutput { buffer, damage }
    }

    /// Feed a palantir-native input event. Schedules a repaint
    /// conservatively: every event sets the gate because hover/press
    /// indices, hit-test rects, or response state may shift even when the
    /// event itself looks like a no-op. Refining this needs a hit-test
    /// inside `on_input`.
    pub fn on_input(&mut self, event: InputEvent) {
        self.input.on_input(event, &self.cascades.result);
        self.repaint_requested = true;
    }

    /// True if the UI has changed since the last `end_frame`. Hosts gate
    /// `request_redraw` on this so idle frames cost nothing.
    pub fn should_repaint(&self) -> bool {
        self.repaint_requested
    }

    /// Schedule a repaint on the next host tick. Use for animations, async
    /// state landing, theme changes, or any mutation that doesn't flow
    /// through `on_input`.
    pub fn request_repaint(&mut self) {
        self.repaint_requested = true;
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

    pub(crate) fn response_for(&self, id: WidgetId) -> ResponseState {
        self.input.response_for(id, &self.cascades.result)
    }

    pub(crate) fn node(&mut self, mut element: Element, f: impl FnOnce(&mut Ui)) -> NodeId {
        if !self.ids.record(element.id) {
            assert!(
                element.auto_id,
                "WidgetId collision — id {:?} recorded twice this frame. \
                 Two explicit `.with_id(key)` calls produced the same hash; \
                 pick distinct keys. Duplicate ids silently corrupt focus, \
                 scroll, click capture, and hit-testing.",
                element.id,
            );
            element.id = self.ids.next_dup(element.id);
        }
        let node = self.tree.open_node(element);
        f(self);
        self.tree.close_node();
        node
    }

    pub(crate) fn add_shape(&mut self, shape: Shape) {
        if shape.is_noop() {
            return;
        }
        let node = self
            .tree
            .current_open
            .expect("add_shape called outside any open node");
        self.tree.add_shape(node, shape);
    }
}

#[cfg(test)]
mod tests;
