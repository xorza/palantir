//! Per-layer recording-only state, kept off [`Tree`](crate::scene::tree::Tree) so downstream passes holding `&Tree` cannot reach transient state. Cleared by `Forest::pre_record`.

use crate::primitives::layout::placement::Placement;
use crate::scene::tree::node_id::NodeId;

/// One entry on the recording ancestor stack: the open node's `NodeId` plus precomputed disabled and visibility bits.
#[derive(Clone, Copy, Debug)]
pub(crate) struct OpenFrame {
    pub(crate) node: NodeId,
    pub(crate) ancestor_or_self_disabled: bool,
    pub(crate) effectively_visible: bool,
    /// Paint-arena rows this node's span holds so far: the chrome row (seeded to 1 at open) plus one per direct shape or child, mirroring `cascade::compute_paint_rect` so an animated shape records its row at add time.
    pub(crate) paint_rows: u32,
}

/// Per-layer recording-only state: the ancestor stack and pending root placement. Drained at every top-level `close_node`.
#[derive(Debug, Default)]
pub(crate) struct RecordingScratch {
    /// Ancestor stack for the open scope; empty outside the `pre_record` ↔ root `close_node` window, capacity retained.
    pub(crate) open_frames: Vec<OpenFrame>,

    /// Placement for the active `Forest::push_layer` scope; roots read it without consuming. `Main` falls through to `Placement::default()`.
    pub(crate) pending_placement: Option<Placement>,

    /// Whether the scope the active `Forest::push_layer` was raised from is disabled. A side layer is a tree of its own, so a popup raised in a disabled panel would otherwise be live.
    pub(crate) owner_disabled: bool,
}

impl RecordingScratch {
    pub(crate) fn clear(&mut self) {
        self.open_frames.clear();
        self.pending_placement = None;
        self.owner_disabled = false;
    }

    /// True when any open ancestor in the active scope is `disabled=true`; `cascade.disabled` is one frame stale, so without this an inherited-disabled child paints alive on first appearance.
    #[inline]
    pub(crate) fn ancestor_disabled(&self) -> bool {
        self.open_frames
            .last()
            .map_or(self.owner_disabled, |f| f.ancestor_or_self_disabled)
    }
}
