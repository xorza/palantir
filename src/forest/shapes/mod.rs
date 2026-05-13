pub(crate) mod payloads;
pub(crate) mod record;

use crate::forest::shapes::payloads::{BezierInputs, ShapePayloads};
use crate::forest::shapes::record::ShapeRecord;
use crate::layout::types::span::Span;
use crate::primitives::bezier::{flatten_cubic, flatten_quadratic};
use crate::shape::{PolylineColors, Shape};

/// Per-frame shape store for one [`crate::forest::tree::Tree`].
///
/// - `records` is the flat shape buffer; each node owns a contiguous
///   sub-range via `NodeRecord.shape_span`. The gaps between a node's
///   children's spans hold that node's direct shapes in record order,
///   which is what [`crate::forest::tree::TreeItems`] interleaves.
/// - `payloads` holds variable-length side-tables that record variants
///   (`Mesh` / `Polyline`) reference via inner `Span`s; lowering helpers
///   live on `ShapePayloads` so this struct stays a thin dispatcher.
///
/// Cleared together per frame, capacity retained — same lifecycle as
/// the rest of the tree.
#[derive(Default)]
pub(crate) struct Shapes {
    pub(crate) records: Vec<ShapeRecord>,
    pub(crate) payloads: ShapePayloads,
}

impl Shapes {
    pub(crate) fn clear(&mut self) {
        self.records.clear();
        self.payloads.clear();
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
    pub(crate) fn add(&mut self, shape: Shape<'_>) {
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
            } => ShapeRecord::RoundedRect {
                local_rect,
                radius,
                fill,
                stroke,
            },
            Shape::Line {
                a,
                b,
                width,
                brush,
                cap,
                join,
            } => self.payloads.lower_polyline(
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
            } => self
                .payloads
                .lower_polyline(points, colors, width, cap, join),
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
                self.payloads.bezier_scratch.clear();
                flatten_cubic(p0, p1, p2, p3, tolerance, &mut self.payloads.bezier_scratch);
                self.payloads.lower_bezier(
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
                self.payloads.bezier_scratch.clear();
                flatten_quadratic(p0, p1, p2, tolerance, &mut self.payloads.bezier_scratch);
                self.payloads.lower_bezier(
                    BezierInputs::Quadratic([p0, p1, p2]),
                    width,
                    brush.expect_solid(),
                    cap,
                    join,
                    tolerance,
                )
            }
            Shape::Text {
                local_rect,
                text,
                brush,
                font_size_px,
                line_height_px,
                wrap,
                align,
            } => ShapeRecord::Text {
                local_rect,
                text,
                color: brush.expect_solid(),
                font_size_px,
                line_height_px,
                wrap,
                align,
            },
            Shape::Shadow {
                local_rect,
                radius,
                shadow,
            } => ShapeRecord::Shadow {
                local_rect,
                radius,
                shadow,
            },
            Shape::Mesh {
                mesh,
                local_rect,
                tint,
            } => {
                let arena = &mut self.payloads.meshes;
                let v_start = arena.vertices.len() as u32;
                arena.vertices.extend_from_slice(&mesh.vertices);
                let i_start = arena.indices.len() as u32;
                arena.indices.extend_from_slice(&mesh.indices);
                let content_hash = mesh.content_hash();
                ShapeRecord::Mesh {
                    local_rect,
                    tint: tint.expect_solid(),
                    vertices: Span::new(v_start, mesh.vertices.len() as u32),
                    indices: Span::new(i_start, mesh.indices.len() as u32),
                    content_hash,
                }
            }
        };
        self.records.push(record);
    }
}
