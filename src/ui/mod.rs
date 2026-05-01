mod theme;
pub use theme::Theme;

use crate::element::UiElement;
use crate::input::{InputEvent, InputState, PointerState, ResponseState};
use crate::layout::LayoutEngine;
use crate::primitives::{Rect, WidgetId};
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

    #[cfg(debug_assertions)]
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
            #[cfg(debug_assertions)]
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
        #[cfg(debug_assertions)]
        self.seen_ids.clear();
    }

    /// Run measure + arrange for the recorded tree at the given surface rect.
    /// Call after recording (post `begin_frame` + widget tree) and before
    /// `end_frame`. Reuses the persistent `LayoutEngine` — amortized
    /// zero-alloc after warmup.
    pub fn layout(&mut self, surface: Rect) {
        let root = self.root();
        self.layout_engine.run(&mut self.tree, root, surface);
    }

    /// Rebuild input's last-frame rect cache from the just-arranged tree.
    /// Call after `layout`.
    pub fn end_frame(&mut self) {
        self.input.end_frame(&self.tree);
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

    /// Mutably borrow the recorded tree. `LayoutEngine::run` needs `&mut Tree`
    /// to fill in `desired` and `rect`. Most callers should use `Ui::layout`
    /// instead, which uses the persistent layout engine.
    pub fn tree_mut(&mut self) -> &mut Tree {
        &mut self.tree
    }

    pub(crate) fn response_for(&self, id: WidgetId) -> ResponseState {
        self.input.response_for(id)
    }

    pub(crate) fn node(&mut self, element: UiElement, f: impl FnOnce(&mut Ui)) -> NodeId {
        let parent = self.parents.last().copied();
        let id = element.id;
        let node = self.tree.push_node(element, parent);
        #[cfg(debug_assertions)]
        if !self.seen_ids.insert(id) {
            tracing::warn!(
                ?id,
                ?node,
                "WidgetId collision — use `with_id(...)` to disambiguate"
            );
        }
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
