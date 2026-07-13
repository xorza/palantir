//! Per-frame bulk geometry arena. Owned by `WindowRenderer`, cloned (cheap, Rc)
//! into every subsystem that touches per-frame mesh / polyline / fmt
//! bytes (`Ui`, `Frontend`, `WgpuBackend`). Cleared at frame start.
//!
//! Replaces the previous three-step copy (user `Mesh` →
//! `Tree.shapes.payloads` → `RenderCmdBuffer.shape_payloads` →
//! `RenderBuffer.meshes.arena`) with a single arena. Shape records on
//! the tree, payloads on the cmd buffer, and `MeshDraw` entries on the
//! render buffer all carry spans into this arena directly.

use crate::common::hash::Hasher as FxHasher;
use crate::forest::rollups::NodeHash;
use crate::forest::shapes::record::{
    ChromeRow, LoweredGradient, LoweredShadow, ShapeBrush, ShapeRecord, ShapeStroke,
};
use crate::primitives::arc::arc_bbox;
use crate::primitives::background::Background;
use crate::primitives::bezier::{CurveBounds, cubic_bezier_bbox, quadratic_to_cubic};
use crate::primitives::brush::Brush;
use crate::primitives::color::{Color, ColorU8};
use crate::primitives::interned_str::InternedStr;
use crate::primitives::mesh::Mesh;
use crate::primitives::paint::FillKind;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::span::Span;
use crate::primitives::stroke::Stroke;
use crate::renderer::gradient_atlas::GradientAtlas;
use crate::renderer::stroke_tessellate::{HALF_FRINGE, MITER_LIMIT};
use crate::shape::{ColorMode, LineCap, LineJoin, PolylineColors};
use glam::Vec2;
use std::cell::{Ref, RefCell, RefMut};
use std::fmt::Write as _;
use std::hash::Hasher;
use std::rc::Rc;

/// Shared per-frame arena. `WindowRenderer` constructs one and clones it into
/// every subsystem (`Ui`, `Frontend`, `WgpuBackend`). Phases run
/// sequentially (record → encode → compose → upload) so the underlying
/// borrow is never contested; a double-borrow indicates a wiring bug
/// and panics.
///
/// User-facing operations (`clear`, `lower_*`, `intern_fmt`) take
/// `&self` and borrow internally — call sites never touch RefCell.
/// Pass-orchestration code (encode/compose/intrinsic) reaches the raw
/// storage via [`Self::inner`] / [`Self::inner_mut`] once per pass and
/// hands `&FrameArenaInner` down through the pass.
#[derive(Clone, Default, Debug)]
pub struct FrameArena(Rc<RefCell<FrameArenaInner>>);

/// One arena per frame. All bulk shape-geometry bytes live here for
/// the duration of a frame and are read by every later phase via
/// spans recorded on tree shape records and cmd-buffer payloads.
#[derive(Default, Debug)]
pub(crate) struct FrameArenaInner {
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
    /// `ColorMode`. Stored as `ColorU8` (4 B/elem, same precision
    /// the tessellator's destination `MeshVertex.color` uses) —
    /// quantization happens once at lowering, not per-emitted-vertex.
    pub(crate) polyline_colors: Vec<ColorU8>,
    /// Frame-scoped gradient payloads. `ShapeBrush::Gradient(id)` (set
    /// by `lower_brush`) indexes into this vec. Cross-tree — keeping
    /// it on the frame arena means chrome lowering on one tree and
    /// shape lowering on another share one pool, and the encoder only
    /// needs the arena (not the originating tree) to resolve a
    /// gradient id.
    pub(crate) gradients: Vec<LoweredGradient>,
    /// `Ui::fmt` formatter scratch. The `InternedStr::Interned { span }`
    /// handle returned by [`FrameArena::intern_fmt`] points into this
    /// buffer; the `Borrowed` / `Owned` carriers don't touch it (they
    /// keep bytes inline on `ShapeRecord::Text`). Cross-tree on purpose
    /// so `Interned` handles survive `Ui::layer(...)` scopes. Cleared
    /// per frame, capacity retained — steady-state `ui.fmt(...)` flows
    /// skip the `format!() → String` allocation entirely.
    pub(crate) fmt_scratch: String,
}

