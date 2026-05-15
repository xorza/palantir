//! Per-frame bulk geometry arena. Owned by `Host`, threaded `&mut` into
//! `Ui::frame` (record + add_shape lowering) and into the frontend
//! (compose-time polyline tessellation). Cleared at frame start.
//!
//! Replaces the previous three-step copy (user `Mesh` →
//! `Tree.shapes.payloads` → `RenderCmdBuffer.shape_payloads` →
//! `RenderBuffer.meshes.arena`) with a single arena. Shape records on
//! the tree, payloads on the cmd buffer, and `MeshDraw` entries on the
//! render buffer all carry spans into this arena directly.

use crate::common::hash::Hasher as FxHasher;
use crate::forest::shapes::record::ShapeRecord;
use crate::layout::types::span::Span;
use crate::primitives::bezier::FlatPoint;
use crate::primitives::color::Color;
use crate::primitives::mesh::Mesh;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::shape::{ColorMode, LineCap, LineJoin, PolylineColors};
use glam::Vec2;
use std::cell::RefCell;
use std::hash::Hasher;
use std::rc::Rc;

/// Shared, interior-mutable handle to the per-frame arena. Each
/// subsystem (`Ui`, `Frontend`, `WgpuBackend`) holds a clone; `Host`
/// constructs the canonical `Rc` once and injects it into all three at
/// construction time. Phases are sequential (record → encode → compose
/// → upload) so the runtime borrow is never contested in practice;
/// double-borrow would be a wiring bug worth panicking on.
pub type FrameArenaHandle = Rc<RefCell<FrameArena>>;

/// Construct a fresh handle wrapping a default-empty arena.
pub fn new_handle() -> FrameArenaHandle {
    Rc::new(RefCell::new(FrameArena::default()))
}

/// One arena per frame. All bulk shape-geometry bytes live here for
/// the duration of a frame and are read by every later phase via
/// spans recorded on tree shape records and cmd-buffer payloads.
#[derive(Default)]
pub struct FrameArena {
    /// User-supplied mesh geometry plus the compose-time polyline
    /// tessellation output. The latter appends in
    /// [`crate::renderer::frontend::Composer::compose`], so the arena
    /// is mutably borrowed by compose too — not just record.
    pub(crate) meshes: Mesh,
    /// Point storage for `ShapeRecord::Polyline`. Indexed by the
    /// record's `points` `Span`.
    pub(crate) polyline_points: Vec<Vec2>,
    /// Color storage for `ShapeRecord::Polyline`. Length per
    /// record is 1, `points.len()`, or `points.len() - 1` per
    /// `ColorMode`.
    pub(crate) polyline_colors: Vec<Color>,
    /// Scratch for bezier flattening. Cleared (length only) at the
    /// top of every bezier lowering; the flattened points are copied
    /// into `polyline_points` immediately after.
    pub(crate) bezier_scratch: Vec<FlatPoint>,
}

/// Control points for the unified bezier lowering — quadratic carries
/// three, cubic four. Just enough variant info to hash the right bytes
/// and tag the degree; flattening already happened before we get here
/// (different `flatten_*` per variant), so `lower_bezier` itself is
/// degree-agnostic past hashing.
pub(crate) enum BezierInputs {
    Quadratic([Vec2; 3]),
    Cubic([Vec2; 4]),
}

impl FrameArena {
    pub(crate) fn clear(&mut self) {
        self.meshes.clear();
        self.polyline_points.clear();
        self.polyline_colors.clear();
        self.bezier_scratch.clear();
    }

    /// Lower a (points, colors, width) authoring shape into a
    /// `ShapeRecord::Polyline`: validate `colors` length against
    /// `points.len()`, copy both into the arena, compute the content
    /// hash. `Shape::Line` and `Shape::Polyline` both route through
    /// this — one record path downstream.
    pub(crate) fn lower_polyline(
        &mut self,
        points: &[Vec2],
        colors: PolylineColors<'_>,
        width: f32,
        cap: LineCap,
        join: LineJoin,
    ) -> ShapeRecord {
        let (mode, color_slice): (ColorMode, &[Color]) = match &colors {
            PolylineColors::Single(c) => (ColorMode::Single, std::slice::from_ref(c)),
            PolylineColors::PerPoint(cs) => (ColorMode::PerPoint, cs),
            PolylineColors::PerSegment(cs) => (ColorMode::PerSegment, cs),
        };

        let p_start = self.polyline_points.len() as u32;
        self.polyline_points.extend_from_slice(points);
        let c_start = self.polyline_colors.len() as u32;
        self.polyline_colors.extend_from_slice(color_slice);

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

    /// Lower a flattened bezier (already in `self.bezier_scratch`) into
    /// `ShapeRecord::Polyline`: copy points and track bbox in one pass,
    /// push the single color, hash variant tag + control points + style.
    pub(crate) fn lower_bezier(
        &mut self,
        ctrl: BezierInputs,
        width: f32,
        color: Color,
        cap: LineCap,
        join: LineJoin,
        tolerance: f32,
    ) -> ShapeRecord {
        let Some((first, rest)) = self.bezier_scratch.split_first() else {
            unreachable!("flatten_{{cubic,quadratic}} always emits >= 2 points")
        };

        let p_start = self.polyline_points.len() as u32;
        let c_start = self.polyline_colors.len() as u32;
        let n = 1 + rest.len();

        let mut lo = first.p;
        let mut hi = first.p;
        self.polyline_points.reserve(n);
        self.polyline_points.push(first.p);
        for fp in rest {
            self.polyline_points.push(fp.p);
            lo = lo.min(fp.p);
            hi = hi.max(fp.p);
        }
        self.polyline_colors.push(color);

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
        h.write(bytemuck::bytes_of(&color));
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
            color_mode: ColorMode::Single,
            cap,
            join,
            points: Span::new(p_start, n as u32),
            colors: Span::new(c_start, 1),
            bbox,
            content_hash,
        }
    }
}

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
