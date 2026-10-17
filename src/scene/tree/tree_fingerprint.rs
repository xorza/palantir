//! The two whole-tree numbers `CascadeKey::keeps_structure` compares each frame.

use crate::common::content_hash::ContentHash;

/// Whole-tree fingerprints stamped by [`Tree::post_record`](crate::scene::tree::Tree::post_record), read by
/// the cascade's incremental-update gate; one value each per finalized tree, hence beside the per-node
/// [`SubtreeRollups`](crate::scene::tree::subtree_rollups::SubtreeRollups).
#[derive(Debug, Default)]
pub(crate) struct TreeFingerprint {
    /// Hash of what the cascade's structural tables are built from (widget id, nesting, flags, visibility, Tab
    /// order key); geometry and paint stay out, so a change here forces a full rebuild.
    pub(crate) cascade_static: ContentHash,
    /// Counts of stored shapes, chrome rows and nodes folded together, so any move in how many paint rows
    /// nodes emit changes the hash. It lets `CascadeKey::keeps_structure` foresee that the incremental walk, which repairs paint
    /// rows only in place, would bail (a widget adding a shape without moving, e.g. a caret). A conservative
    /// optimization: it can miss or over-fire, and the per-node length check stays the correctness backstop.
    pub(crate) paint_counts: ContentHash,
}