/// Stable content hash for a gradient variant: discriminant byte
/// then the gradient's `Hash` impl (which hashes f32 canon-bits).
/// Lets `ShapeRecord::Hash` stay context-free — we capture the hash
/// at lowering and stamp it on the record alongside the
/// `GradientId`, so downstream cache keys don't need the arena.
#[inline]
fn grad_hash<G: std::hash::Hash>(tag: u8, g: &G) -> u64 {
    let mut h = FxHasher::new();
    h.write_u8(tag);
    g.hash(&mut h);
    h.finish()
}

impl FrameArena {
    /// Borrow the raw inner storage for the duration of a pass. Used
    /// by encode/compose/intrinsic — the orchestrator opens one borrow
    /// at pass entry and threads `&FrameArenaInner` down so per-node
    /// code touches fields directly. Authoring code (widgets, tests)
    /// should prefer the `lower_*` / `intern_fmt` methods on `Self`.
    pub(crate) fn inner(&self) -> Ref<'_, FrameArenaInner> {
        self.0.borrow()
    }

    /// Mutable counterpart to [`Self::inner`]. Composer holds this for
    /// the full compose pass (it appends polyline-tessellation output);
    /// `Frontend::build` opens a single guard for encode + compose.
    pub(crate) fn inner_mut(&self) -> RefMut<'_, FrameArenaInner> {
        self.0.borrow_mut()
    }

    /// Drop all per-frame storage. Run once at frame start.
    pub(crate) fn clear(&self) {
        let mut a = self.0.borrow_mut();
        a.meshes.clear();
        a.polyline_points.clear();
        a.polyline_colors.clear();
        a.gradients.clear();
        a.fmt_scratch.clear();
    }

    /// Pre-computed FxHash of `s` for stamping into
    /// `ShapeRecord::Text.text_hash`. Stateless.
    pub(crate) fn hash_text(s: &str) -> u64 {
        use std::hash::Hash;
        let mut h = FxHasher::new();
        s.hash(&mut h);
        h.finish()
    }

    /// Copy `s` into the per-frame text arena and return an
    /// `InternedStr::Interned` handle. Backs [`crate::Ui::intern`] for
    /// the format-less case (plain `&str` borrow, no `format_args!`).
    #[must_use]
    pub(crate) fn intern_str(&self, s: &str) -> InternedStr {
        let mut a = self.0.borrow_mut();
        let start = a.fmt_scratch.len();
        a.fmt_scratch.push_str(s);
        let hash = Self::hash_text(s);
        InternedStr::Interned {
            span: Span::new(start as u32, s.len() as u32),
            hash,
        }
    }

    /// Format `args` directly into the per-frame text arena and return
    /// an `InternedStr::Interned` handle that spans the freshly-written
    /// bytes. Backs [`crate::Ui::fmt`].
    #[must_use]
    pub(crate) fn intern_fmt(&self, args: std::fmt::Arguments<'_>) -> InternedStr {
        let mut a = self.0.borrow_mut();
        let start = a.fmt_scratch.len();
        a.fmt_scratch.write_fmt(args).unwrap();
        let end = a.fmt_scratch.len();
        let bytes = &a.fmt_scratch.as_str()[start..end];
        let hash = Self::hash_text(bytes);
        InternedStr::Interned {
            span: Span::new(start as u32, (end - start) as u32),
            hash,
        }
    }

    /// Lower a user-side `Brush` to the storage form: `Solid` stays
    /// inline, gradients push to `inner.gradients` and return an
    /// indexing `ShapeBrush::Gradient`. The pre-computed content hash
    /// is returned alongside so the caller can stamp it into the
    /// `ShapeRecord` / `ChromeRow` and keep their `Hash` impls
    /// context-free (no need to thread the arena into hashing).
    pub(crate) fn lower_brush(&self, brush: Brush, atlas: &GradientAtlas) -> LoweredBrush {
        self.0.borrow_mut().lower_brush_inner(&brush, atlas)
    }

    /// Lower a user-facing `Background` to a `ChromeRow`. Same
    /// gradient lowering as `Shapes::add` uses for `RoundedRect.fill`,
    /// so chrome and shape paints share one pool. Takes `bg` by
    /// reference — `Background` is 168 B and the recording chain
    /// threads it through 4 functions; the per-field reads below copy
    /// the small fields locally as needed.
    pub(crate) fn lower_background(&self, bg: &Background, atlas: &GradientAtlas) -> ChromeRow {
        let mut a = self.0.borrow_mut();
        let LoweredBrush {
            brush: fill,
            hash: fill_grad_hash,
        } = a.lower_brush_inner(&bg.fill, atlas);
        let stroke = ShapeStroke::from(&bg.stroke);
        let corners = bg.corners;
        let shadow: LoweredShadow = bg.shadow.into();
        // Canonical authoring hash: fold all inputs into one
        // `Hasher::pod` call. Hashing field-by-field via 5 separate
        // `Hasher::write*` calls (the prior shape) paid `hash_bytes`
        // setup + final `add_to_hash` 5 times — ~40 cycles of overhead
        // dominated `lower_background`'s self-time (~0.5% of frame
        // total). Field order is layout-engineered to avoid internal
        // padding (u64s first → 2-align Pod structs → tag);
        // `padding_struct` fills the tail so `NoUninit` is sound.
        #[repr(C)]
        #[padding_struct::padding_struct]
        #[derive(Clone, Copy, bytemuck::NoUninit, bytemuck::Zeroable)]
        struct ChromeHashBytes {
            fill_payload: u64, // ColorF16-as-u64 (Solid) or fill_grad_hash (Gradient)
            corners_u64: u64,
            stroke: ShapeStroke,   // 10 B align 2
            shadow: LoweredShadow, // 18 B align 2
            fill_tag: u8,
        }
        let fill_payload: u64 = match fill {
            ShapeBrush::Solid(c) => c.as_u64(),
            ShapeBrush::Gradient(_) => fill_grad_hash,
        };
        let fill_tag: u8 = match fill {
            ShapeBrush::Solid(_) => 0,
            ShapeBrush::Gradient(_) => 1,
        };
        let packed = ChromeHashBytes {
            fill_payload,
            corners_u64: bytemuck::cast(corners),
            stroke,
            shadow,
            fill_tag,
            ..bytemuck::Zeroable::zeroed()
        };
        let mut h = FxHasher::new();
        h.pod(&packed);
        let hash = NodeHash(h.finish());
        ChromeRow {
            fill,
            stroke,
            corners,
            shadow,
            hash,
        }
    }

    /// Lower a (points, colors, width) authoring shape into a
    /// `ShapeRecord::Polyline`: copy points and colors into the arena,
    /// compute the content hash. Only `Shape::Polyline` routes through
    /// this — the CPU stroke tessellator is reserved for genuinely
    /// multi-segment strokes with interior joins; every single-stroke
    /// shape (`Line`/beziers/`Arc`) rides the GPU curve pipeline.
    pub(crate) fn lower_polyline(
        &self,
        points: &[Vec2],
        colors: PolylineColors<'_>,
        width: f32,
        cap: LineCap,
        join: LineJoin,
    ) -> ShapeRecord {
        self.0
            .borrow_mut()
            .lower_polyline_inner(points, colors, width, cap, join)
    }

    /// Lower a cubic bezier into a `ShapeRecord::Curve`. Tessellation
    /// happens GPU-side at draw time — no CPU flattening, no per-curve
    /// vertex/index allocation. The composer derives sub-instance count
    /// from the post-transform control-polygon length. `brush` may be
    /// `Brush::Solid` or `Brush::Linear` — the linear gradient samples
    /// along the curve parameter `t` and its `angle` is ignored;
    /// `Radial`/`Conic` panic at lowering (no meaningful axis on a
    /// 1-D stroke).
    pub(crate) fn lower_cubic_bezier(
        &self,
        ctrl: [Vec2; 4],
        width: f32,
        brush: Brush,
        cap: LineCap,
        atlas: &GradientAtlas,
    ) -> ShapeRecord {
        assert_curve_brush(&brush);
        let lowered = self.0.borrow_mut().lower_brush_inner(&brush, atlas);
        lower_curve_inner(ctrl, width, lowered, cap, 0)
    }

    /// Lower a quadratic bezier by promoting it to a cubic and going
    /// through `lower_cubic_bezier`. Exact reparameterization:
    /// `q1' = q0 + 2/3·(c - q0)`, `q2' = q2 + 2/3·(c - q2)`.
    pub(crate) fn lower_quadratic_bezier(
        &self,
        ctrl: [Vec2; 3],
        width: f32,
        brush: Brush,
        cap: LineCap,
        atlas: &GradientAtlas,
    ) -> ShapeRecord {
        assert_curve_brush(&brush);
        let [p0, c, p2] = ctrl;
        let cubic = quadratic_to_cubic(p0, c, p2);
        let lowered = self.0.borrow_mut().lower_brush_inner(&brush, atlas);
        lower_curve_inner([p0, cubic.c1, cubic.c2, p2], width, lowered, cap, 1)
    }

    /// Lower a straight line as a degenerate cubic on the native GPU
    /// stroke path. Inner control points sit on the segment's thirds,
    /// so `B(t) = a + (b - a)·t` exactly — `t` (and thus a gradient
    /// brush) runs linearly from `a` to `b`. The composer's flatness
    /// fast-path keeps the collinear cubic a single GPU instance.
    pub(crate) fn lower_line(
        &self,
        a: Vec2,
        b: Vec2,
        width: f32,
        brush: Brush,
        cap: LineCap,
        atlas: &GradientAtlas,
    ) -> ShapeRecord {
        assert_curve_brush(&brush);
        let lowered = self.0.borrow_mut().lower_brush_inner(&brush, atlas);
        let third = (b - a) / 3.0;
        lower_curve_inner([a, a + third, b - third, b], width, lowered, cap, 2)
    }

    /// Lower a circular arc into a [`ShapeRecord::Arc`]. Same native-GPU
    /// stroke path as the béziers — no CPU flattening; the shader
    /// evaluates the exact circle, so the record stores center/radius/
    /// angles verbatim. `brush` follows the curve contract (`Solid` /
    /// `Linear` sampled along the sweep; `Radial`/`Conic` rejected).
    /// `|sweep| ≤ 2π` is hard-asserted: a longer sweep would repaint
    /// pixels and double-blend a translucent stroke.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn lower_arc(
        &self,
        center: Vec2,
        radius: f32,
        start_angle: f32,
        sweep: f32,
        width: f32,
        brush: Brush,
        cap: LineCap,
        atlas: &GradientAtlas,
    ) -> ShapeRecord {
        assert_curve_brush(&brush);
        assert!(
            sweep.abs() <= std::f32::consts::TAU + 1.0e-4,
            "Shape::Arc sweep {sweep} exceeds a full circle (±2π)"
        );
        let lowered = self.0.borrow_mut().lower_brush_inner(&brush, atlas);
        let a1 = start_angle + sweep;
        let CurveBounds { lo, hi } = arc_bbox(center, radius, start_angle, a1);
        let bbox = padded_bbox(lo, hi, stroke_pad(width, cap));
        // `0xCA` prefix keeps arc content hashes disjoint from the
        // bezier family's `0xCB00 | degree` tags.
        let mut h = FxHasher::new();
        h.write_u16(0xCA00);
        h.write(bytemuck::bytes_of(&[
            center.x,
            center.y,
            radius,
            start_angle,
            sweep,
        ]));
        h.write_u64((u64::from(width.to_bits()) << 8) | u64::from(cap as u8));
        let content_hash = h.finish();
        ShapeRecord::Arc {
            center,
            radius,
            a0: start_angle,
            a1,
            width,
            fill: lowered.brush,
            fill_grad_hash: lowered.hash,
            cap,
            bbox,
            content_hash,
        }
    }

    /// Lower a triangle into a [`ShapeRecord::Triangle`]. Solid fill only —
    /// gradients can't ride the reused quad-instance lanes, so `fill` is
    /// `expect_solid`'d here (rejecting a gradient at the authoring boundary).
    /// `bbox` is the owner-local AABB of `a`/`b`/`c` inflated by
    /// `radius + AA fringe` (the SDF offsets the shape outward by `radius`;
    /// the stroke is inner-edge and adds no outward reach), so damage and
    /// clip-cull cover the rounded, antialiased extent. No arena touch (no
    /// gradient to register); `&self` mirrors the sibling `lower_*` call shape.
    pub(crate) fn lower_triangle(
        &self,
        a: Vec2,
        b: Vec2,
        c: Vec2,
        radius: f32,
        fill: Brush,
        stroke: Stroke,
    ) -> ShapeRecord {
        let lo = a.min(b).min(c);
        let hi = a.max(b).max(c);
        let pad = radius.max(0.0) + HALF_FRINGE;
        let bbox = padded_bbox(lo, hi, pad);
        ShapeRecord::Triangle {
            a,
            b,
            c,
            radius,
            fill: fill.expect_solid().into(),
            stroke: ShapeStroke::from(stroke),
            bbox,
        }
    }
}

