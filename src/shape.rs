use crate::layout::types::align::Align;
use crate::primitives::mesh::Mesh;
use crate::primitives::{
    approx::{noop_f32, vec2_approx_eq},
    brush::Brush,
    color::Color,
    corners::Corners,
    rect::Rect,
    shadow::Shadow,
    stroke::Stroke,
};
use crate::text::FontFamily;
use glam::Vec2;
use std::borrow::Cow;

/// User-facing paint primitive. Pushed into the active tree via
/// [`crate::Ui::add_shape`], which copies the data into the per-frame
/// arena and converts to the internal [`ShapeRecord`] form.
///
/// The `'a` lifetime is borrowed by [`Shape::Mesh`]; the other three
/// variants infer `Shape<'static>` and behave identically at the call
/// site to today's owned variants.
#[derive(Clone, Debug)]
pub enum Shape<'a> {
    RoundedRect {
        local_rect: Option<Rect>,
        radius: Corners,
        fill: Brush,
        stroke: Stroke,
    },
    /// Two-point stroked line — ergonomic shorthand for a 2-point
    /// `Polyline { Single(color) }`. Lowers to `ShapeRecord::Polyline`
    /// at authoring time; there's no `ShapeRecord::Line`. `cap`
    /// applies to both endpoints; `join` is unused (no interior).
    Line {
        a: Vec2,
        b: Vec2,
        width: f32,
        brush: Brush,
        cap: LineCap,
        join: LineJoin,
    },
    /// Stroked polyline with per-vertex or per-segment coloring. The
    /// framework copies `points` and `colors` into the active tree's
    /// per-frame arenas at `add_shape` time, so the borrows only have
    /// to outlive the call. `colors` length is constrained by `mode`
    /// (see [`PolylineColors`]); mismatches hard-assert.
    Polyline {
        points: &'a [Vec2],
        colors: PolylineColors<'a>,
        width: f32,
        cap: LineCap,
        join: LineJoin,
    },
    /// Cubic Bezier curve, stroked. Flattened to a polyline at
    /// authoring time (adaptive subdivision in
    /// [`crate::primitives::bezier`]) and lowered to
    /// `ShapeRecord::Polyline` — no dedicated record/cmd path.
    /// `tolerance` is the chord-deviation budget in logical px
    /// (tighter = more segments); values `<= EPS` clamp to `EPS`.
    /// Color is solid for now — parametric-t gradient evaluation
    /// (see `FlatPoint.t`) is reserved for a follow-up.
    CubicBezier {
        p0: Vec2,
        p1: Vec2,
        p2: Vec2,
        p3: Vec2,
        width: f32,
        brush: Brush,
        cap: LineCap,
        join: LineJoin,
        tolerance: f32,
    },
    /// Quadratic Bezier curve, stroked. See [`Shape::CubicBezier`].
    QuadraticBezier {
        p0: Vec2,
        p1: Vec2,
        p2: Vec2,
        width: f32,
        brush: Brush,
        cap: LineCap,
        join: LineJoin,
        tolerance: f32,
    },
    Text {
        local_rect: Option<Rect>,
        text: Cow<'static, str>,
        brush: Brush,
        font_size_px: f32,
        line_height_px: f32,
        wrap: TextWrap,
        /// Visual placement *and* cache-key discriminator: encoder
        /// positions the glyph bbox inside `base` via both axes, and
        /// the layout pipeline threads `align.halign()` into cosmic's
        /// per-line `set_align` + [`crate::TextCacheKey`]. Same field
        /// because both want the user-intended alignment — `halign`
        /// drives X positioning and the shaped buffer's per-line
        /// offset together, `valign` drives Y positioning only.
        align: Align,
        family: FontFamily,
    },
    /// User-supplied colored triangle mesh. The framework copies
    /// `mesh.vertices` / `mesh.indices` into the active `Tree`'s mesh
    /// arena at `add_shape` time, so `mesh` only has to outlive the
    /// call. `tint` multiplies every vertex color in the shader —
    /// lets the same mesh paint in different colors without rebuilding.
    Mesh {
        mesh: &'a Mesh,
        local_rect: Option<Rect>,
        tint: Brush,
    },
    /// Gaussian-blurred rounded rectangle — drop shadow or inner
    /// shadow. Closed-form analytic shader (Evan Wallace's erf
    /// trick) batched on the existing quad pipeline; no offscreen
    /// pass, no separable blur. `local_rect = None` shadows the
    /// owner's full arranged rect; `Some(r)` paints the shadow of
    /// the owner-relative rect `r`. `offset` shifts the shadow in
    /// logical px (CSS `box-shadow` x/y). `blur` is the Gaussian
    /// σ in logical px (CSS `blur-radius / 2`, matching native
    /// renderers); `0` collapses to a sharp SDF — same code path.
    /// `spread` inflates (drop) or deflates (inset) the source
    /// rect. `inset = true` paints inside the shape boundary;
    /// `false` paints outside (the common drop-shadow case).
    /// Multi-shadow stacks just push multiple `Shape::Shadow`s in
    /// record order — composer batches them on the same draw call.
    Shadow {
        local_rect: Option<Rect>,
        radius: Corners,
        shadow: Shadow,
    },
}

