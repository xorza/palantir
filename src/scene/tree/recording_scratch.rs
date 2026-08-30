//! Per-layer recording-only state, kept off [`Tree`](crate::scene::tree::Tree)
//! so downstream passes holding `&Tree` are type-prevented from reaching
//! transient state — `Tree` itself is the finalized output. Cleared by
//! `Forest::pre_record`.

use crate::layout::types::placement::Placement;
use crate::scene::tree::node_id::NodeId;

/// One entry on the recording ancestor stack
/// ([`RecordingScratch::open_frames`]). Carries the open node's
/// `NodeId` plus precomputed disabled and effective-visibility cascade
/// bits, so inherited state is available during recording without a
/// tree walk. The node's resolved `WidgetId` is read on demand via
/// `records.id[node.idx()]` at the one site that needs it
/// (`Ui::widget`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct OpenFrame {
    pub(crate) node: NodeId,
    pub(crate) ancestor_or_self_disabled: bool,
    pub(crate) effectively_visible: bool,
    /// Paint-arena rows this node's span holds so far: the chrome row
    /// (seeded to 1 when a `ChromeRow` was allocated at open) plus one
    /// per direct shape / immediate child, bumped in record order.
    /// Mirrors the row stream `cascade::compute_paint_rect` emits, so
    /// an animated shape can record its own row index at add time
    /// (`PaintAnimEntry::row`) instead of damage re-deriving it from a
    /// `TreeItems` walk every frame.
    pub(crate) paint_rows: u32,
}

/// Per-layer recording-only state: the ancestor stack and pending root
/// placement — see the module doc for why it lives here rather than on
/// `Tree`. Drained at every top-level `close_node`.
#[derive(Debug, Default)]
pub(crate) struct RecordingScratch {
    /// Ancestor stack for the currently-open scope. Empty outside the
    /// `pre_record` ↔ root `close_node` window. Capacity retained across
    /// frames.
    ///
    /// Each frame carries precomputed disabled and effective-visibility
    /// cascade bits. That makes inherited state a one-node load
    /// instead of an O(depth) walk.
    pub(crate) open_frames: Vec<OpenFrame>,

    /// Placement for the active `Forest::push_layer` scope. Root mints
    /// inside the scope read it without consuming it because multiple
    /// roots can share the policy. `Main` falls through to
    /// `Placement::default()`.
    pub(crate) pending_placement: Option<Placement>,
}

impl RecordingScratch {
    pub(crate) fn clear(&mut self) {
        self.open_frames.clear();
        self.pending_placement = None;
    }

    /// True when any currently-open ancestor in the active recording
    /// scope has `disabled=true`. Lets widgets see inherited-disabled
    /// at record time, in the *same* frame the ancestor was opened —
    /// `cascade.disabled` is one frame stale, so without this an
    /// inherited-disabled child paints alive on first appearance and
    /// then animates to disabled. O(1): the bit is propagated on
    /// `open_node` push, so `last()` already encodes the OR over the
    /// whole open chain.
    #[inline]
    pub(crate) fn ancestor_disabled(&self) -> bool {
        self.open_frames
            .last()
            .is_some_and(|f| f.ancestor_or_self_disabled)
    }
}
