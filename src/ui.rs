use crate::input::{InputEvent, InputState, PointerState, ResponseState};

use crate::primitives::{Style, WidgetId};
use crate::shape::Shape;
use crate::tree::{LayoutKind, NodeId, Tree};
use std::collections::HashMap;

/// Recorder + input/response broker. Lives across frames; rebuilds the tree each frame
/// while persisting input state via [`InputState`].
///
/// All public coordinate inputs and recorded rects are in **logical pixels** (DIPs).
/// `scale_factor` is the conversion to physical pixels; the renderer applies it at
/// upload time. Pointer events from winit are converted at the boundary
/// (`handle_event` / `InputEvent::from_winit`).
pub struct Ui {
    pub tree: Tree,
    parents: Vec<NodeId>,
    root: Option<NodeId>,

    #[cfg(debug_assertions)]
    seen_ids: HashMap<WidgetId, NodeId>,

    input: InputState,

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
            parents: Vec::new(),
            root: None,
            #[cfg(debug_assertions)]
            seen_ids: HashMap::new(),
            input: InputState::new(),
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

    /// Rebuild input's last-frame rect cache from the just-arranged tree.
    /// Call after `layout::run`.
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

    pub(crate) fn response_for(&self, id: WidgetId) -> ResponseState {
        self.input.response_for(id)
    }

    pub(crate) fn node(
        &mut self,
        id: WidgetId,
        style: Style,
        layout: LayoutKind,
        f: impl FnOnce(&mut Ui),
    ) -> NodeId {
        let parent = self.parents.last().copied();
        let node = self.tree.push_node(id, style, layout, parent);
        #[cfg(debug_assertions)]
        if let Some(prev) = self.seen_ids.insert(id, node) {
            tracing::warn!(
                ?id, ?node, first_seen = ?prev,
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
        let node = *self
            .parents
            .last()
            .expect("add_shape called outside any open node");
        self.tree.add_shape(node, shape);
    }
}
