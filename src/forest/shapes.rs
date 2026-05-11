use crate::common::hash::Hasher as FxHasher;
use crate::layout::types::align::Align;
use crate::layout::types::span::Span;
use crate::primitives::bezier::{
    FlatPoint, eval_color_cubic, eval_color_quadratic, flatten_cubic, flatten_quadratic, lerp_color,
};
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::mesh::Mesh;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::stroke::Stroke;
use crate::shape::{BezierColors, ColorMode, LineCap, LineJoin, PolylineColors, Shape, TextWrap};
use glam::Vec2;
use std::borrow::Cow;
use std::hash::{Hash, Hasher};

#[derive(Clone, Debug)]
pub(crate) enum ShapeRecord {
    /// Filled/stroked rounded rectangle. With `local_rect = None` it covers
    /// the owner node's full arranged rect (position/size come from layout).
    /// With `local_rect = Some(r)` it paints `r` at owner-relative coords —
    /// `r.min = (0, 0)` is the owner's top-left. The sub-rect form paints in
    /// the slot it was pushed in (interleaved with children via the slot
    /// mechanism — see `Tree::add_shape`), still under the owner's clip but
    /// outside its pan transform. Used for scrollbar tracks/thumbs (pushed
    /// after body content → slot N) and TextEdit carets (pushed after the
    /// Text shape on a leaf → slot 0, after the Text in record order).
    RoundedRect {
        local_rect: Option<Rect>,
        radius: Corners,
        fill: Color,
        stroke: Stroke,
    },
    /// Stroked polyline. `points`/`colors` index into the active
    /// tree's `polyline_points` / `polyline_colors` arenas. `colors`
    /// length depends on `color_mode`: 1 for `Single`,
    /// `points.len()` for `PerPoint`, `points.len() - 1` for
    /// `PerSegment`. `content_hash` summarizes points+colors+mode
    /// +cap+join bytes for cache identity. `bbox` is the
    /// axis-aligned bounds of `points` in owner-relative coords —
    /// the encoder translates it into cmd-buffer coords by adding
    /// the owner rect origin. `cap` and `join` are user-picked
    /// stroke-style enums; tessellator branches on them.
    Polyline {
        width: f32,
        color_mode: ColorMode,
        cap: LineCap,
        join: LineJoin,
        points: Span,
        colors: Span,
        bbox: Rect,
        content_hash: u64,
    },
    /// Shaped text run — *authoring inputs only*. Measured size and
    /// shaped-buffer key are layout outputs and live on
    /// `LayoutResult.text_shapes`, not here. `wrap` selects between "shape
    /// once and freeze" (`Single`) and "reshape if the parent commits a
    /// narrower width than the natural unbroken line" (`Wrap`). `align`
    /// positions the glyph bbox inside the owner leaf's arranged rect (or
    /// `local_rect` if set) — the encoder reads it together with the
    /// shaped run's `measured` to shift the emitted `DrawText` rect.
    /// `HAlign::Auto`/`Stretch` and `VAlign::Auto`/`Stretch` collapse to
    /// top-left for text (glyphs don't stretch).
    ///
    /// `local_rect` mirrors `RoundedRect::local_rect`: `None` paints into
    /// the owner's arranged rect (deflated by the node's `padding`);
    /// `Some(lr)` paints `lr` at owner-relative coords (`lr.min = (0, 0)`
    /// is owner top-left), with `padding` skipped and `align` positioning
    /// the run *inside `lr`*. Lets a custom widget place multiple text
    /// runs in one leaf without each clobbering the others.
    Text {
        local_rect: Option<Rect>,
        /// `Cow<'static, str>` so static-string labels (the common case via
        /// `&'static str → Into<Cow<…>>`) round-trip with only pointer-copy
        /// `Clone`s — no per-frame heap alloc. Dynamic strings still allocate
        /// once into `Cow::Owned` at the authoring boundary.
        text: Cow<'static, str>,
        color: Color,
        font_size_px: f32,
        /// Line-height in logical px, fed straight to the shaper's
        /// `Metrics::new`. Authoring-side widgets typically set this to
        /// `font_size_px * line_height_mult` where the multiplier
        /// defaults to [`crate::text::LINE_HEIGHT_MULT`] (1.2). Carrying
        /// the resolved px on the shape — instead of a multiplier the
        /// shaper would re-resolve — means the shaper doesn't have to
        /// know about widget conventions, and two `ShapeRecord::Text` runs at
        /// the same font-size but different leading correctly produce
        /// distinct cached shaped buffers (via [`TextCacheKey::lh_q`]).
        line_height_px: f32,
        wrap: TextWrap,
        align: Align,
    },
    /// User-supplied colored triangle mesh. Vertex/index data lives in
    /// the active `Tree`'s `mesh_vertices` / `mesh_indices` arenas;
    /// these spans index into them. `content_hash` summarizes
    /// vertex+index bytes for cache identity — two frames with
    /// identical mesh content share a hash even though their span
    /// offsets differ.
    Mesh {
        local_rect: Option<Rect>,
        tint: Color,
        vertices: Span,
        indices: Span,
        content_hash: u64,
    },
}