/// GPU-stroked shapes (Line / beziers / Arc) accept `Brush::Solid` or
/// `Brush::Linear`. Radial and conic gradients project onto a 2-D
/// shape; there is no obvious projection onto a 1-D stroke (the chord
/// and the curve's bounding rect are both poor proxies), so we reject
/// at lowering rather than silently pick one. If a stroke-friendly
/// radial/conic interpretation shows up, lift this gate.
fn assert_curve_brush(brush: &Brush) {
    match brush {
        Brush::Solid(_) | Brush::Linear(_) => {}
        Brush::Radial(_) | Brush::Conic(_) => {
            panic!(
                "stroked shapes (Line / beziers / Arc): only Brush::Solid and Brush::Linear are supported"
            )
        }
    }
}

/// Conservative bbox padding for a GPU-stroked shape: half-width, plus
/// the cap extension (`Square`/`Round` reach `width/2` past each
/// endpoint along the local tangent — direction varies, so the
/// axis-aligned pad takes it on every side), plus the AA fringe.
fn stroke_pad(width: f32, cap: LineCap) -> f32 {
    let half = (width * 0.5).max(0.0);
    let cap_extent = match cap {
        LineCap::Butt => 0.0,
        LineCap::Square | LineCap::Round => half,
    };
    half + cap_extent + HALF_FRINGE
}

