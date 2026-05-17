//! Per-frame bulk geometry arena. Owned by `Host`, cloned (cheap, Rc)
//! into every subsystem that touches per-frame mesh / polyline / fmt
//! bytes (`Ui`, `Frontend`, `WgpuBackend`). Cleared at frame start.
//!
//! Replaces the previous three-step copy (user `Mesh` →
//! `Tree.shapes.payloads` → `RenderCmdBuffer.shape_payloads` →
//! `RenderBuffer.meshes.arena`) with a single arena. Shape records on
//! the tree, payloads on the cmd buffer, and `MeshDraw` entries on the
//! render buffer all carry spans into this arena directly.

use crate::common::hash::Hasher as FxHasher;
use crate::forest::shapes::record::{
    ChromeRow, LoweredGradient, ShapeBrush, ShapeRecord, ShapeStroke,
};
use crate::primitives::background::Background;
use crate::primitives::bezier::{FlatPoint, flatten_cubic, flatten_quadratic};
use crate::primitives::brush::Brush;
use crate::primitives::color::{Color, ColorU8};
use crate::primitives::interned_str::InternedStr;
use crate::primitives::mesh::Mesh;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::span::Span;
use crate::renderer::gradient_atlas::GradientAtlas;
use crate::renderer::quad::FillKind;
use crate::shape::{ColorMode, LineCap, LineJoin, PolylineColors};
use glam::Vec2;
use std::cell::{Ref, RefCell, RefMut};
use std::fmt::Write as _;
use std::hash::Hasher;
use std::rc::Rc;

/// Shared per-frame arena. `Host` constructs one and clones it into
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
#[derive(Clone, Default)]
pub struct FrameArena(Rc<RefCell<FrameArenaInner>>);

/// One arena per frame. All bulk shape-geometry bytes live here for
/// the duration of a frame and are read by every later phase via
/// spans recorded on tree shape records and cmd-buffer payloads.
#[derive(Default)]
pub struct FrameArenaInner {
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
    /// Scratch for bezier flattening. Cleared (length only) at the
    /// top of every bezier lowering; the flattened points are copied
    /// into `polyline_points` immediately after.
    pub(crate) bezier_scratch: Vec<FlatPoint>,
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

/// Control points for the unified bezier lowering — quadratic carries
/// three, cubic four. Just enough variant info to hash the right bytes
/// and tag the degree; flattening already happened before we get here
/// (different `flatten_*` per variant), so `lower_bezier_inner` itself
/// is degree-agnostic past hashing.
enum BezierInputs {
    Quadratic([Vec2; 3]),
    Cubic([Vec2; 4]),
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
        a.bezier_scratch.clear();
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
    pub(crate) fn intern_str(&self, s: &str) -> InternedStr<'static> {
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
    pub(crate) fn intern_fmt(&self, args: std::fmt::Arguments<'_>) -> InternedStr<'static> {
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
        self.0.borrow_mut().lower_brush_inner(brush, atlas)
    }

    /// Lower a user-facing `Background` to a `ChromeRow`. Same
    /// gradient lowering as `Shapes::add` uses for `RoundedRect.fill`,
    /// so chrome and shape paints share one pool.
    pub(crate) fn lower_background(&self, bg: Background, atlas: &GradientAtlas) -> ChromeRow {
        let mut a = self.0.borrow_mut();
        let LoweredBrush {
            brush: fill,
            hash: fill_grad_hash,
        } = a.lower_brush_inner(bg.fill, atlas);
        ChromeRow {
            fill,
            stroke: ShapeStroke::from(bg.stroke),
            radius: bg.radius,
            shadow: bg.shadow.into(),
            fill_grad_hash,
        }
    }

    /// Lower a (points, colors, width) authoring shape into a
    /// `ShapeRecord::Polyline`: copy points and colors into the arena,
    /// compute the content hash. `Shape::Line` and `Shape::Polyline`
    /// both route through this — one record path downstream.
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

    /// Flatten a cubic bezier into the per-frame scratch and lower it
    /// to a `ShapeRecord::Polyline`. Combined so the scratch borrow
    /// doesn't escape the call.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn lower_cubic_bezier(
        &self,
        p0: Vec2,
        p1: Vec2,
        p2: Vec2,
        p3: Vec2,
        width: f32,
        color: Color,
        cap: LineCap,
        join: LineJoin,
        tolerance: f32,
    ) -> ShapeRecord {
        let mut a = self.0.borrow_mut();
        a.bezier_scratch.clear();
        flatten_cubic(p0, p1, p2, p3, tolerance, &mut a.bezier_scratch);
        a.lower_bezier_inner(
            BezierInputs::Cubic([p0, p1, p2, p3]),
            width,
            color,
            cap,
            join,
            tolerance,
        )
    }