impl Hash for ShapeRecord {
    /// Discriminant tags are stable (`RoundedRect=0`, `Polyline=1`,
    /// `Text=2`, `Mesh=3`) so cache keys don't shift if variants are
    /// reordered.
    fn hash<H: Hasher>(&self, h: &mut H) {
        match self {
            ShapeRecord::RoundedRect {
                local_rect,
                radius,
                fill,
                stroke,
            } => {
                h.write_u8(0);
                match local_rect {
                    None => h.write_u8(0),
                    Some(r) => {
                        h.write_u8(1);
                        r.hash(h);
                    }
                }
                radius.hash(h);
                fill.hash(h);
                stroke.hash(h);
            }
            ShapeRecord::Polyline { content_hash, .. } => {
                // `content_hash` already covers width + color_mode +
                // cap + join + points + colors (computed in
                // `lower_polyline` / `lower_bezier`). bbox is derived
                // from points; spans are frame-local — neither belongs
                // in cache identity.
                h.write_u8(1);
                h.write_u64(*content_hash);
            }
            ShapeRecord::Text {
                local_rect,
                text,
                color,
                font_size_px,
                line_height_px,
                wrap,
                align,
            } => {
                h.write_u8(2);
                match local_rect {
                    None => h.write_u8(0),
                    Some(r) => {
                        h.write_u8(1);
                        r.hash(h);
                    }
                }
                text.hash(h);
                color.hash(h);
                h.write_u32(font_size_px.to_bits());
                h.write_u32(line_height_px.to_bits());
                h.write_u16(((align.raw() as u16) << 8) | *wrap as u8 as u16);
            }
            ShapeRecord::Mesh {
                local_rect,
                tint,
                vertices: _,
                indices: _,
                content_hash,
            } => {
                h.write_u8(3);
                match local_rect {
                    None => h.write_u8(0),
                    Some(r) => {
                        h.write_u8(1);
                        r.hash(h);
                    }
                }
                tint.hash(h);
                h.write_u64(*content_hash);
            }
        }
    }
}

