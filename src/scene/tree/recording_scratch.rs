//! Per-plane recording-only state, kept off [`Tree`](crate::scene::tree::Tree) so downstream passes holding `&Tree` cannot reach transient state. Cleared by `Forest::pre_record`.

use crate::primitives::layout::layout_mode::GridDefId;
use crate::primitives::layout::placement::Placement;
use crate::scene::layer::Layer;
use crate::scene::node::inherited::Inherited;
use crate::scene::tree::node_id::NodeId;

/// One entry on the recording ancestor stack: the open node's `NodeId` plus what it inherits.
#[derive(Clone, Copy, Debug)]
pub(crate) struct OpenFrame {
    pub(crate) node: NodeId,
    /// The node's grid, when it is one, which its children's cells mark as spanning.
    pub(crate) grid: Option<GridDefId>,
    pub(crate) inherited: Inherited,
    /// Paint-arena rows this node's span holds so far: the chrome row (seeded to 1 at open) plus one per direct shape or child, mirroring `cascade::compute_paint_rect` so an animated shape records its row at add time.
    pub(crate) paint_rows: u32,
}

/// What the active `Forest::push_layer` scope gives each root it opens; roots read it without
/// consuming it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PendingRoot {
    /// Where its roots sit on the surface.
    pub(crate) placement: Placement,
    /// The layer the scope asked for, which its roots keep: an overlay nested in a higher layer
    /// records there but stays what it asked to be.
    pub(crate) layer: Layer,
    /// Whether the scope it was raised from is disabled. A side plane is a tree of its own, so a
    /// popup raised in a disabled panel would otherwise be live.
    pub(crate) owner_disabled: bool,
}

/// Per-plane recording-only state: the ancestor stack and the pending overlay root. Drained at every top-level `close_node`.
#[derive(Debug, Default)]
pub(crate) struct RecordingScratch {
    /// Ancestor stack for the open scope; empty outside the `pre_record` ↔ root `close_node` window, capacity retained.
    pub(crate) open_frames: Vec<OpenFrame>,

    /// The active `Forest::push_layer` scope's roots; `None` outside every scope, where a root is
    /// main content filling the surface.
    pub(crate) pending: Option<PendingRoot>,
}

impl RecordingScratch {
    pub(crate) fn clear(&mut self) {
        self.open_frames.clear();
        self.pending = None;
    }

    /// True when any open ancestor in the active scope is `disabled=true`; `cascade.disabled` is one frame stale, so without this an inherited-disabled child paints alive on first appearance.
    #[inline]
    pub(crate) fn ancestor_disabled(&self) -> bool {
        self.open_frames.last().map_or_else(
            || self.pending.is_some_and(|pending| pending.owner_disabled),
            |f| f.inherited.disabled,
        )
    }
}
