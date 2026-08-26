pub(crate) mod hash;
pub(crate) mod lower;
pub(crate) mod paint;
pub(crate) mod record;

use crate::common::content_hash::ContentHash;
use crate::primitives::color::{Color, ColorF16};
use crate::primitives::image::{ImageDownsample, ImageFilter, ImageFit};
use crate::primitives::nan::NanCheck;
use crate::scene::record_store::RecordStore;
use crate::scene::shapes::paint::ImageSource;
use crate::scene::shapes::record::ShapeRecord;
use crate::shape::Lower;

/// Per-frame shape-record buffer for one [`crate::scene::tree::Tree`].
///
/// Each node owns a contiguous sub-range of `records` via
/// `NodeRecord.shape_span`. The gaps between a node's children's spans
/// hold that node's direct shapes in record order, which is what
/// [`crate::scene::tree::iter::TreeItems`] interleaves.
///
/// Bulk variable-length payloads (mesh verts/indices, polyline
/// points/colors, gradients) live on the `RecordStore` passed into
/// [`Self::add`]; `ShapeRecord` variants reference them via spans /
/// ids. `ShapeRecord::Text.text` holds a
/// [`RecordedText`](crate::primitives::recorded_text::RecordedText)
/// span and content hash after normalizing its source into the active
/// text arena. Cleared per record pass, capacity retained.
#[derive(Debug, Default)]
pub(crate) struct Shapes {
    pub(crate) records: Vec<ShapeRecord>,
    /// Per-shape authoring hash, parallel to `records`. Computed once
    /// in [`Self::add`] at lowering time (the canonical value);
    /// `Tree::compute_rollups` only folds the stored hash into the
    /// owner's node hash, never recomputes it. Keys the per-shape
    /// damage diff (`(WidgetId, ordinal)` identity) in
    /// `DamageEngine::compute`, letting a single moved shape on a
    /// multi-shape owner push only its own rect pair instead of the
    /// owner's whole paint-rect union. Cleared per frame, capacity
    /// retained.
    pub(crate) hashes: Vec<ContentHash>,
}

impl Shapes {
    pub(crate) fn clear(&mut self) {
        self.records.clear();
        self.hashes.clear();
    }

    /// Lower a user-facing [`Shape`](crate::Shape) and append it to
    /// `records`: passthrough for rect/text, cubic promotion for beziers,
    /// span-stamping for the variable-length variants (polyline / mesh)
    /// whose payload bytes land on the [`RecordStore`].
    ///
    /// Drops any shape whose authoring inputs would emit no visible
    /// pixels, before lowering runs — the shape buffer's half of tier 2
    /// (see [`paint_sink`](crate::renderer::frontend::paint_sink) for
    /// the policy). Saves the per-shape lowering cost — payload
    /// staging, mesh hashing, text shaping downstream — which the
    /// emit-time gate cannot, having already paid it.
    /// Returns the index of the pushed `ShapeRecord` in `self.records`,
    /// or `None` if the shape was dropped as a no-op — the index is what
    /// `Forest::add_shape_animated` keys its paint-anim row by.
    pub(crate) fn add<S: Lower>(&mut self, shape: S, store: &RecordStore) -> Option<u32> {
        if shape.is_noop() {
            return None;
        }
        let record = shape.lower(store);
        // **The one NaN gate.** Lowered rather than authored, because by
        // here every bulk input has been folded into a `bbox` under the
        // AABB NaN contract — so this is `O(1)` for every shape and can
        // stay in release, where it makes NaN mean what the rest of the
        // pipeline already means by "no-op": drop the draw. Debug builds
        // get the loud version.
        if record.has_nan() {
            debug_assert!(
                !record.has_nan(),
                "NaN in a paint-shape input — an arithmetic bug on the \
                 calling side (0/0, ∞-∞, an unseeded layout value). It \
                 would not crash: the draw silently vanishes, or worse, \
                 poisons a damage bbox and leaves trails. {record:?}",
            );
            return None;
        }
        Some(self.push(record))
    }

    /// Append an already-lowered record, returning its index.
    fn push(&mut self, record: ShapeRecord) -> u32 {
        let idx = self.records.len() as u32;
        let hash = hash::compute_record_hash(&record);
        self.records.push(record);
        self.hashes.push(hash);
        idx
    }

    /// Append a [`ImageSource::GpuView`]-sourced [`ShapeRecord::Image`]
    /// directly — assembled by `Ui::gpu_view`, not lowered from a
    /// user-facing [`Shape`](crate::Shape), so this bypasses the
    /// [`Self::add`] lowering. The view's `id` + `paint` live in
    /// `Ui::gpu_views` keyed by the owner's `WidgetId`; the record carries
    /// only `epoch` (which the per-frame damage hash reads).
    ///
    /// The placement fields are the neutral ones that reproduce a
    /// full-owner-rect composite: no sub-rect, untinted, and a `Fill`
    /// fit which — against the all-zero intrinsic size the encoder hands
    /// a view — resolves to the base rect at full UV.
    pub(crate) fn add_gpu_view(&mut self, epoch: u64) {
        let record = ShapeRecord::Image {
            local_rect: None,
            tint: ColorF16::from(Color::WHITE),
            source: ImageSource::GpuView { epoch },
            fit: ImageFit::Fill,
            min_filter: ImageFilter::Linear,
            mag_filter: ImageFilter::Linear,
            // A view's target is allocated to the rect it composites into, so
            // it is never minified and has no footprint to cover.
            downsample: ImageDownsample::Single,
        };
        self.push(record);
    }
}

#[cfg(test)]
mod tests;