/// Per-frame side-table arenas for shape variants that need
/// variable-length backing storage. Lives on both [`Shapes`] (records
/// reference these via `Span`s) and
/// [`crate::renderer::frontend::cmd_buffer::RenderCmdBuffer`] (cmd
/// payloads do the same). Cleared together per frame, capacity
/// retained — single struct keeps the lifecycle and future-extension
/// story (curves, etc.) in one place instead of scattered fields on
/// every container.
#[derive(Default)]
pub(crate) struct ShapePayloads {
    /// Vertex + index storage for `ShapeRecord::Mesh`.
    pub(crate) meshes: Mesh,
    /// Point storage for `ShapeRecord::Polyline`. Indexed by the
    /// record's `points` `Span`.
    pub(crate) polyline_points: Vec<Vec2>,
    /// Color storage for `ShapeRecord::Polyline`. Length per
    /// record is 1, `points.len()`, or `points.len() - 1` per
    /// `ColorMode`.
    pub(crate) polyline_colors: Vec<Color>,
    /// Scratch for bezier flattening. Lives here so capacity
    /// persists across frames — steady-state alloc-free. Cleared
    /// (length only) every `add_shape` call that uses it; the
    /// flattened points it produces get copied into
    /// `polyline_points` immediately after.
    pub(crate) bezier_scratch: Vec<FlatPoint>,
}

impl ShapePayloads {
    pub(crate) fn clear(&mut self) {
        self.meshes.clear();
        self.polyline_points.clear();
        self.polyline_colors.clear();
        self.bezier_scratch.clear();
    }
}

/// Per-frame shape store for one [`crate::forest::tree::Tree`].
///
/// - `records` is the flat shape buffer; each node owns a contiguous
///   sub-range via `NodeRecord.shape_span`. The gaps between a node's
///   children's spans hold that node's direct shapes in record order,
///   which is what [`crate::forest::tree::TreeItems`] interleaves.
/// - `payloads` holds variable-length side-tables that record variants
///   (`Mesh` / `Polyline`) reference via inner `Span`s.
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
    pub(crate) fn add(&mut self, shape: Shape<'_>) {
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
                color,
                cap,
                join,
            } => lower_polyline(
                &mut self.payloads,
                &[a, b],
                PolylineColors::Single(color),
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
            } => lower_polyline(&mut self.payloads, points, colors, width, cap, join),
            Shape::CubicBezier {
                p0,
                p1,
                p2,
                p3,
                width,
                colors,
                cap,
                join,
                tolerance,
            } => {
                self.payloads.bezier_scratch.clear();
                flatten_cubic(p0, p1, p2, p3, tolerance, &mut self.payloads.bezier_scratch);
                lower_bezier(
                    &mut self.payloads,
                    BezierInputs::Cubic([p0, p1, p2, p3]),
                    width,
                    colors,
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
                colors,
                cap,
                join,
                tolerance,
            } => {
                self.payloads.bezier_scratch.clear();
                flatten_quadratic(p0, p1, p2, tolerance, &mut self.payloads.bezier_scratch);
                lower_bezier(
                    &mut self.payloads,
                    BezierInputs::Quadratic([p0, p1, p2]),
                    width,
                    colors,
                    cap,
                    join,
                    tolerance,
                )
            }
            Shape::Text {
                local_rect,
                text,
                color,
                font_size_px,
                line_height_px,
                wrap,
                align,
            } => ShapeRecord::Text {
                local_rect,
                text,
                color,
                font_size_px,
                line_height_px,
                wrap,
                align,
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
                    tint,
                    vertices: Span::new(v_start, mesh.vertices.len() as u32),
                    indices: Span::new(i_start, mesh.indices.len() as u32),
                    content_hash,
                }
            }
        };
        self.records.push(record);
    }
}

