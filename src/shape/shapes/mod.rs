//! Recorded shapes for one tree: the [`Shapes`] buffer and its parallel hash
//! column over the [`ShapeRecord`] variants it holds.

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::image::{ImageDownsample, ImageFilter, ImageFit};
use crate::shape::Lower;
use crate::shape::paint::image_source::ImageSource;
use crate::shape::paint_animation::PaintAnimation;
use crate::shape::record::ShapeRecord;
use crate::shape::record_store::RecordStore;
use crate::shape::sealed::Resolve;
use std::hash::Hash as _;

/// Per-frame shape-record buffer for one [`crate::scene::tree::Tree`].
///
/// Each node owns a sub-range of `records` via `NodeRecord.shape_span`; the
/// gaps between its children's spans hold its direct shapes in record order
/// ([`crate::scene::tree::iter::TreeItems`] interleaves them). Variable-length
/// payloads live on the `RecordStore` passed to [`Self::add`]; `ShapeRecord`
/// variants reference them by span or id. Cleared per record pass, capacity
/// retained.
#[derive(Debug, Default)]
pub(crate) struct Shapes {
    pub(crate) records: Vec<ShapeRecord>,
    /// Per-shape authoring hash, parallel to `records`, computed once in
    /// [`Self::add`]; `Tree::compute_rollups` only folds it. Keys the per-shape
    /// damage diff in `DamageEngine::compute`. Cleared per frame.
    pub(crate) hashes: Vec<ContentHash>,
}

impl Shapes {
    /// Lower a shape and its paint, their names resolved in `resolve`, and append the record to
    /// `records`; variable-length payloads land on the [`RecordStore`].
    ///
    /// Drops a pair whose authoring inputs would emit no pixels **as it lowers** (tier 2; see
    /// [`paint_sink`](crate::renderer::frontend::paint_sink)), saving the cost the emit-time gate
    /// can't. Returns the index of the pushed `ShapeRecord`, or `None` if dropped; the index keys
    /// `Forest::add_shape`'s paint-anim row.
    ///
    /// No NaN reaches here: every shape setter and `Ui::add_shape` check their inputs and panic
    /// on a non-finite one.
    #[track_caller]
    pub(crate) fn add<S: Lower>(
        &mut self,
        shape: S,
        resolve: Resolve<'_>,
        store: &mut RecordStore,
    ) -> Option<u32> {
        let record = shape.lower(resolve, store)?;
        debug_assert!(
            !record.has_nan(),
            "a checked shape lowered to a NaN record: {record:?}",
        );
        Some(self.push(record))
    }

    /// Fold `animation` into shape `idx`'s stored hash, so starting, stopping or
    /// changing an animation reads as a change to every hash-keyed gate.
    pub(crate) fn fold_paint_animation(&mut self, idx: u32, animation: &PaintAnimation) {
        let slot = &mut self.hashes[idx as usize];
        let mut h = Hasher::new();
        slot.hash(&mut h);
        animation.hash_static(&mut h);
        *slot = ContentHash::finish(&h);
    }

    /// Append an already-lowered record, returning its index.
    fn push(&mut self, record: ShapeRecord) -> u32 {
        let idx = self.records.len() as u32;
        let hash = record.content_hash();
        self.records.push(record);
        self.hashes.push(hash);
        idx
    }

    /// Append an [`ImageSource::GpuView`]-sourced [`ShapeRecord::Image`] directly,
    /// bypassing [`Self::add`] lowering. The view's `paint` lives in `Ui::gpu_views`
    /// by the owner's `WidgetId`; the record carries its target `id` and `epoch`.
    ///
    /// Placement is neutral: no sub-rect, untinted, `Fill` fit, which resolves to
    /// the base rect at full UV against the encoder's zero intrinsic size.
    pub(crate) fn add_gpu_view(&mut self, id: TextureId, epoch: u64) {
        let record = ShapeRecord::Image {
            local_rect: None,
            tint: RgbaF32::WHITE.to_f16(),
            source: ImageSource::GpuView { id, epoch },
            fit: ImageFit::Fill,
            min_filter: ImageFilter::Linear,
            mag_filter: ImageFilter::Linear,
            // A view's target is allocated to the rect it composites into, so it is
            // never minified and has no footprint to cover.
            downsample: ImageDownsample::Single,
        };
        self.push(record);
    }
}

#[cfg(test)]
mod tests;