/// Build a `ShapeRecord::Curve` from cubic control points. `degree_tag`
/// (0 = cubic, 1 = quadratic-derived, 2 = line-derived) is folded into
/// the content hash so shapes with bit-identical post-promotion CPs
/// still hash distinctly (matches the old bezier-hash discipline). The
/// brush variant (solid colour or gradient hash) is folded into the
/// record hash separately by `ShapeRecord::Hash`, so this function's
/// hash only covers geometry + cap.
fn lower_curve_inner(
    ctrl: [Vec2; 4],
    width: f32,
    fill: LoweredBrush,
    cap: LineCap,
    degree_tag: u8,
) -> ShapeRecord {
    let [p0, p1, p2, p3] = ctrl;

    let CurveBounds { lo, hi } = cubic_bezier_bbox(p0, p1, p2, p3);
    let bbox = padded_bbox(lo, hi, stroke_pad(width, cap));
    let mut h = FxHasher::new();
    h.write_u16(0xCB00 | u16::from(degree_tag));
    h.write(bytemuck::bytes_of(&ctrl));
    // Pack width bits + cap tag into one 64-bit hasher write — single
    // dispatch instead of two.
    h.write_u64((u64::from(width.to_bits()) << 8) | u64::from(cap as u8));
    let content_hash = h.finish();
    ShapeRecord::Curve {
        p0,
        p1,
        p2,
        p3,
        width,
        fill: fill.brush,
        fill_grad_hash: fill.hash,
        cap,
        bbox,
        content_hash,
    }
}