/// Color source for [`Shape::Polyline`]. Length constraints
/// enforced by hard `assert!` at `add_shape` — a mismatch is a
/// caller bug.
#[derive(Clone, Copy, Debug)]
pub enum PolylineColors<'a> {
    /// One color for the whole stroke. Broadcast to every cross-section.
    Single(Color),
    /// One color per input point. `len()` must equal `points.len()`.
    /// GPU lerps between adjacent cross-sections, giving a smooth
    /// gradient along the stroke.
    PerPoint(&'a [Color]),
    /// One color per segment. `len()` must equal
    /// `points.len() - 1`. Tessellator duplicates interior
    /// cross-sections so each segment paints as a solid block —
    /// no color bleed at joins.
    PerSegment(&'a [Color]),
}

impl PolylineColors<'_> {
    /// Hard-assert the per-variant length contract against `points_len`.
    /// `Single` has none; `PerPoint` must equal; `PerSegment` must be
    /// one less. Called at the `Ui::add_shape` boundary so violations
    /// blow up at the authoring call site rather than deep in the
    /// per-frame lowering pass.
    pub fn assert_matches(&self, points_len: usize) {
        match self {
            PolylineColors::Single(_) => {}
            PolylineColors::PerPoint(cs) => assert_eq!(
                cs.len(),
                points_len,
                "Shape::Polyline PerPoint colors len {} != points len {}",
                cs.len(),
                points_len,
            ),
            PolylineColors::PerSegment(cs) => assert_eq!(
                cs.len() + 1,
                points_len,
                "Shape::Polyline PerSegment colors len {} != points len - 1 ({})",
                cs.len(),
                points_len.saturating_sub(1),
            ),
        }
    }
}

/// Endpoint cap style for stroked [`Shape::Line`] / [`Shape::Polyline`].
/// `#[repr(u8)]` with stable discriminants so cache keys don't
/// shift across reorderings; `pub` because it's user-facing.
///
/// - `Butt` — no extension. The stroke ends exactly at the
///   endpoint. Default.
/// - `Square` — extend by `width / 2` along the segment direction.
///   The end face is flat and perpendicular to the segment.
///
/// `Round` is reserved for a follow-up (requires fan-tessellation).
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LineCap {
    #[default]
    Butt = 0,
    Square = 1,
    Round = 2,
}

/// Pod wire form for [`LineCap`]. See [`ColorModeBits`].
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LineCapBits(u8);

impl LineCapBits {
    #[inline]
    pub(crate) const fn new(v: LineCap) -> Self {
        Self(v as u8)
    }
    #[inline]
    pub(crate) const fn get(self) -> LineCap {
        LineCap::from_u8(self.0)
    }
}

impl LineCap {
    /// Decode the discriminant carried by `DrawPolylinePayload`.
    /// Caller invariant: encoder only ever writes valid `as u8`
    /// values; an out-of-range byte means corrupted cmd buffer.
    pub(crate) const fn from_u8(v: u8) -> Self {
        match v {
            0 => LineCap::Butt,
            1 => LineCap::Square,
            2 => LineCap::Round,
            _ => panic!("invalid LineCap discriminant in cmd buffer"),
        }
    }
}

/// Interior-join style for [`Shape::Polyline`]. Default is `Miter`
/// — matches the SVG convention: try a sharp miter corner, fall
/// back to a bevel when the miter factor would exceed
/// `MITER_LIMIT` (4.0). `Bevel` forces a flat corner at every
/// join regardless of angle. `Round` is reserved for a follow-up.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LineJoin {
    #[default]
    Miter = 0,
    Bevel = 1,
    Round = 2,
}

/// Pod wire form for [`LineJoin`]. See [`ColorModeBits`].
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LineJoinBits(u8);

impl LineJoinBits {
    #[inline]
    pub(crate) const fn new(v: LineJoin) -> Self {
        Self(v as u8)
    }
    #[inline]
    pub(crate) const fn get(self) -> LineJoin {
        LineJoin::from_u8(self.0)
    }
}

impl LineJoin {
    pub(crate) const fn from_u8(v: u8) -> Self {
        match v {
            0 => LineJoin::Miter,
            1 => LineJoin::Bevel,
            2 => LineJoin::Round,
            _ => panic!("invalid LineJoin discriminant in cmd buffer"),
        }
    }
}

