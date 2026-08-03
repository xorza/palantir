pub(crate) mod hash;
pub(super) mod lower;
pub(crate) mod paint;
pub(crate) mod record;

#[cfg(test)]
mod tests;

use crate::common::content_hash::ContentHash;
use crate::primitives::color::{Color, ColorF16};
use crate::primitives::image::{ImageFilter, ImageFit};
use crate::primitives::nan::NanCheck;
use crate::primitives::span::Span;
use crate::scene::record_store::RecordStore;
use crate::scene::shapes::hash::compute_record_hash;
use crate::scene::shapes::paint::ImageSource;
use crate::scene::shapes::record::ShapeRecord;
use crate::shape::Shape;
use crate::shape::curve::CurveGeometry;

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
/// [`RecordedText`](crate::primitives::interned_str::RecordedText)
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

    /// Lower a user-facing [`Shape`] and append it to `records`:
    /// passthrough for rect/text, cubic promotion for beziers,
    /// span-stamping for the variable-length variants (polyline /
    /// mesh) whose payload bytes land on the [`RecordStore`].
    ///
    /// Drops any shape whose authoring inputs would emit no visible
    /// pixels, before lowering runs — the shape buffer's half of tier 2
    /// (see [`paint_sink`](crate::renderer::frontend::paint_sink) for
    /// the policy). Saves the per-shape lowering cost — payload
    /// staging, mesh hashing, text shaping downstream — which the
    /// emit-time gate cannot, having already paid it.
    /// Returns the index of the pushed `ShapeRecord` in `self.records`,
    /// or `None` if the shape was dropped as a no-op. Callers that want
    /// to attach side data keyed by shape-index (e.g. paint-anim
    /// registry) use the returned index; the legacy "fire and forget"
    /// path ignores it.
    pub(crate) fn add(&mut self, shape: Shape<'_>, store: &RecordStore) -> Option<u32> {
        // Ahead of every other gate, including `is_noop`: `noop_f32`
        // reads NaN as invisible, so a NaN-width shape would leave
        // through that early return unexamined.
        debug_assert!(
            !shape.has_nan(),
            "NaN in a paint-shape input — an arithmetic bug on the \
             calling side (0/0, ∞-∞, an unseeded layout value). Release \
             builds don't crash on it; the draw silently vanishes, or \
             worse, poisons a damage bbox and leaves trails. {shape:?}",
        );
        if let Shape::Polyline(shape) = &shape {
            shape.colors.assert_matches(shape.points.len());
        }
        if shape.is_noop() {
            return None;
        }
        let record = match shape {
            Shape::Rect(shape) => lower::rect(
                store,
                shape.kind,
                shape.local_rect,
                shape.corners,
                &shape.fill,
                shape.stroke,
            ),
            Shape::Triangle(shape) => lower::triangle(
                shape.a,
                shape.b,
                shape.c,
                shape.radius,
                shape.fill,
                shape.stroke,
            ),
            Shape::Curve(shape) => match shape.geometry {
                CurveGeometry::Line { a, b } => {
                    lower::line(store, a, b, shape.width, shape.brush, shape.cap)
                }
                CurveGeometry::CubicBezier { p0, p1, p2, p3 } => lower::cubic_bezier(
                    store,
                    [p0, p1, p2, p3],
                    shape.width,
                    shape.brush,
                    shape.cap,
                ),
                CurveGeometry::QuadraticBezier { p0, p1, p2 } => lower::quadratic_bezier(
                    store,
                    [p0, p1, p2],
                    shape.width,
                    shape.brush,
                    shape.cap,
                ),
                CurveGeometry::Arc {
                    center,
                    radius,
                    start_angle,
                    sweep,
                } => lower::arc(
                    store,
                    center,
                    radius,
                    start_angle,
                    sweep,
                    shape.width,
                    shape.brush,
                    shape.cap,
                ),
            },
            Shape::Polyline(shape) => lower::polyline(
                store,
                shape.points,
                shape.colors,
                shape.width,
                shape.cap,
                shape.join,
            ),
            Shape::Text {
                local_origin,
                text,
                color,
                font_size_px,
                line_height_px,
                wrap,
                align,
                family,
                weight,
            } => {
                let text = store.record_text(text);
                ShapeRecord::Text {
                    local_origin,
                    text,
                    color: color.into(),
                    font_size_px,
                    line_height_px,
                    wrap,
                    align,
                    family,
                    weight,
                }
            }
            Shape::Shadow(shape) => lower::shadow(shape.local_rect, shape.corners, shape.shadow),
            Shape::Image(shape) => ShapeRecord::Image {
                local_rect: shape.local_rect,
                tint: shape.tint.into(),
                // Extract the cheap id + size; the owning `ImageHandle`
                // the caller holds is what keeps the GPU texture alive.
                source: ImageSource::Texture {
                    id: shape.handle.id(),
                    size: shape.handle.size(),
                },
                fit: shape.fit,
                min_filter: shape.min_filter,
                mag_filter: shape.mag_filter,
            },
            Shape::Mesh(shape) => {
                let mut payloads = store.payloads.borrow_mut();
                let v_start = payloads.meshes.vertices.len() as u32;
                payloads
                    .meshes
                    .vertices
                    .extend_from_slice(&shape.mesh.vertices);
                let i_start = payloads.meshes.indices.len() as u32;
                payloads
                    .meshes
                    .indices
                    .extend_from_slice(&shape.mesh.indices);
                let content_hash = shape.mesh.content_hash();
                let bbox = shape.mesh.bbox();
                ShapeRecord::Mesh {
                    local_rect: shape.local_rect,
                    tint: shape.tint.into(),
                    vertices: Span::new(v_start, shape.mesh.vertices.len() as u32),
                    indices: Span::new(i_start, shape.mesh.indices.len() as u32),
                    bbox,
                    content_hash,
                }
            }
        };
        let idx = self.records.len() as u32;
        let hash = compute_record_hash(&record);
        self.records.push(record);
        self.hashes.push(hash);
        Some(idx)
    }

    /// Append a [`ImageSource::GpuView`]-sourced [`ShapeRecord::Image`]
    /// directly — assembled by `Ui::gpu_view`, not lowered from a
    /// user-facing [`Shape`], so this bypasses the [`Self::add`]
    /// lowering. The view's `id` + `paint` live in `Ui::gpu_views` keyed
    /// by the owner's `WidgetId`; the record carries only `epoch` (which
    /// the per-frame damage hash reads).
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
        };
        let hash = compute_record_hash(&record);
        self.records.push(record);
        self.hashes.push(hash);
    }
}
