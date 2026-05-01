mod theme;
pub use theme::Theme;

use crate::element::UiElement;
use crate::input::{InputEvent, InputState, PointerState, ResponseState};
use crate::layout::LayoutEngine;
use crate::primitives::{Rect, Size, WidgetId};
use crate::shape::Shape;
use crate::tree::{NodeId, Tree};
use std::collections::HashSet;

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
    parents: Vec<NodeId>,
    root: Option<NodeId>,

    /// Per-frame collision set: every `WidgetId` that has been recorded this
    /// frame. Cleared in `begin_frame`. Used to enforce id uniqueness — a
    /// repeat insertion in `Ui::node` is a release-`assert!` panic, not a
    /// warning, because duplicate ids silently corrupt every per-id store
    /// (focus, scroll, click capture, hit-test rect lookup).
    seen_ids: HashSet<WidgetId>,

    input: InputState,
    /// Persistent layout engine: holds reusable scratch buffers across frames.
    /// Accessed via `Ui::layout(surface)`.
    layout_engine: LayoutEngine,

    scale_factor: f32,
    pixel_snap: bool,
}

impl Default for Ui {
    fn default() -> Self {
        Self::new()
    }
}

impl Ui {
    pub fn new() -> Self {
        Self {
            tree: Tree::new(),
            theme: Theme::default(),
            parents: Vec::new(),
            root: None,
            seen_ids: HashSet::new(),
            input: InputState::new(),
            layout_engine: LayoutEngine::new(),
            scale_factor: 1.0,
            pixel_snap: true,
        }
    }

    /// Logical→physical conversion factor (e.g. 2.0 on a 2× retina display).
    pub fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    /// Update on `WindowEvent::ScaleFactorChanged` or any DPI change. Clamped to
    /// a non-zero positive value.
    pub fn set_scale_factor(&mut self, scale: f32) {
        self.scale_factor = scale.max(f32::EPSILON);
    }

    /// Whether the renderer snaps rect edges to integer physical pixels.
    /// Default `true` — sharper edges, no half-pixel blur.
    pub fn pixel_snap(&self) -> bool {
        self.pixel_snap
    }

    pub fn set_pixel_snap(&mut self, on: bool) {
        self.pixel_snap = on;
    }

    pub fn begin_frame(&mut self) {
        self.tree.clear();
        self.parents.clear();
        self.root = None;
        self.seen_ids.clear();
    }

    /// Run measure + arrange for the recorded tree at the given surface rect.
    /// Call after recording (post `begin_frame` + widget tree) and before
    /// `end_frame`. Reuses the persistent `LayoutEngine` — amortized
    /// zero-alloc after warmup. The tree is read-only; layout output lands
    /// in `LayoutEngine`.
    pub fn layout(&mut self, surface: Rect) {
        let root = self.root();
        self.layout_engine.run(&self.tree, root, surface);
    }

    /// Rebuild input's last-frame rect cache from the just-arranged tree.
    /// Call after `layout`.
    pub fn end_frame(&mut self) {
        self.input
            .end_frame(&self.tree, self.layout_engine.result());
    }

    pub fn rect(&self, id: NodeId) -> Rect {
        self.layout_engine.rect(id)
    }

    pub fn desired(&self, id: NodeId) -> Size {
        self.layout_engine.desired(id)
    }

    pub fn layout_result(&self) -> &crate::layout::LayoutResult {
        self.layout_engine.result()
    }

    /// Feed a palantir-native input event. Backend-agnostic.
    pub fn on_input(&mut self, event: InputEvent) {
        self.input.on_input(event);
    }

    pub fn pointer(&self) -> PointerState {
        self.input.pointer()
    }

    pub fn root(&self) -> NodeId {
        self.root
            .expect("no root pushed yet — open a node before any other ops")
    }

    /// Borrow the recorded tree. Pass to the renderer pipeline or any other
    /// consumer that needs read access.
    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    pub(crate) fn response_for(&self, id: WidgetId) -> ResponseState {
        self.input.response_for(id)
    }

    pub(crate) fn node(&mut self, element: UiElement, f: impl FnOnce(&mut Ui)) -> NodeId {
        let parent = self.parents.last().copied();
        let id = element.id;
        assert!(
            self.seen_ids.insert(id),
            "WidgetId collision — id {id:?} recorded twice this frame. Use `with_id(key)` (or `WidgetId::with`) to disambiguate widgets at the same call site, e.g. inside a loop. Duplicate ids silently corrupt focus, scroll, click capture, and hit-testing.",
        );
        let node = self.tree.push_node(element, parent);
        if self.root.is_none() {
            self.root = Some(node);
        }
        self.parents.push(node);
        f(self);
        self.parents.pop();
        node
    }

    pub(crate) fn add_shape(&mut self, shape: Shape) {
        if shape.is_noop() {
            return;
        }
        let node = *self
            .parents
            .last()
            .expect("add_shape called outside any open node");
        self.tree.add_shape(node, shape);
    }
}

#[cfg(test)]
mod tests;
