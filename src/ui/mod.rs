pub(crate) mod cascade;
pub(crate) mod damage;
pub(crate) mod seen_ids;
pub(crate) mod state;

use crate::input::{InputEvent, InputState, ResponseState};
use crate::layout::LayoutEngine;
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::display::Display;
use crate::renderer::frontend::{FrameOutput, Frontend};
use crate::shape::Shape;
use crate::text::{SharedCosmic, TextMeasurer};
use crate::tree::element::Element;
use crate::tree::widget_id::WidgetId;
use crate::tree::{NodeId, Tree};
use crate::ui::cascade::Cascades;
use crate::ui::damage::{Damage, DamagePaint};
use crate::ui::seen_ids::SeenIds;
use crate::ui::state::StateMap;
use crate::widgets::scroll::{ScrollNode, ScrollState};
use crate::widgets::theme::{Surface, Theme};

/// The three rendering-pipeline subsystems Ui owns: text shaping →
/// layout measurement/arrangement → frontend encoding/composition.
/// Bundled because they share the `sweep_removed(&[WidgetId])` contract
/// fired once per frame for widgets that vanished — see
/// [`Self::sweep_removed`]. Widget Any-state lives separately on
/// [`Ui::state`] since it's orthogonal to the rendering chain.
#[derive(Default)]
pub(crate) struct Pipeline {
    pub(crate) text: TextMeasurer,
    pub(crate) layout: LayoutEngine,
    pub(crate) frontend: Frontend,
}

impl Pipeline {
    fn sweep_removed(&mut self, removed: &[WidgetId]) {
        self.text.sweep_removed(removed);
        self.layout.sweep_removed(removed);
        self.frontend.sweep_removed(removed);
    }
}

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

    /// Cross-frame `WidgetId → Any` widget state. See [`StateMap`].
    pub(crate) state: StateMap,

    /// Rendering pipeline: text shaping → layout → frontend
    /// encode/compose. See [`Pipeline`].
    pub(crate) pipeline: Pipeline,

    pub(crate) input: InputState,
    pub(crate) cascades: Cascades,
    pub(crate) display: Display,

    /// Per-frame damage state. `Damage::compute` returns
    /// [`DamagePaint`] — `Full`, `Partial(rect)`, or `Skip`.
    pub(crate) damage: Damage,

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
            pipeline: Pipeline::default(),
            input: InputState::new(),
            cascades: Cascades::default(),
            display: Display::default(),
            damage: Damage::default(),
            scroll_nodes: Vec::new(),
        }
    }

    /// Install a shared shaper handle. Apps construct one [`SharedCosmic`]
    /// at startup and clone it into both `Ui` and the wgpu backend so they
    /// see the same buffer cache. Tests leave this unset and run on the
    /// deterministic mono fallback.
    pub fn set_cosmic(&mut self, cosmic: SharedCosmic) {
        self.pipeline.text.set_cosmic(cosmic);
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
    /// `WgpuBackend::submit`. Damage's prev-state is committed here on the
    /// assumption that the host will present this frame — see
    /// [`Self::invalidate_prev_frame`] for the rewind path when that
    /// assumption breaks.
    pub fn end_frame(&mut self) -> FrameOutput<'_> {
        let surface = self.display.logical_rect();
        // Hashes are pure functions of recorded inputs and don't depend on
        // layout output, so we compute them up front. Layout reads them to
        // skip text reshape for unchanged Text nodes; damage reads them after.
        self.tree.end_frame();
        let removed = self.ids.end_frame();
        self.pipeline.sweep_removed(removed);
        self.state.sweep_removed(removed);

        // Disjoint-field reborrow: `layout`, `text`, `frontend` are
        // independent fields of `pipeline`, so we can hold `&mut`s to
        // each in turn (and to `layout` for the rest of the function,
        // since `layout.run` returns a `&LayoutResult` borrow on it).
        let pipeline = &mut self.pipeline;
        let layout = pipeline
            .layout
            .run(&self.tree, self.tree.root(), surface, &mut pipeline.text);

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
            // Record both:
            //  - viewport = INNER (padding-deflated) — what children
            //    see; drives `content > viewport` overflow checks.
            //  - outer = full arranged rect — drives bar positioning
            //    so bars land flush with the OUTER far edge (which
            //    sits inside the reserved strip even with user padding).
            let outer_rect = layout.rect[s.node.index()];
            let pad = self.tree.layout[s.node.index()].padding;
            let outer = outer_rect.size;
            let viewport = outer_rect.deflated_by(pad).size;
            let content = layout.scroll_content[s.node.index()];
            let row = self
                .state
                .get_or_insert_with::<ScrollState, _>(s.id, Default::default);
            row.viewport = viewport;
            row.outer = outer;
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

        // Encoder filter is `Some` only on Partial frames. Full
        // re-encodes everything; Skip will be ignored by `submit`,
        // but we still encode normally so the next non-Skip frame
        // doesn't have stale cache state.
        let damage_filter = match damage {
            DamagePaint::Partial(r) => Some(r),
            DamagePaint::Full | DamagePaint::Skip => None,
        };
        let buffer =
            pipeline
                .frontend
                .build(&self.tree, layout, cascades, damage_filter, &self.display);

        FrameOutput { buffer, damage }
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
        // Apply the surface's clip mode to the element (with Rounded
        // → Rect downgrade for zero-radius paint), and pass the
        // chrome (paint Background) to the tree to land on
        // `extras.chrome`. Element doesn't carry chrome — chrome is
        // a per-node-call concern, paired with the body.
        let chrome = surface.map(|s| {
            element.clip = match s.clip {
                ClipMode::Rounded if s.paint.radius.approx_zero() => ClipMode::Rect,
                mode => mode,
            };
            s.paint
        });
        let node = self.tree.open_node(element, chrome);
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