/// Storage tag for [`ShapeRecord::Polyline`]. `u8` for compactness
/// on the record; promoted to `u32` in `DrawPolylinePayload` to
/// keep that struct Pod-aligned. Discriminants are stable
/// (`Single=0`, `PerPoint=1`, `PerSegment=2`) so cache keys don't
/// shift across reorderings.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ColorMode {
    Single = 0,
    PerPoint = 1,
    PerSegment = 2,
}

/// Pod-safe wire form for [`ColorMode`] inside payload structs.
/// A `#[repr(u8)]` enum with N variants isn't `bytemuck::Pod` —
/// only N out of 256 bit patterns are valid — so payloads can't
/// store the enum directly. This `#[repr(transparent)]` wrapper
/// is bit-identical to `u8`, fully Pod, and gives compile-time
/// distinction from raw bytes so the encoder can't write a
/// `cap` byte into a `color_mode` slot.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ColorModeBits(u8);

impl ColorModeBits {
    #[inline]
    pub(crate) const fn new(v: ColorMode) -> Self {
        Self(v as u8)
    }
    #[inline]
    pub(crate) const fn get(self) -> ColorMode {
        ColorMode::from_u8(self.0)
    }
}

impl ColorMode {
    pub(crate) const fn from_u8(v: u8) -> Self {
        match v {
            0 => ColorMode::Single,
            1 => ColorMode::PerPoint,
            2 => ColorMode::PerSegment,
            _ => panic!("invalid ColorMode discriminant in cmd buffer"),
        }
    }
}

/// Wrap mode for [`ShapeRecord::Text`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextWrap {
    /// ShapeRecord once at unbounded width and never reshape. Used by every text
    /// run that fits on a single line — labels, headings, anything that
    /// shouldn't wrap.
    Single,
    /// Reshape during measure if the parent commits a width narrower than
    /// the natural unbroken line. The widest unbreakable run (longest word)
    /// is the floor — text overflows rather than breaking inside a word.
    Wrap,
}

/// True iff `local_rect` is set with a degenerate or negative extent
/// — paints no pixels regardless of fill/stroke/text. Broader than
/// `Size::approx_zero` (which is strict both-axes-near-zero); this
/// also catches `Rect::new(0, 0, -10, 20)` and similar from
/// authoring bugs. `None` means "paint into owner's full rect" and
/// is never paint-empty.
#[inline]
fn local_rect_paint_empty(local_rect: &Option<Rect>) -> bool {
    local_rect.is_some_and(|r| r.is_paint_empty())
}

impl Shape<'_> {
    /// True if this shape paints nothing visible. `Ui::add_shape`
    /// filters these out so widgets can push speculatively without
    /// guarding.
    pub fn is_noop(&self) -> bool {
        match self {
            Shape::RoundedRect {
                local_rect,
                fill,
                stroke,
                ..
            } => local_rect_paint_empty(local_rect) || (fill.is_noop() && stroke.is_noop()),
            Shape::Line { width, brush, .. } => noop_f32(*width) || brush.is_noop(),
            Shape::Polyline {
                points,
                colors,
                width,
                ..
            } => {
                if noop_f32(*width) || points.len() < 2 {
                    return true;
                }
                match colors {
                    PolylineColors::Single(c) => c.is_noop(),
                    PolylineColors::PerPoint(cs) => cs.iter().all(|c| c.is_noop()),
                    PolylineColors::PerSegment(cs) => cs.iter().all(|c| c.is_noop()),
                }
            }
            Shape::CubicBezier {
                width,
                brush,
                p0,
                p1,
                p2,
                p3,
                ..
            } => {
                noop_f32(*width)
                    || brush.is_noop()
                    || (vec2_approx_eq(*p0, *p1)
                        && vec2_approx_eq(*p0, *p2)
                        && vec2_approx_eq(*p0, *p3))
            }
            Shape::QuadraticBezier {
                width,
                brush,
                p0,
                p1,
                p2,
                ..
            } => {
                noop_f32(*width)
                    || brush.is_noop()
                    || (vec2_approx_eq(*p0, *p1) && vec2_approx_eq(*p0, *p2))
            }
            Shape::Text { text, brush, .. } => text.is_empty() || brush.is_noop(),
            Shape::Mesh {
                mesh,
                local_rect,
                tint,
            } => {
                local_rect_paint_empty(local_rect)
                    || tint.is_noop()
                    || mesh.is_empty()
                    || mesh.indices.len() < 3
                    || mesh.indices.len() % 3 != 0
            }
            Shape::Shadow {
                local_rect, shadow, ..
            } => local_rect_paint_empty(local_rect) || shadow.is_noop(),
        }
    }
}
