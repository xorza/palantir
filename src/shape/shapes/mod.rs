//! Recorded shapes for one tree: the [`Shapes`] buffer and its parallel hash
//! column over the [`ShapeRecord`] variants it holds.

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::image::{ImageDownsample, ImageFilter, ImageFit};
use crate::shape::Shape;
use crate::shape::geometry::closed_geometry::ClosedGeometry;
use crate::shape::paint::closed_paint::ClosedPaint;
use crate::shape::paint::image_source::ImageSource;
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

    /// Append an [`ImageSource::GpuView`]-sourced image [`ShapeRecord::Closed`] directly,
    /// bypassing the record store's lowering. The view's `paint` lives in `Ui::gpu_views`
    /// by the owner's `WidgetId`; the record carries its target `id` and `epoch`.
    ///
    /// Placement is neutral: no sub-rect, untinted, `Fill` fit, which resolves to
    /// the base rect at full UV against the encoder's zero intrinsic size.
    pub(crate) fn add_gpu_view(&mut self, id: TextureId, epoch: u64) {
        let record = ShapeRecord::Closed {
            geometry: ClosedGeometry::Box(Shape::rect().geometry()),
            paint: ClosedPaint::Image {
                tint: RgbaF32::WHITE.to_f16(),
                source: ImageSource::GpuView { id, epoch },
                fit: ImageFit::Fill,
                min_filter: ImageFilter::Linear,
                mag_filter: ImageFilter::Linear,
                // A view's target is allocated to the rect it composites into, so it is
                // never minified and has no footprint to cover.
                downsample: ImageDownsample::Single,
            },
        };
        self.push(record);
    }
}

#[cfg(test)]
mod tests;