/// Result of lowering a user-side `Brush`. `brush` is the storage form
/// (`Solid` inline or `Gradient(id)` indexing into the arena's
/// gradient pool); `hash` is the pre-computed content hash so the
/// caller can stamp it into a `ShapeRecord` / `ChromeRow` without
/// threading the arena into their `Hash` impls. `hash == 0` for
/// `Solid` (no gradient payload to identify).
#[derive(Clone, Copy, Debug)]
pub(crate) struct LoweredBrush {
    pub(crate) brush: ShapeBrush,
    pub(crate) hash: u64,
}

impl FrameArenaInner {
    fn lower_brush_inner(&mut self, brush: &Brush, atlas: &GradientAtlas) -> LoweredBrush {
        let (kind, axis, stops, interp, hash) = match brush {
            Brush::Solid(c) => {
                return LoweredBrush {
                    brush: ShapeBrush::Solid((*c).into()),
                    hash: 0,
                };
            }
            Brush::Linear(g) => {
                let h = grad_hash(0, g);
                (FillKind::linear(g.spread), g.axis(), &g.stops, g.interp, h)
            }
            Brush::Radial(g) => {
                let h = grad_hash(1, g);
                (FillKind::radial(g.spread), g.axis(), &g.stops, g.interp, h)
            }
            Brush::Conic(g) => {
                let h = grad_hash(2, g);
                (FillKind::conic(g.spread), g.axis(), &g.stops, g.interp, h)
            }
        };
        let row = atlas.register_stops(stops, interp);
        let id = self.gradients.len() as u32;
        self.gradients.push(LoweredGradient { axis, row, kind });
        LoweredBrush {
            brush: ShapeBrush::Gradient(id),
            hash,
        }
    }

