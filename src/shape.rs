use crate::layout::types::align::Align;
use crate::primitives::image::{ImageFilter, ImageFit};
use crate::primitives::mesh::Mesh;
use crate::primitives::{
    approx::{noop_f32, vec2_approx_eq},
    brush::Brush,
    color::Color,
    corners::Corners,
    interned_str::InternedStr,
    rect::Rect,
    shadow::Shadow,
    stroke::Stroke,
};
use crate::renderer::image_registry::ImageHandle;
use crate::text::{FontFamily, FontWeight};
use glam::Vec2;

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
        corners: Corners,
        fill: Brush,
        stroke: Stroke,
    },
    /// Inverse of [`Shape::RoundedRect`]: a rect with a rounded window
    /// punched through it. The `fill` paints *outside* the rounded
    /// boundary — the corner wedges, out to the rect edge — the
    /// `stroke` keeps the same inner-edge annulus a `RoundedRect`
    /// stroke occupies, and the window interior is transparent.
    ///
    /// Cheap stand-in for rounded-corner clipping: draw content as a
    /// plain unclipped rect, then paint this on top with the
    /// surrounding background as `fill` — the corners mask to the
    /// background and the stroke draws the border over the content
    /// edge, visually identical to a stroked `RoundedRect` under
    /// `ClipMode::Rounded` but without the stencil-mask pass. The rect
    /// edge itself is a hard cut (no outward AA); the fill is meant to
    /// blend into a matching backdrop.
    WindowedRect {
        local_rect: Option<Rect>,
        corners: Corners,
        fill: Brush,
        stroke: Stroke,
    },
    /// Filled/stroked triangle with optional corner rounding, drawn as an
    /// analytic SDF on the shared quad pipeline (a sibling of `RoundedRect` —
    /// no tessellation, crisp AA at any zoom, rounded corners = `SDF - radius`).
    /// `a`/`b`/`c` are the corner points in owner-local coords; `radius`
    /// rounds all three corners uniformly (`0.0` = sharp). `fill` must be
    /// `Brush::Solid` — gradients aren't packable into the reused quad
    /// instance lanes (a dedicated-pipeline follow-up if ever needed);
    /// `stroke` sits on the inner edge like `RoundedRect`'s.
    Triangle {
        a: Vec2,
        b: Vec2,
        c: Vec2,
        radius: f32,
        fill: Brush,
        stroke: Stroke,
    },
    /// Two-point stroked line. Rendered natively on the GPU on the
    /// same parametric stroke pipeline as the Béziers — lowered to a
    /// degenerate `ShapeRecord::Curve` (control points on the
    /// segment's thirds, so `t` runs linearly a → b); the composer's
    /// flatness fast-path keeps it a single instance. `cap` applies
    /// to both endpoints; a linear-gradient brush is sampled along
    /// the segment (the gradient's own `angle` is ignored).
    Line {
        a: Vec2,
        b: Vec2,
        width: f32,
        brush: Brush,
        cap: LineCap,
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
    /// Cubic Bezier curve, stroked. Rendered natively on the GPU —
    /// lowered to `ShapeRecord::Curve` at authoring time, batched per
    /// scissor group, expanded to a thickened triangle strip in the
    /// vertex shader. The composer derives an adaptive sub-instance
    /// count from the post-transform control-polygon length. `brush`
    /// takes `Solid` or `Linear` (sampled along the curve parameter
    /// `t`); no `join` (single-curve primitive — no interior joins).
    /// `cap` ships `Butt`, `Square`, and `Round`.
    CubicBezier {
        p0: Vec2,
        p1: Vec2,
        p2: Vec2,
        p3: Vec2,
        width: f32,
        brush: Brush,
        cap: LineCap,
    },
    /// Quadratic Bezier curve, stroked. See [`Shape::CubicBezier`].
    QuadraticBezier {
        p0: Vec2,
        p1: Vec2,
        p2: Vec2,
        width: f32,
        brush: Brush,
        cap: LineCap,
    },
    /// Circular arc, stroked. Rendered natively on the GPU on the
    /// same parametric stroke pipeline as the Béziers — the vertex
    /// shader evaluates the exact circle (no flattening, no cubic
    /// approximation), so it stays smooth at any size, zoom, and DPI.
    ///
    /// `start_angle` is in radians, screen convention: `0` points
    /// along +x and, with y down, increasing angles run clockwise.
    /// The arc sweeps `sweep` radians from there; the sign picks the
    /// direction. `|sweep|` is capped at `2π` (hard assert at
    /// lowering — anything longer would repaint pixels and
    /// double-blend a translucent stroke); exactly `±2π` with `Butt`
    /// caps closes seamlessly into a full circle.
    ///
    /// A linear-gradient brush is sampled along the sweep (`t = 0`
    /// at `start_angle`, `t = 1` at the far end); the gradient's own
    /// `angle` is ignored, same as on the Bézier shapes.
    Arc {
        center: Vec2,
        radius: f32,
        start_angle: f32,
        sweep: f32,
        width: f32,
        brush: Brush,
        cap: LineCap,
    },
    Text {
        /// `None` → encoder owns positioning: the glyph bbox is
        /// placed inside the owner's padded inner rect via `align`.
        /// Used by Text/Button/ContextMenu.
        /// `Some(origin)` → widget owns positioning: bbox origin is
        /// `owner.min + origin`, encoder is a passthrough (`align`'s
        /// placement axes are ignored). Used by TextEdit so it can
        /// shift the text by scroll + alignment offsets the encoder
        /// can't compute.
        local_origin: Option<Vec2>,
        /// Moved into `ShapeRecord` at lowering and lives there until
        /// the next frame's `Shapes::clear`. Static literals
        /// (`Button::new().label("foo")`) wrap zero-cost via
        /// `SmolStr::new_static`; short owned strings inline on the
        /// stack; long ones sit behind `Arc<str>`. Transient `&str`
        /// callers materialize via [`crate::Ui::intern`] / [`crate::Ui::fmt`]
        /// to land in the `Interned` arena. `Shape<'a>`'s `'a`
        /// parameter doesn't constrain this variant; it's used by
        /// `Polyline.points` / `Mesh.mesh` instead.
        text: InternedStr,
        brush: Brush,
        font_size_px: f32,
        line_height_px: f32,
        wrap: TextWrap,
        /// Visual placement *and* cache-key discriminator: encoder
        /// positions the glyph bbox inside `base` via both axes (only
        /// when `local_rect = None`), and the layout pipeline always
        /// threads `align.halign()` into cosmic's per-line
        /// `set_align` + [`crate::TextCacheKey`]. Same field because
        /// both consumers want the user-intended alignment.
        align: Align,
        family: FontFamily,
        weight: FontWeight,
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
    /// Textured rectangle painted from a registered [`ImageHandle`].
    /// `local_rect = None` paints into the owner's full arranged rect;
    /// `Some(r)` paints `r` at owner-relative coords (`r.min = (0, 0)`
    /// is the owner's top-left). `fit` (default `Fill`) controls how
    /// the image's intrinsic size maps onto that rect — see
    /// [`ImageFit`]. `filter` picks the sampling between texels —
    /// bilinear (default) or hard-edged nearest, see [`ImageFilter`].
    /// `tint` multiplies the sampled pixel in linear-RGB
    /// premultiplied space; `Color::WHITE` is "no tint." `handle` is the
    /// RAII [`ImageHandle`] from [`crate::Ui::register_image`]; hold it to
    /// keep the GPU texture resident (the bytes upload once, then free)
    /// and `clone` it in here each frame.
    Image {
        handle: ImageHandle,
        local_rect: Option<Rect>,
        fit: ImageFit,
        filter: ImageFilter,
        tint: Color,
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
        corners: Corners,
        shadow: Shadow,
    },
}

impl<'a> Shape<'a> {
    /// A rounded rectangle painting `rect` (owner-relative). Starts
    /// transparent-filled, strokeless, sharp-cornered — chain
    /// [`Self::fill`] / [`Self::stroke`] / [`Self::corners`].
    pub fn rect(rect: Rect) -> Self {
        Shape::RoundedRect {
            local_rect: Some(rect),
            corners: Corners::ZERO,
            fill: Brush::TRANSPARENT,
            stroke: Stroke::ZERO,
        }
    }

    /// A [`Shape::WindowedRect`] over `rect` — the inverse-mask sibling of
    /// [`Self::rect`], same chainable fill/stroke/corners.
    pub fn windowed_rect(rect: Rect) -> Self {
        Shape::WindowedRect {
            local_rect: Some(rect),
            corners: Corners::ZERO,
            fill: Brush::TRANSPARENT,
            stroke: Stroke::ZERO,
        }
    }

    /// A triangle with corners `a`/`b`/`c` (owner-local). Starts sharp
    /// (radius 0), transparent-filled, strokeless — chain [`Self::fill`] /
    /// [`Self::stroke`] / [`Self::radius`].
    pub fn triangle(a: Vec2, b: Vec2, c: Vec2) -> Self {
        Shape::Triangle {
            a,
            b,
            c,
            radius: 0.0,
            fill: Brush::TRANSPARENT,
            stroke: Stroke::ZERO,
        }
    }

    /// A `width`-thick straight line from `a` to `b` (`Butt` cap).
    /// Starts transparent — chain [`Self::brush`] / [`Self::cap`].
    pub fn line(a: Vec2, b: Vec2, width: f32) -> Self {
        Shape::Line {
            a,
            b,
            width,
            brush: Brush::TRANSPARENT,
            cap: LineCap::Butt,
        }
    }

    /// A stroked polyline through `points`, coloured by `colors` (`Butt`
    /// cap, `Miter` join). Chain [`Self::cap`] / [`Self::join`].
    pub fn polyline(points: &'a [Vec2], colors: PolylineColors<'a>, width: f32) -> Self {
        Shape::Polyline {
            points,
            colors,
            width,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
        }
    }

    /// A stroked cubic Bézier through control points `p0..=p3` (`Butt`
    /// cap). Starts transparent — chain [`Self::brush`] / [`Self::cap`].
    pub fn cubic_bezier(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, width: f32) -> Self {
        Shape::CubicBezier {
            p0,
            p1,
            p2,
            p3,
            width,
            brush: Brush::TRANSPARENT,
            cap: LineCap::Butt,
        }
    }

    /// A stroked quadratic Bézier through `p0`/`p1`/`p2`. See
    /// [`Self::cubic_bezier`].
    pub fn quadratic_bezier(p0: Vec2, p1: Vec2, p2: Vec2, width: f32) -> Self {
        Shape::QuadraticBezier {
            p0,
            p1,
            p2,
            width,
            brush: Brush::TRANSPARENT,
            cap: LineCap::Butt,
        }
    }

    /// A stroked circular arc sweeping `sweep` radians from
    /// `start_angle` (`Butt` cap). Starts transparent — chain
    /// [`Self::brush`] / [`Self::cap`]. See [`Shape::Arc`] for the
    /// angle convention.
    pub fn arc(center: Vec2, radius: f32, start_angle: f32, sweep: f32, width: f32) -> Self {
        Shape::Arc {
            center,
            radius,
            start_angle,
            sweep,
            width,
            brush: Brush::TRANSPARENT,
            cap: LineCap::Butt,
        }
    }

    /// A stroked full circle — [`Self::arc`] with a `2π` sweep, which
    /// closes seamlessly under the default `Butt` cap.
    pub fn circle(center: Vec2, radius: f32, width: f32) -> Self {
        Shape::arc(center, radius, 0.0, std::f32::consts::TAU, width)
    }

    /// A `shadow` of the owner's full rect. Chain [`Self::at`] to shadow a
    /// specific owner-relative rect and [`Self::corners`] to round it.
    pub fn shadow(shadow: Shadow) -> Self {
        Shape::Shadow {
            local_rect: None,
            corners: Corners::ZERO,
            shadow,
        }
    }

    /// A textured rect from `handle` painting the owner's full rect at the
    /// default fit/filter, untinted. Chain [`Self::at`] / [`Self::fit`] /
    /// [`Self::filter`] / [`Self::tint`].
    pub fn image(handle: ImageHandle) -> Self {
        Shape::Image {
            handle,
            local_rect: None,
            fit: ImageFit::default(),
            filter: ImageFilter::default(),
            tint: Color::WHITE,
        }
    }

    /// A colored triangle `mesh` painting the owner's full rect, untinted.
    /// Chain [`Self::at`] to place it in an owner-relative rect.
    pub fn mesh(mesh: &'a Mesh) -> Self {
        Shape::Mesh {
            mesh,
            local_rect: None,
            tint: Color::WHITE.into(),
        }
    }

    /// Set the fill brush — [`Shape::RoundedRect`] / [`Shape::WindowedRect`]
    /// / [`Shape::Triangle`].
    pub fn fill(mut self, brush: impl Into<Brush>) -> Self {
        match &mut self {
            Shape::RoundedRect { fill, .. }
            | Shape::WindowedRect { fill, .. }
            | Shape::Triangle { fill, .. } => *fill = brush.into(),
            _ => panic!("Shape::fill() applies only to RoundedRect / WindowedRect / Triangle"),
        }
        self
    }

    /// Set the border stroke — RoundedRect / WindowedRect / Triangle.
    pub fn stroke(mut self, stroke: Stroke) -> Self {
        match &mut self {
            Shape::RoundedRect { stroke: s, .. }
            | Shape::WindowedRect { stroke: s, .. }
            | Shape::Triangle { stroke: s, .. } => *s = stroke,
            _ => panic!("Shape::stroke() applies only to RoundedRect / WindowedRect / Triangle"),
        }
        self
    }

    /// Set the corner radii — RoundedRect / WindowedRect / Shadow.
    pub fn corners(mut self, corners: Corners) -> Self {
        match &mut self {
            Shape::RoundedRect { corners: c, .. }
            | Shape::WindowedRect { corners: c, .. }
            | Shape::Shadow { corners: c, .. } => *c = corners,
            _ => panic!("Shape::corners() applies only to RoundedRect / WindowedRect / Shadow"),
        }
        self
    }

    /// Set a [`Shape::Triangle`]'s uniform corner radius.
    pub fn radius(mut self, radius: f32) -> Self {
        match &mut self {
            Shape::Triangle { radius: r, .. } => *r = radius,
            _ => panic!("Shape::radius() applies only to Triangle"),
        }
        self
    }

    /// Paint into the owner-relative `rect` instead of the owner's full
    /// rect — RoundedRect / WindowedRect / Mesh / Image / Shadow.
    pub fn at(mut self, rect: Rect) -> Self {
        match &mut self {
            Shape::RoundedRect { local_rect, .. }
            | Shape::WindowedRect { local_rect, .. }
            | Shape::Mesh { local_rect, .. }
            | Shape::Image { local_rect, .. }
            | Shape::Shadow { local_rect, .. } => *local_rect = Some(rect),
            _ => panic!("Shape::at() applies only to rect / mesh / image / shadow shapes"),
        }
        self
    }

    /// Set the stroke brush — [`Shape::Line`] / [`Shape::CubicBezier`] /
    /// [`Shape::QuadraticBezier`] / [`Shape::Arc`].
    pub fn brush(mut self, brush: impl Into<Brush>) -> Self {
        match &mut self {
            Shape::Line { brush: b, .. }
            | Shape::CubicBezier { brush: b, .. }
            | Shape::QuadraticBezier { brush: b, .. }
            | Shape::Arc { brush: b, .. } => *b = brush.into(),
            _ => panic!("Shape::brush() applies only to Line / Bézier / Arc shapes"),
        }
        self
    }

    /// Set the endpoint cap — Line / Polyline / CubicBezier /
    /// QuadraticBezier / Arc.
    pub fn cap(mut self, cap: LineCap) -> Self {
        match &mut self {
            Shape::Line { cap: c, .. }
            | Shape::Polyline { cap: c, .. }
            | Shape::CubicBezier { cap: c, .. }
            | Shape::QuadraticBezier { cap: c, .. }
            | Shape::Arc { cap: c, .. } => *c = cap,
            _ => panic!("Shape::cap() applies only to Line / Polyline / Bézier / Arc shapes"),
        }
        self
    }

    /// Set the interior join — [`Shape::Polyline`] (the only shape
    /// with interior joins; single-stroke shapes have none).
    pub fn join(mut self, join: LineJoin) -> Self {
        match &mut self {
            Shape::Polyline { join: j, .. } => *j = join,
            _ => panic!("Shape::join() applies only to Polyline"),
        }
        self
    }

    /// Set a [`Shape::Image`]'s fit mode.
    pub fn fit(mut self, fit: ImageFit) -> Self {
        match &mut self {
            Shape::Image { fit: f, .. } => *f = fit,
            _ => panic!("Shape::fit() applies only to Image"),
        }
        self
    }

    /// Set a [`Shape::Image`]'s sampling filter.
    pub fn filter(mut self, filter: ImageFilter) -> Self {
        match &mut self {
            Shape::Image { filter: f, .. } => *f = filter,
            _ => panic!("Shape::filter() applies only to Image"),
        }
        self
    }

    /// Set a [`Shape::Image`]'s multiply tint (`Color::WHITE` = untinted).
    pub fn tint(mut self, tint: Color) -> Self {
        match &mut self {
            Shape::Image { tint: t, .. } => *t = tint,
            _ => panic!("Shape::tint() applies only to Image"),
        }
        self
    }
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
    /// `points.len() - 1`. Each segment renders as its own solid
    /// block (join chrome blends the two neighbors) — no color
    /// bleed at joins.
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

/// Endpoint cap style for stroked shapes (Line / Polyline / béziers /
/// Arc). `#[repr(u8)]` with stable discriminants so cache keys don't
/// shift across reorderings; `pub` because it's user-facing.
///
/// - `Butt` — no extension. The stroke ends exactly at the
///   endpoint. Default.
/// - `Square` — extend by `width / 2` along the local tangent.
///   The end face is flat and perpendicular to the stroke.
/// - `Round` — a `width / 2` half-disc past the endpoint.
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
/// join regardless of angle; `Round` fills the corner with an arc
/// fan.
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextWrap {
    /// **Default.** Single line shaped once at unbounded width and never
    /// reshaped, so it overflows a too-narrow slot rather than truncating.
    /// Min-content equals the full line width, so a Hug track won't shrink
    /// below it — keeps labels at their natural width, *meant* to run past
    /// the viewport. For a field that clips + scrolls its own overflow use
    /// [`TextWrap::Scroll`], whose min-content is zero.
    #[default]
    SingleLine,
    /// Single line shaped once at unbounded width and never reshaped, exactly
    /// like [`TextWrap::SingleLine`], but with **zero** min-content: the owner
    /// is expected to clip the overflow (`ClipMode::Rect`) and scroll it into
    /// view, so a Hug/Fill box may shrink below the text rather than reserving
    /// its full width. For editable single-line fields and any widget that
    /// manages its own horizontal scroll. Unlike [`TextWrap::Truncate`] the run
    /// is never cut at shape time, so the owner can scroll to any offset over
    /// the full buffer.
    Scroll,
    /// Single line, hard-truncated to the committed width with
    /// no trailing marker — glyphs past the box edge are simply dropped.
    /// Min-content is zero (the run can shrink to nothing), so a bounded
    /// parent clips it to its slot instead of overflowing — single-line text
    /// is limited to its available width like any other widget. In an
    /// unbounded / Hug-width parent the full line shows; truncation only
    /// bites once a parent commits a narrower width.
    Truncate,
    /// Single line, truncated to the committed width with a trailing `…`.
    /// Identical to [`TextWrap::Truncate`] (min-content zero, clipped to the
    /// slot) except the cut is marked with an ellipsis. For labels where the
    /// elision should be visible — paths, names in fixed-width chrome.
    Ellipsis,
    /// Reshape during measure if the parent commits a width narrower than
    /// the natural unbroken line. Breaks on word boundaries first; when a
    /// single word still doesn't fit it falls back to a character-level
    /// break. Min-content is effectively zero — the run can always reflow
    /// to fit the committed slot, like WPF's `TextWrapping="Wrap"`.
    Wrap,
    /// Reshape during measure on word boundaries only. The widest unbreakable
    /// run (longest word) is the floor — words that exceed the committed
    /// width overflow rather than breaking mid-word. Matches WPF's
    /// `TextWrapping="WrapWithOverflow"`.
    WrapWithOverflow,
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
            }
            | Shape::WindowedRect {
                local_rect,
                fill,
                stroke,
                ..
            } => local_rect_paint_empty(local_rect) || (fill.is_noop() && stroke.is_noop()),
            Shape::Triangle {
                a,
                b,
                c,
                fill,
                stroke,
                ..
            } => {
                (fill.is_noop() && stroke.is_noop())
                    // Fully-degenerate triangle (all three corners coincident)
                    // paints nothing even rounded — a point has zero area.
                    || (vec2_approx_eq(*a, *b) && vec2_approx_eq(*a, *c))
            }
            Shape::Line {
                a, b, width, brush, ..
            } => noop_f32(*width) || brush.is_noop() || vec2_approx_eq(*a, *b),
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
                cap: _,
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
                cap: _,
            } => {
                noop_f32(*width)
                    || brush.is_noop()
                    || (vec2_approx_eq(*p0, *p1) && vec2_approx_eq(*p0, *p2))
            }
            Shape::Arc {
                radius,
                sweep,
                width,
                brush,
                ..
            } => noop_f32(*width) || brush.is_noop() || noop_f32(*radius) || noop_f32(sweep.abs()),
            Shape::Text { text, brush, .. } => text.is_empty() || brush.is_noop(),
            Shape::Mesh {
                mesh,
                local_rect,
                tint,
            } => local_rect_paint_empty(local_rect) || tint.is_noop() || mesh.is_noop(),
            Shape::Image {
                local_rect, tint, ..
            } => local_rect_paint_empty(local_rect) || tint.is_noop(),
            Shape::Shadow {
                local_rect, shadow, ..
            } => local_rect_paint_empty(local_rect) || shadow.is_noop(),
        }
    }
}
