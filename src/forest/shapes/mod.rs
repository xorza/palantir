pub(crate) mod record;

use crate::common::frame_arena::{BezierInputs, FrameArena};
use crate::forest::shapes::record::{
    ChromeRow, GradientPayload, ShapeBrush, ShapeRecord, ShapeStroke,
};
use crate::layout::types::span::Span;
use crate::primitives::background::Background;
use crate::primitives::bezier::{flatten_cubic, flatten_quadratic};
use crate::primitives::brush::Brush;
use crate::shape::{PolylineColors, Shape};

/// Per-frame shape store for one [`crate::forest::tree::Tree`].
///
/// - `records` is the flat shape buffer; each node owns a contiguous
///   sub-range via `NodeRecord.shape_span`. The gaps between a node's
///   children's spans hold that node's direct shapes in record order,
///   which is what [`crate::forest::tree::TreeItems`] interleaves.
/// - bulk variable-length payloads (mesh verts/indices, polyline
///   points/colors) live on the `FrameArena` passed into [`Self::add`];
///   `ShapeRecord` variants reference them via spans.
///
/// Cleared together per frame, capacity retained — same lifecycle as
/// the rest of the tree.
#[derive(Default)]
pub(crate) struct Shapes {
    pub(crate) records: Vec<ShapeRecord>,
    /// Per-frame gradient arena. `ShapeBrush::Gradient(id)` indexes
    /// into this vec; the lowering site (`Self::lower_brush`) pushes
    /// the gradient's `Brush::{Linear,Radial,Conic}` payload here so
    /// `ShapeRecord` only carries a 4-byte handle instead of the
    /// 88-byte `Brush` enum. Cleared per frame, capacity retained.
    pub(crate) gradients: Vec<GradientPayload>,
}

impl Shapes {
    pub(crate) fn clear(&mut self) {
        self.records.clear();
        self.gradients.clear();
    }

    /// Lower a user-side `Brush` to the storage form: `Solid` stays
    /// inline, gradients push to `self.gradients` and return an
    /// indexing `ShapeBrush::Gradient`. The pre-computed content hash
    /// is returned alongside so the caller can stamp it into the
    /// `ShapeRecord` and keep `ShapeRecord::Hash` context-free.
    fn lower_brush(&mut self, brush: Brush) -> (ShapeBrush, u64) {
        match brush {
            Brush::Solid(c) => (ShapeBrush::Solid(c.into()), 0),
            Brush::Linear(g) => {
                let payload = GradientPayload::Linear(g);
                let hash = payload.content_hash();
                let id = self.gradients.len() as u32;
                self.gradients.push(payload);
                (ShapeBrush::Gradient(id), hash)
            }
            Brush::Radial(g) => {
                let payload = GradientPayload::Radial(g);
                let hash = payload.content_hash();
                let id = self.gradients.len() as u32;
                self.gradients.push(payload);
                (ShapeBrush::Gradient(id), hash)
            }
            Brush::Conic(g) => {
                let payload = GradientPayload::Conic(g);
                let hash = payload.content_hash();
                let id = self.gradients.len() as u32;
                self.gradients.push(payload);
                (ShapeBrush::Gradient(id), hash)
            }
        }
    }

    /// Lower a user-facing `Background` to a `ChromeRow`. Same
    /// gradient-arena lowering as `Shapes::add` uses for
    /// `RoundedRect.fill`, so chrome and shape paints share one
    /// gradients vec per tree.
    pub(crate) fn lower_background(&mut self, bg: Background) -> ChromeRow {
        let (fill, fill_grad_hash) = self.lower_brush(bg.fill);
        ChromeRow {
            fill,
            stroke: ShapeStroke::from(bg.stroke),
            radius: bg.radius,
            shadow: bg.shadow.into(),
            fill_grad_hash,
        }
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
    pub(crate) fn add(&mut self, shape: Shape<'_>, arena: &mut FrameArena) {
        if shape.is_noop() {
            return;
        }
        if let Shape::Polyline { points, colors, .. } = &shape {
            colors.assert_matches(points.len());
        }
        let record = match shape {
            Shape::RoundedRect {
                local_rect,
                radius,
                fill,
                stroke,
            } => {
                let (fill, fill_grad_hash) = self.lower_brush(fill);
                let stroke = ShapeStroke::from(stroke);
                ShapeRecord::RoundedRect {
                    local_rect,
                    radius,
                    fill,
                    stroke,
                    fill_grad_hash,
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
                join,
                tolerance,
            } => {
                arena.bezier_scratch.clear();
                flatten_cubic(p0, p1, p2, p3, tolerance, &mut arena.bezier_scratch);
                arena.lower_bezier(
                    BezierInputs::Cubic([p0, p1, p2, p3]),
                    width,
                    brush.expect_solid(),
                    cap,
                    join,
                    tolerance,
                )
            }
            Shape::QuadraticBezier {
                p0,
                p1,
                p2,
                width,
                brush,
                cap,
                join,
                tolerance,
            } => {
                arena.bezier_scratch.clear();
                flatten_quadratic(p0, p1, p2, tolerance, &mut arena.bezier_scratch);
                arena.lower_bezier(
                    BezierInputs::Quadratic([p0, p1, p2]),
                    width,
                    brush.expect_solid(),
                    cap,
                    join,
                    tolerance,
                )
            }
            Shape::Text {
                local_origin,
                text,
                brush,
                font_size_px,
                line_height_px,
                wrap,
                align,
                family,
            } => ShapeRecord::Text {
                local_origin,
                text,
                color: brush.expect_solid().into(),
                font_size_px,
                line_height_px,
                wrap,
                align,
                family,
            },
            Shape::Shadow {
                local_rect,
                radius,
                shadow,
            } => ShapeRecord::Shadow {
                local_rect,
                radius,
                shadow: shadow.into(),
            },
            Shape::Mesh {
                mesh,
                local_rect,
                tint,
            } => {
                let v_start = arena.meshes.vertices.len() as u32;
                arena.meshes.vertices.extend_from_slice(&mesh.vertices);
                let i_start = arena.meshes.indices.len() as u32;
                arena.meshes.indices.extend_from_slice(&mesh.indices);
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
        self.records.push(record);
    }
}
