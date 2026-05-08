//! Recording-only state owned by `Forest`: lives between
//! `Forest::begin_frame` and `Forest::end_frame`, reset once per frame.

use crate::tree::Layer;

/// Recording-only state owned by `Forest`. The active layer selects
/// which `Tree` receives the next `open_node` / `add_shape`. The
/// anchor for the active scope's next root mint lives on the
/// destination tree's `pending_anchor` field — set by
/// `Forest::push_layer`, consumed by the next `Tree::open_node`
/// that mints a `RootSlot`. Per-layer ancestor stacks live on each
/// `Tree` itself.
#[derive(Default)]
pub(crate) struct RecordingState {
    /// Active layer for the next `open_node`. `Main` between/outside
    /// `Ui::layer` scopes; switched by `push_scope` / `pop_scope`.
    pub(crate) current_layer: Layer,
    /// Save-stack: one entry per open `push_scope` — the outer layer
    /// is restored on `pop_scope`. Empty outside any layer scope.
    /// Anchors don't ride the stack because each `Tree` owns its own
    /// `pending_anchor` and same-layer nesting (which would clobber
    /// it) is forbidden by `Forest::push_layer`'s assert.
    layer_stack: Vec<Layer>,
}

impl RecordingState {
    pub(crate) fn reset(&mut self) {
        self.current_layer = Layer::Main;
        self.layer_stack.clear();
    }

    /// Save the active layer and switch to `layer`. The destination
    /// tree's `pending_anchor` is updated separately by the caller.
    pub(crate) fn push_scope(&mut self, layer: Layer) {
        self.layer_stack.push(self.current_layer);
        self.current_layer = layer;
    }

    pub(crate) fn pop_scope(&mut self) {
        self.current_layer = self
            .layer_stack
            .pop()
            .expect("pop_scope called without a matching push_scope");
    }
}