    fn lower_polyline_inner(
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
        let c_start = self.polyline_colors.len() as u32;
        let bbox = match points.split_first() {
            None => {
                self.polyline_colors
                    .extend(color_slice.iter().map(|&c| ColorU8::from(c)));
                Rect::ZERO
            }
            Some((&first, rest)) => {
                let mut lo = first;
                let mut hi = first;
                self.polyline_points.reserve(points.len());
                self.polyline_points.push(first);
                for &p in rest {
                    self.polyline_points.push(p);
                    lo = lo.min(p);
                    hi = hi.max(p);
                }
                self.polyline_colors
                    .extend(color_slice.iter().map(|&c| ColorU8::from(c)));
                inflate_stroke_bbox(lo, hi, width, cap, join)
            }
        };

        // Hash contract for polyline records: no variant tag needed —
        // polylines are the only shape lowering into this record. The
        // GPU-stroke records tag themselves (`0xCB` + degree for the
        // bezier family, `0xCA` for arcs — see `lower_curve_inner` /
        // `lower_arc`) and hash under their own record tag anyway.
        let mut h = FxHasher::new();
        h.write(bytemuck::cast_slice(points));
        h.write(bytemuck::cast_slice(color_slice));
        let style = (width.to_bits() as u64) << 24
            | ((mode as u64) << 16)
            | ((cap as u64) << 8)
            | (join as u64);
        h.write_u64(style);
        let content_hash = h.finish();

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
}

/// Inflate the centerline AABB `[lo, hi]` of a stroked polyline so it
/// conservatively covers the painted extent: stroke half-width on every
/// side, miter-limit slack at sharp joins (matches the bevel fallback
/// in `stroke_tessellate`), `Square` cap projection past endpoints, and
/// the AA fringe. Damage and per-shape clipping key on this — undersizing
/// here leaves miter spikes / square caps unclipped/undamaged.
fn inflate_stroke_bbox(lo: Vec2, hi: Vec2, width: f32, cap: LineCap, join: LineJoin) -> Rect {
    let half = width * 0.5;
    let join_extent = if matches!(join, LineJoin::Miter) {
        half * MITER_LIMIT
    } else {
        half
    };
    let cap_extent = if matches!(cap, LineCap::Square) {
        half
    } else {
        0.0
    };
    let pad = join_extent.max(cap_extent) + HALF_FRINGE;
    padded_bbox(lo, hi, pad)
}

/// Axis-aligned bbox of `[lo, hi]` inflated by `pad` on every side.
/// Shared by the triangle / curve / stroke lowering paths above, which
/// differ only in how they derive `lo` / `hi` / `pad`.
fn padded_bbox(lo: Vec2, hi: Vec2, pad: f32) -> Rect {
    Rect {
        min: Vec2::new(lo.x - pad, lo.y - pad),
        size: Size {
            w: (hi.x - lo.x) + 2.0 * pad,
            h: (hi.y - lo.y) + 2.0 * pad,
        },
    }
}