/// Lower a (points, colors, width) authoring shape into a
/// `ShapeRecord::Polyline`: validate `colors` length against
/// `points.len()`, copy both into the payload arenas, compute the
/// content hash. `Shape::Line` and `Shape::Polyline` both route
/// through this — one record path downstream.
fn lower_polyline(
    payloads: &mut ShapePayloads,
    points: &[Vec2],
    colors: PolylineColors<'_>,
    width: f32,
    cap: LineCap,
    join: LineJoin,
) -> ShapeRecord {
    let (mode, color_slice): (ColorMode, &[Color]) = match colors {
        PolylineColors::Single(ref c) => (ColorMode::Single, std::slice::from_ref(c)),
        PolylineColors::PerPoint(cs) => {
            assert_eq!(
                cs.len(),
                points.len(),
                "Shape::Polyline PerPoint colors len {} != points len {}",
                cs.len(),
                points.len(),
            );
            (ColorMode::PerPoint, cs)
        }
        PolylineColors::PerSegment(cs) => {
            assert_eq!(
                cs.len() + 1,
                points.len(),
                "Shape::Polyline PerSegment colors len {} != points len - 1 ({})",
                cs.len(),
                points.len().saturating_sub(1),
            );
            (ColorMode::PerSegment, cs)
        }
    };

    let p_start = payloads.polyline_points.len() as u32;
    payloads.polyline_points.extend_from_slice(points);
    let c_start = payloads.polyline_colors.len() as u32;
    payloads.polyline_colors.extend_from_slice(color_slice);

    // Hash contract for polyline records: no variant tag. `Shape::Line`
    // and a 2-point `Shape::Polyline { Single(color) }` lower
    // byte-identically by design — sharing a hash is correct. Bezier
    // records tag themselves with `0xCB` + degree (see `lower_bezier`)
    // so curve-derived polylines can never collide with hand-authored
    // ones that happen to share the same flattened bytes.
    let mut h = FxHasher::new();
    h.write(bytemuck::cast_slice(points));
    h.write(bytemuck::cast_slice(color_slice));
    h.write_u32(width.to_bits());
    h.write_u8(mode as u8);
    h.write_u8(cap as u8);
    h.write_u8(join as u8);
    let content_hash = h.finish();

    // Owner-relative AABB computed once here so the encoder hot path
    // stays a straight `extend(map)`. Doesn't include cap-extension;
    // the composer inflates by the tessellator's outer-fringe offset
    // which already covers half-width (sufficient for Butt and a
    // tight upper bound for Square).
    let bbox = points_aabb(points);

    ShapeRecord::Polyline {
        width,
        color_mode: mode,
        cap,
        join,
        points: Span::new(p_start, points.len() as u32),
        colors: Span::new(c_start, color_slice.len() as u32),
        bbox,
        content_hash,
    }
}

