pub(crate) mod hash;
pub(crate) mod record;

use crate::forest::frame_arena::FrameArena;
use crate::forest::rollups::NodeHash;
use crate::forest::shapes::hash::compute_record_hash;
use crate::forest::shapes::record::{ShapeRecord, ShapeStroke};
use crate::primitives::span::Span;
use crate::renderer::gradient_atlas::GradientAtlas;
use crate::renderer::texture_id::TextureId;
use crate::shape::{PolylineColors, Shape};

/// Per-frame shape-record buffer for one [`crate::forest::tree::Tree`].
///
/// Each node owns a contiguous sub-range of `records` via
/// `NodeRecord.shape_span`. The gaps between a node's children's spans
/// hold that node's direct shapes in record order, which is what
/// [`crate::forest::tree::iter::TreeItems`] interleaves.
///
/// Bulk variable-length payloads (mesh verts/indices, polyline
/// points/colors, gradients) live on the `FrameArena` passed into
/// [`Self::add`]; `ShapeRecord` variants reference them via spans /
/// ids. `ShapeRecord::Text.text` is the asymmetric case — it holds
/// an [`InternedStr`](crate::InternedStr) inline: `Owned` carries
/// its bytes on the record itself (`SmolStr`), while `Interned`
/// references `FrameArena::fmt_scratch` via a `Span`. Cleared per
/// frame, capacity retained.
#[derive(Default)]
pub(crate) struct Shapes {
    pub(crate) records: Vec<ShapeRecord>,
    /// Per-shape authoring hash, parallel to `records`. Computed once
    /// in [`Self::add`] at lowering time (the canonical value);
    /// `Tree::compute_hashes` only folds the stored hash into the
    /// owner's node hash, never recomputes it. Keys the per-shape
    /// damage diff (`(WidgetId, ordinal)` identity) in
    /// `DamageEngine::compute`, letting a single moved shape on a
    /// multi-shape owner push only its own rect pair instead of the
    /// owner's whole paint-rect union. Cleared per frame, capacity
    /// retained.
    pub(crate) hashes: Vec<NodeHash>,
}

impl Shapes {
    pub(crate) fn clear(&mut self) {
        self.records.clear();
        self.hashes.clear();
    }

    /// Lower a user-facing [`Shape`] and append it to `records`:
    /// passthrough for rect/text, curve flattening for beziers,
    /// span-stamping for the variable-length variants (polyline /
    /// mesh) whose payloads land in `self.payloads`.
    ///
    /// Single canonical noop gate for the shape buffer — drops any
    /// shape whose authoring inputs would emit no visible pixels
    /// before lowering runs. Mirrors `cmd_buffer::draw_*`'s emit-time
    /// gate: caller code can pass anything, the storage layer
    /// canonicalises. Saves the per-shape lowering cost (polyline
    /// tessellation, bezier flattening, mesh hashing, text shaping
    /// downstream) that the cmd-buffer gate alone wouldn't.
    /// Returns the index of the pushed `ShapeRecord` in `self.records`,
    /// or `None` if the shape was dropped as a no-op. Callers that want
    /// to attach side data keyed by shape-index (e.g. paint-anim
    /// registry) use the returned index; the legacy "fire and forget"
    /// path ignores it.
    pub(crate) fn add(
        &mut self,
        shape: Shape<'_>,
        arena: &FrameArena,
        atlas: &GradientAtlas,
    ) -> Option<u32> {
        if shape.is_noop() {
            return None;
        }
        if let Shape::Polyline { points, colors, .. } = &shape {
            colors.assert_matches(points.len());
        }
        let record = match shape {
            Shape::RoundedRect {
                local_rect,
                corners,
                fill,
                stroke,
            } => {
                let lowered = arena.lower_brush(fill, atlas);
                ShapeRecord::RoundedRect {
                    local_rect,
                    corners,
                    fill: lowered.brush,
                    stroke: ShapeStroke::from(stroke),
                    fill_grad_hash: lowered.hash,
                }
            }
            Shape::Line {
                a,
                b,
                width,
                brush,
                cap,
                join,
            } => arena.lower_polyline(
                &[a, b],
                PolylineColors::Single(brush.expect_solid()),
                width,
                cap,
                join,
            ),
            Shape::Polyline {
                points,
                colors,
                width,
                cap,
                join,
            } => arena.lower_polyline(points, colors, width, cap, join),
            Shape::CubicBezier {
                p0,
                p1,
                p2,
                p3,
                width,
                brush,
                cap,
            } => arena.lower_cubic_bezier([p0, p1, p2, p3], width, brush, cap, atlas),
            Shape::QuadraticBezier {
                p0,
                p1,
                p2,
                width,
                brush,
                cap,
            } => arena.lower_quadratic_bezier([p0, p1, p2], width, brush, cap, atlas),
            Shape::Text {
                local_origin,
                text,
                brush,
                font_size_px,
                line_height_px,
                wrap,
                align,
                family,
            } => {
                use crate::primitives::interned_str::InternedStr;
                // Each carrier costs only its hash compute:
                // - `Interned` reuses the hash captured at `Ui::fmt` time.
                // - `Owned` hashes the bytes once at lowering; the bytes
                //   stay where they are (no memcpy into the text arena,
                //   no per-shape allocation).
                let text_hash = match &text {
                    InternedStr::Interned { hash, .. } => *hash,
                    InternedStr::Owned(s) => FrameArena::hash_text(s),
                };
                ShapeRecord::Text {
                    local_origin,
                    text,
                    text_hash,
                    color: brush.expect_solid().into(),
                    font_size_px,
                    line_height_px,
                    wrap,
                    align,
                    family,
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
                tint,
            } => ShapeRecord::Image {
                local_rect,
                tint: tint.into(),
                // Extract the cheap id + size; the owning `ImageHandle`
                // the caller holds is what keeps the GPU texture alive.
                id: handle.id(),
                size: handle.size_u16(),
                fit,
            },
            Shape::Mesh {
                mesh,
                local_rect,
                tint,
            } => {
                let mut a = arena.inner_mut();
                let v_start = a.meshes.vertices.len() as u32;
                a.meshes.vertices.extend_from_slice(&mesh.vertices);
                let i_start = a.meshes.indices.len() as u32;
                a.meshes.indices.extend_from_slice(&mesh.indices);
                let content_hash = mesh.content_hash();
                let bbox = mesh.bbox();
                ShapeRecord::Mesh {
                    local_rect,
                    tint: tint.expect_solid().into(),
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

    /// Append a [`ShapeRecord::GpuView`] directly — the `GpuView` widget's
    /// `id` + redraw `epoch` come from the per-window
    /// [`GpuViewRegistry`](crate::renderer::gpu_view::GpuViewRegistry)
    /// (a `Ui`-level concern), not from a user-facing [`Shape`], so this
    /// bypasses the [`Self::add`] lowering. Returns the pushed index.
    pub(crate) fn add_gpu_view(&mut self, id: TextureId, epoch: u64) -> u32 {
        let record = ShapeRecord::GpuView { id, epoch };
        let idx = self.records.len() as u32;
        let hash = compute_record_hash(&record);
        self.records.push(record);
        self.hashes.push(hash);
        idx
    }
}
