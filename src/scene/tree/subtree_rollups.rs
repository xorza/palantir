//! The per-node authoring-hash columns a finalized tree carries.

use crate::common::content_hash::ContentHash;

/// Per-node hash columns filled by
/// [`Tree::post_record`](crate::scene::tree::Tree::post_record), indexed by `NodeId.0`; capacity is retained across frames.
///
/// - `node[i]`: authoring hash of node `i` alone; read by the damage diff.
/// - `subtree[i]`: `node[i]` rolled up with its children's subtree hashes. Equal across frames means nothing below changed; cascade and damage key on it.
/// - `layout_subtree[i]`: the same rollup over the layout half only, keying the measure and root intrinsic caches so hover tints and paint animations still hit.
///
/// Chrome and shape hashes live on `ChromeRow.hash` and `Tree.shapes.hashes`; whole-tree fingerprints are
/// [`TreeFingerprint`](crate::scene::tree::tree_fingerprint::TreeFingerprint).
#[derive(Debug, Default)]
pub(crate) struct SubtreeRollups {
    pub(crate) node: Vec<ContentHash>,
    pub(crate) subtree: Vec<ContentHash>,
    pub(crate) layout_subtree: Vec<ContentHash>,
}

impl SubtreeRollups {
    /// Resize the columns for `n` records, default-filled; `Tree::compute_rollups` overwrites every slot.
    pub(crate) fn reset_for(&mut self, n: usize) {
        // Every slot is overwritten, so `resize` beats `clear()+resize_with` when `n` is steady.
        self.node.resize(n, ContentHash::default());
        self.subtree.resize(n, ContentHash::default());
        self.layout_subtree.resize(n, ContentHash::default());
    }
}