/// AABB of a non-empty point slice. Returns the zero rect on empty
/// input — `Shape::is_noop` filters `points.len() < 2` upstream so
/// the empty branch is defensive, not hot.
fn points_aabb(points: &[Vec2]) -> Rect {
    let Some((&first, rest)) = points.split_first() else {
        return Rect::ZERO;
    };
    let (mut lo, mut hi) = (first, first);
    for p in rest {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    Rect {
        min: lo,
        size: Size {
            w: hi.x - lo.x,
            h: hi.y - lo.y,
        },
    }
}

/// Control points for the unified bezier lowering — quadratic carries
/// three, cubic four. Just enough variant info to hash the right bytes
/// and tag the degree; flattening already happened before we get here
/// (different `flatten_*` per variant), so `lower_bezier` itself is
/// degree-agnostic past hashing.
enum BezierInputs {
    Quadratic([Vec2; 3]),
    Cubic([Vec2; 4]),
}

/// Lower a flattened bezier (already in `payloads.bezier_scratch`)
/// into `ShapeRecord::Polyline`: copy points + evaluate colors + track
/// bbox in one fused pass, then hash variant tag + control points +
/// style. `content_hash` covers control points + colors + tolerance +
/// width + cap + join — the flattened output is derived from these
/// and shouldn't shift cache identity by itself.
#[allow(clippy::too_many_arguments)]
fn lower_bezier(
    payloads: &mut ShapePayloads,
    ctrl: BezierInputs,
    width: f32,
    colors: BezierColors,
    cap: LineCap,
    join: LineJoin,
    tolerance: f32,
) -> ShapeRecord {
    let Some((first, rest)) = payloads.bezier_scratch.split_first() else {
        // `flatten_*` always emits at least 2 points (start + end);
        // empty would mean a bezier with no endpoints. Defensive.
        unreachable!("flatten_{{cubic,quadratic}} always emits >= 2 points")
    };

    let p_start = payloads.polyline_points.len() as u32;
    let c_start = payloads.polyline_colors.len() as u32;
    let n = 1 + rest.len();

    // Single fused pass: push point, extend bbox, push color. For
    // `Solid` we push the color once before the loop and leave the
    // per-point branch unset (`ColorMode::Single`).
    let mode = match colors {
        BezierColors::Solid(c) => {
            payloads.polyline_colors.push(c);
            ColorMode::Single
        }
        _ => ColorMode::PerPoint,
    };

    let mut lo = first.p;
    let mut hi = first.p;
    payloads.polyline_points.reserve(n);
    if matches!(mode, ColorMode::PerPoint) {
        payloads.polyline_colors.reserve(n);
    }
    payloads.polyline_points.push(first.p);
    push_color(&mut payloads.polyline_colors, colors, first.t, mode);
    for fp in rest {
        payloads.polyline_points.push(fp.p);
        lo = lo.min(fp.p);
        hi = hi.max(fp.p);
        push_color(&mut payloads.polyline_colors, colors, fp.t, mode);
    }
    let c_len = payloads.polyline_colors.len() as u32 - c_start;

    // Hash contract: bezier-derived records tag with `0xCB` + degree
    // byte (0x01 cubic, 0x02 quadratic), so they can never collide
    // with `lower_polyline`'s untagged hash even if the flattened
    // bytes happened to match a hand-authored polyline.
    let mut h = FxHasher::new();
    h.write_u8(0xCB);
    match ctrl {
        BezierInputs::Cubic(ps) => {
            h.write_u8(0x01);
            h.write(bytemuck::bytes_of(&ps));
        }
        BezierInputs::Quadratic(ps) => {
            h.write_u8(0x02);
            h.write(bytemuck::bytes_of(&ps));
        }
    }
    h.write_u32(width.to_bits());
    h.write_u32(tolerance.to_bits());
    h.write_u8(cap as u8);
    h.write_u8(join as u8);
    colors.hash(&mut h);
    let content_hash = h.finish();

    let bbox = Rect {
        min: lo,
        size: Size {
            w: hi.x - lo.x,
            h: hi.y - lo.y,
        },
    };

    ShapeRecord::Polyline {
        width,
        color_mode: mode,
        cap,
        join,
        points: Span::new(p_start, n as u32),
        colors: Span::new(c_start, c_len),
        bbox,
        content_hash,
    }
}

#[inline]
fn push_color(out: &mut Vec<Color>, colors: BezierColors, t: f32, mode: ColorMode) {
    if matches!(mode, ColorMode::Single) {
        return;
    }
    let c = match colors {
        BezierColors::Solid(_) => return,
        BezierColors::Gradient2(a, b) => lerp_color(a, b, t),
        BezierColors::Gradient3(a, b, c) => eval_color_quadratic(a, b, c, t),
        BezierColors::Gradient4(a, b, c, d) => eval_color_cubic(a, b, c, d, t),
    };
    out.push(c);
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_mesh_hash_excludes_span_offsets() {
        let a = ShapeRecord::Mesh {
            local_rect: None,
            tint: Color {
                r: 0.0,
                g: 1.0,
                b: 0.0,
                a: 1.0,
            },
            vertices: Span::new(0, 3),
            indices: Span::new(0, 3),
            content_hash: 0xdead_beef,
        };
        let b = ShapeRecord::Mesh {
            local_rect: None,
            tint: Color {
                r: 0.0,
                g: 1.0,
                b: 0.0,
                a: 1.0,
            },
            vertices: Span::new(1234, 3),
            indices: Span::new(5678, 3),
            content_hash: 0xdead_beef,
        };
        let mut ha = FxHasher::new();
        let mut hb = FxHasher::new();
        a.hash(&mut ha);
        b.hash(&mut hb);
        assert_eq!(ha.finish(), hb.finish());
    }
}
