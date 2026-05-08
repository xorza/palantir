//! Recording-only state owned by `Forest`: lives between
//! `Forest::begin_frame` and `Forest::end_frame`, reset once per frame.

use crate::primitives::rect::Rect;
use crate::tree::Layer;

/// Recording-only state owned by `Forest`. The active layer selects
/// which `Tree` receives the next `open_node` / `add_shape`. The
/// anchor for the active scope's next root slot rides alongside —
/// stashed on the save-stack at `push_scope` and restored on
/// `pop_scope` so nested scopes can't clobber an outer scope's
/// anchor. Per-layer ancestor stacks live on each `Tree` itself.
#[derive(Default)]
pub(crate) struct RecordingState {
    /// Active layer for the next `open_node`. `Main` between/outside
    /// `Ui::layer` scopes; switched by `push_scope` / `pop_scope`.
    pub(crate) current_layer: Layer,
    /// Anchor for the next root opened in the active scope. `Main`'s
    /// initial value is `Rect::ZERO` (a placeholder); `Forest::end_frame`
    /// patches every existing `Main` root's anchor to the surface rect
    /// once it's known.
    pub(crate) current_anchor: Rect,
    /// Save-stack: one entry per open `push_scope` — both the outer
    /// layer and the outer anchor are restored on `pop_scope`. Empty
    /// outside any layer scope.
    layer_stack: Vec<LayerScope>,
}

#[derive(Clone, Copy, Debug)]
struct LayerScope {
    outer_layer: Layer,
    outer_anchor: Rect,
}

impl RecordingState {
    pub(crate) fn reset(&mut self) {
        self.current_layer = Layer::Main;
        self.current_anchor = Rect::ZERO;
        self.layer_stack.clear();
    }

    /// Save the active layer and anchor, then switch to `layer` with
    /// `anchor` as the anchor for the next root opened in the new
    /// scope.
    pub(crate) fn push_scope(&mut self, layer: Layer, anchor: Rect) {
        self.layer_stack.push(LayerScope {
            outer_layer: self.current_layer,
            outer_anchor: self.current_anchor,
        });
        self.current_layer = layer;
        self.current_anchor = anchor;
    }

    pub(crate) fn pop_scope(&mut self) {
        let outer = self
            .layer_stack
            .pop()
            .expect("pop_scope called without a matching push_scope");
        self.current_layer = outer.outer_layer;
        self.current_anchor = outer.outer_anchor;
    }
}
