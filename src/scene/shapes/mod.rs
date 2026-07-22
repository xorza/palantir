pub(crate) mod hash;
pub(crate) mod lower;
pub(crate) mod paint;
pub(crate) mod record;

#[cfg(test)]
mod tests;

use crate::common::content_hash::ContentHash;
use crate::primitives::span::Span;
use crate::scene::record_store::RecordStore;
use crate::scene::shapes::hash::compute_record_hash;
use crate::scene::shapes::paint::ShapeStroke;
use crate::scene::shapes::record::ShapeRecord;
use crate::shape::Shape;

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
    /// Single canonical noop gate for the shape buffer — drops any
    /// shape whose authoring inputs would emit no visible pixels
    /// before lowering runs. Mirrors `cmd_buffer::draw_*`'s emit-time
    /// gate: caller code can pass anything, the storage layer
    /// canonicalises. Saves the per-shape lowering cost (payload
    /// staging, mesh hashing, text shaping downstream) that the
    /// cmd-buffer gate alone wouldn't.
    /// Returns the index of the pushed `ShapeRecord` in `self.records`,
    /// or `None` if the shape was dropped as a no-op. Callers that want
    /// to attach side data keyed by shape-index (e.g. paint-anim
    /// registry) use the returned index; the legacy "fire and forget"
    /// path ignores it.
    pub(crate) fn add(&mut self, shape: Shape<'_>, store: &RecordStore) -> Option<u32> {
        if let Shape::Polyline { points, colors, .. } = &shape {
            colors.assert_matches(points.len());
        }
        if shape.is_noop() {
            return None;
        }
        let record = match shape {
            Shape::RoundedRect {
                local_rect,
                corners,
                fill,
                stroke,
            } => {
                let lowered = lower::brush(store, &fill);
                ShapeRecord::RoundedRect {
                    local_rect,
                    corners,
                    fill: lowered.brush,
                    stroke: ShapeStroke::from(stroke),
                    fill_grad_hash: lowered.hash,
                }
            }
            Shape::WindowedRect {
                local_rect,
                corners,
                fill,
                stroke,
            } => {
                let lowered = lower::brush(store, &fill);
                ShapeRecord::WindowedRect {
                    local_rect,
                    corners,
                    fill: lowered.brush,
                    stroke: ShapeStroke::from(stroke),
                    fill_grad_hash: lowered.hash,
                }
            }
            Shape::Triangle {
                a,
                b,
                c,
                radius,
                fill,
                stroke,
            } => lower::triangle(a, b, c, radius, fill, stroke),
            Shape::Line {
                a,
                b,
                width,
                brush,
                cap,
            } => lower::line(store, a, b, width, brush, cap),
            Shape::Polyline {
                points,
                colors,
                width,
                cap,
                join,
            } => lower::polyline(store, points, colors, width, cap, join),
            Shape::CubicBezier {
                p0,
                p1,
                p2,
                p3,
                width,
                brush,
                cap,
            } => lower::cubic_bezier(store, [p0, p1, p2, p3], width, brush, cap),
            Shape::QuadraticBezier {
                p0,
                p1,
                p2,
                width,
                brush,
                cap,
            } => lower::quadratic_bezier(store, [p0, p1, p2], width, brush, cap),
            Shape::Arc {
                center,
                radius,
                start_angle,
                sweep,
                width,
                brush,
                cap,
            } => lower::arc(store, center, radius, start_angle, sweep, width, brush, cap),
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
            Shape::Shadow {
                local_rect,
                corners,
                shadow,
            } => ShapeRecord::Shadow {
                local_rect,
                corners,
                shadow: shadow.into(),
            },
            Shape::Image {
                handle,
                local_rect,
                fit,
                min_filter,
                mag_filter,
                tint,
            } => ShapeRecord::Image {
                local_rect,
                tint: tint.into(),
                // Extract the cheap id + size; the owning `ImageHandle`
                // the caller holds is what keeps the GPU texture alive.
                id: handle.id(),
                size: handle.size(),
                fit,
                min_filter,
                mag_filter,
            },
            Shape::Mesh {
                mesh,
                local_rect,
                tint,
            } => {
                let mut payloads = store.payloads.borrow_mut();
                let v_start = payloads.meshes.vertices.len() as u32;
                payloads.meshes.vertices.extend_from_slice(&mesh.vertices);
                let i_start = payloads.meshes.indices.len() as u32;
                payloads.meshes.indices.extend_from_slice(&mesh.indices);
                let content_hash = mesh.content_hash();
                let bbox = mesh.bbox();
                ShapeRecord::Mesh {
                    local_rect,
                    tint: tint.into(),
                    vertices: Span::new(v_start, mesh.vertices.len() as u32),
                    indices: Span::new(i_start, mesh.indices.len() as u32),
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

    /// Append a [`ShapeRecord::GpuView`] directly — assembled by `Ui::gpu_view`,
    /// not lowered from a user-facing [`Shape`], so this bypasses the
    /// [`Self::add`] lowering. The view's `id` + `paint` live in `Ui::gpu_views`
    /// keyed by the owner's `WidgetId`; the shape carries only `epoch` (which
    /// the per-frame damage hash reads).
    pub(crate) fn add_gpu_view(&mut self, epoch: u64) {
        let record = ShapeRecord::GpuView { epoch };
        let hash = compute_record_hash(&record);
        self.records.push(record);
        self.hashes.push(hash);
    }
}
