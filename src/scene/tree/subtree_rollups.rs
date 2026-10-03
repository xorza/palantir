//! The per-node authoring-hash columns a finalized tree carries.

use crate::common::content_hash::ContentHash;

/// Per-node hash columns populated by
/// [`Tree::post_record`](crate::scene::tree::Tree::post_record). Each
/// indexes by `NodeId.0` and is length `records.len()` afterwards;
/// storage capacity is retained across frames.
///
/// - `node[i]` — authoring hash of node `i` alone (layout / paint /
///   extras / shapes / grid def). Read by the damage diff.
/// - `subtree[i]` — rollup of `node[i]` together with the subtree
///   hashes of `i`'s direct children, in declaration order. Equality
///   across frames means nothing in the subtree changed; the cascade
///   and the damage diff key on this.
/// - `layout_subtree[i]` — the same rollup over the layout half of each
///   node, the part measure and arrange read. The measure cache and the
///   root intrinsic cache key on this, so a hover tint or a paint
///   animation still hits them. Each node's full hash is built from its
///   layout half, so nothing in this rollup can be missing from
///   `subtree`.
///
/// Per-chrome authoring hash lives inline on `ChromeRow.hash` (only
/// chromed nodes pay storage); per-shape canonical hash lives on
/// `Tree.shapes.hashes`. The whole-tree fingerprints are separate —
/// see [`TreeFingerprint`](crate::scene::tree::tree_fingerprint::TreeFingerprint).
#[derive(Debug, Default)]
pub(crate) struct SubtreeRollups {
    pub(crate) node: Vec<ContentHash>,
    pub(crate) subtree: Vec<ContentHash>,
    pub(crate) layout_subtree: Vec<ContentHash>,
}

impl SubtreeRollups {
    /// Resize the columns for `n` records. Columns are resized with
    /// default values — filled by indexed assignment during the fused
    /// reverse-pre-order pass in `Tree::compute_rollups`.
    pub(crate) fn reset_for(&mut self, n: usize) {
        // Single-pass resize: `compute_rollups` overwrites every slot
        // via indexed assignment, so the fill value is irrelevant —
        // `resize` is preferred over `clear()+resize_with` because it
        // avoids the truncate-then-grow round trip when `n` is steady.
        self.node.resize(n, ContentHash::default());
        self.subtree.resize(n, ContentHash::default());
        self.layout_subtree.resize(n, ContentHash::default());
    }
}