    /// Flatten a quadratic bezier into the per-frame scratch and lower
    /// it to a `ShapeRecord::Polyline`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn lower_quadratic_bezier(
        &self,
        p0: Vec2,
        p1: Vec2,
        p2: Vec2,
        width: f32,
        color: Color,
        cap: LineCap,
        join: LineJoin,
        tolerance: f32,
    ) -> ShapeRecord {
        let mut a = self.0.borrow_mut();
        a.bezier_scratch.clear();
        flatten_quadratic(p0, p1, p2, tolerance, &mut a.bezier_scratch);
        a.lower_bezier_inner(
            BezierInputs::Quadratic([p0, p1, p2]),
            width,
            color,
            cap,
            join,
            tolerance,
        )
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
    fn lower_brush_inner(&mut self, brush: Brush, atlas: &GradientAtlas) -> LoweredBrush {
        let (kind, axis, stops, interp, hash) = match brush {
            Brush::Solid(c) => {
                return LoweredBrush {
                    brush: ShapeBrush::Solid(c.into()),
                    hash: 0,
                };
            }
            Brush::Linear(g) => {
                let h = grad_hash(0, &g);
                (FillKind::linear(g.spread), g.axis(), g.stops, g.interp, h)
            }
            Brush::Radial(g) => {
                let h = grad_hash(1, &g);
                (FillKind::radial(g.spread), g.axis(), g.stops, g.interp, h)
            }
            Brush::Conic(g) => {
                let h = grad_hash(2, &g);
                (FillKind::conic(g.spread), g.axis(), g.stops, g.interp, h)
            }
        };
        let row = atlas.register_stops(&stops, interp);
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
                Rect {
                    min: lo,
                    size: Size {
                        w: hi.x - lo.x,
                        h: hi.y - lo.y,
                    },
                }
            }
        };

        // Hash contract for polyline records: no variant tag. `Shape::Line`
        // and a 2-point `Shape::Polyline { Single(color) }` lower
        // byte-identically by design — sharing a hash is correct. Bezier
        // records tag themselves with `0xCB` + degree (see
        // `lower_bezier_inner`) so curve-derived polylines can never
        // collide with hand-authored ones that happen to share the same
        // flattened bytes.
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

    fn lower_bezier_inner(
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
        self.polyline_colors.push(color.into());

        let mut h = FxHasher::new();
        let degree = match ctrl {
            BezierInputs::Cubic(_) => 0x01_u16,
            BezierInputs::Quadratic(_) => 0x02_u16,
        };
        h.write_u16(0xCB00 | degree);
        match ctrl {
            BezierInputs::Cubic(ps) => h.write(bytemuck::bytes_of(&ps)),
            BezierInputs::Quadratic(ps) => h.write(bytemuck::bytes_of(&ps)),
        }
        let dims = ((width.to_bits() as u64) << 32) | tolerance.to_bits() as u64;
        h.write_u64(dims);
        h.write_u16(((cap as u16) << 8) | join as u16);
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
