//! Recorded shapes for one tree: the [`Shapes`] buffer and its parallel hash
//! column over the [`ShapeRecord`] variants it holds.

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::primitives::math::nan::NanCheck;
use crate::shape::paint_animation::PaintAnimation;
use crate::shape::record::ShapeRecord;
use std::hash::Hash as _;

/// Per-frame shape-record buffer for one [`crate::scene::tree::Tree`].
///
/// Each node owns a sub-range of `records` via `NodeRecord.shape_span`; the
/// gaps between its children's spans hold its direct shapes in record order
/// ([`crate::scene::tree::iter::TreeItems`] interleaves them). Variable-length
/// payloads live on the `RecordStore` that recorded each; `ShapeRecord`
/// variants reference them by span or id. Cleared per record pass, capacity
/// retained.
#[derive(Debug, Default)]
pub(crate) struct Shapes {
    pub(crate) records: Vec<ShapeRecord>,
    /// Per-shape authoring hash, parallel to `records`, computed once in
    /// [`Self::push`]; `Tree::compute_rollups` only folds it. Keys the per-shape
    /// damage diff in `DamageEngine::compute`. Cleared per frame.
    pub(crate) hashes: Vec<ContentHash>,
}

impl Shapes {
    /// Fold `animation` into shape `idx`'s stored hash, so starting, stopping or
    /// changing an animation reads as a change to every hash-keyed gate.
    pub(crate) fn fold_paint_animation(&mut self, idx: u32, animation: &PaintAnimation) {
        let slot = &mut self.hashes[idx as usize];
        let mut h = Hasher::new();
        slot.hash(&mut h);
        animation.hash_static(&mut h);
        *slot = ContentHash::finish(&h);
    }

    /// Append `record`, a draw the record store recorded, returning its index, which keys
    /// `Forest::animate`'s paint-animation row.
    ///
    /// No NaN reaches here: every shape setter and `Ui::add_shape` check their inputs and panic
    /// on a non-finite one.
    pub(crate) fn push(&mut self, record: ShapeRecord) -> u32 {
        debug_assert!(
            !record.has_nan(),
            "a checked shape lowered to a NaN record: {record:?}",
        );
        let idx = self.records.len() as u32;
        let hash = record.content_hash();
        self.records.push(record);
        self.hashes.push(hash);
        idx
    }
}

#[cfg(test)]
mod tests;
