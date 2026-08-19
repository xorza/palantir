//! The two whole-tree numbers `Cascade::can_update` compares each frame.

use crate::common::content_hash::ContentHash;

/// Whole-tree fingerprints stamped by
/// [`Tree::post_record`](crate::scene::tree::Tree::post_record). Not
/// per-node — one value each per finalized tree — which is why they sit
/// beside the per-node columns
/// ([`SubtreeRollups`](crate::scene::tree::subtree_rollups::SubtreeRollups))
/// rather than inside them. Both are read together by the cascade's
/// incremental-update gate.
#[derive(Debug, Default)]
pub(crate) struct TreeFingerprint {
    /// Tree-wide hash of widget identity, layout, flags, bounds, and
    /// panel inputs, excluding chrome and direct shapes. The cascade
    /// engine pairs it with retained structure and layout-rect
    /// comparisons to identify paint-only changes.
    pub(crate) cascade_static: ContentHash,
    /// How many paint rows this tree's nodes will emit between them, as
    /// three counts folded together: stored shapes, chrome rows, and
    /// nodes.
    ///
    /// The cascade's incremental walk can only repair a node's paint
    /// rows *in place*, so it bails the moment a node's row count moves
    /// — and `cascade_static` deliberately excludes chrome and direct
    /// shapes, precisely so paint-only edits stay on the incremental
    /// path. The gap between those two facts is a widget that adds a
    /// shape without moving (a caret appearing, a focus ring, a hover
    /// highlight): `can_update` waved it through, the walk got partway
    /// and gave up, and the whole cascade was rebuilt anyway. This lets
    /// `can_update` see that case coming.
    ///
    /// A conservative signal, and only ever an optimisation — the walk's
    /// per-node length check stays the correctness backstop. It can miss
    /// (one node gains a shape while another loses one, leaving the
    /// counts level), and it can over-fire (an *invisible* node gaining
    /// chrome bumps the count without emitting a row). Neither changes
    /// the answer the length check reaches — a miss just arrives at it
    /// later, an over-fire pays one wasted rebuild to get there.
    pub(crate) paint_cardinality: u64,
}
