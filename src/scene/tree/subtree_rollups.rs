//! The two per-node authoring-hash columns a finalized tree carries.

use crate::common::content_hash::ContentHash;

/// Per-node hash columns populated by
/// [`Tree::post_record`](crate::scene::tree::Tree::post_record). Both
/// index by `NodeId.0` and are length `records.len()` afterwards;
/// storage capacity is retained across frames.
///
/// - `node[i]` — authoring hash of node `i` alone (layout / paint /
///   extras / shapes / grid def). Read by damage diff and the leaf
///   intrinsic cache.
/// - `subtree[i]` — rollup of `node[i]` together with the subtree
///   hashes of `i`'s direct children, in declaration order. Equality
///   across frames means nothing in the subtree changed; the
///   cross-frame measure cache keys on this.
///
/// Per-chrome authoring hash lives inline on `ChromeRow.hash` (only
/// chromed nodes pay storage); per-shape canonical hash lives on
/// `Tree.shapes.hashes`. The whole-tree fingerprints are separate —
/// see [`TreeFingerprint`](crate::scene::tree::tree_fingerprint::TreeFingerprint).
#[derive(Debug, Default)]
pub(crate) struct SubtreeRollups {
    pub(crate) node: Vec<ContentHash>,
    pub(crate) subtree: Vec<ContentHash>,
}

impl SubtreeRollups {
    /// Resize both columns for `n` records. Columns are resized with
    /// default values — filled by indexed assignment during the fused
    /// reverse-pre-order pass in `Tree::compute_rollups`.
    pub(crate) fn reset_for(&mut self, n: usize) {
        // Single-pass resize: `compute_rollups` overwrites every slot
        // via indexed assignment, so the fill value is irrelevant —
        // `resize` is preferred over `clear()+resize_with` because it
        // avoids the truncate-then-grow round trip when `n` is steady.
        self.node.resize(n, ContentHash::default());
        self.subtree.resize(n, ContentHash::default());
    }
}
