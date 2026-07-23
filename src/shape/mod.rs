pub(crate) mod curve;
pub(crate) mod image;
pub(crate) mod mesh;
pub(crate) mod polyline;
pub(crate) mod rect;
pub(crate) mod shadow;
pub(crate) mod stroke_bounds;
pub(crate) mod style;
pub(crate) mod triangle;

use crate::layout::types::align::Align;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::image::{ImageFilter, ImageFit};
use crate::primitives::interned_str::InternedStr;
use crate::primitives::mesh::Mesh;
use crate::primitives::rect::Rect;
use crate::primitives::shadow::Shadow;
use crate::primitives::stroke::Stroke;
use crate::renderer::image_registry::ImageHandle;
use crate::shape::curve::{CurveGeometry, CurveShape};
use crate::shape::image::ImageShape;
use crate::shape::mesh::MeshShape;
use crate::shape::polyline::{PolylineColors, PolylineShape};
use crate::shape::rect::{RectKind, RectShape};
use crate::shape::shadow::ShadowShape;
use crate::shape::style::{LineCap, LineJoin};
use crate::shape::triangle::TriangleShape;
use crate::text::wrap::TextWrap;
use crate::text::{FontFamily, FontWeight, TextMetrics};
use glam::Vec2;
use std::f32::consts::TAU;

/// User-facing paint primitive. Shape-specific constructors return concrete
/// payloads whose methods are valid for that shape; [`crate::Ui::add_shape`]
/// erases them into this enum before lowering.
#[derive(Clone, Debug)]
pub enum Shape<'a> {
    Rect(RectShape),
    /// Filled/stroked triangle with optional corner rounding, drawn as an
    /// analytic SDF on the shared quad pipeline (a sibling of `RoundedRect` —
    /// no tessellation, crisp AA at any zoom, rounded corners = `SDF - radius`).
    /// `a`/`b`/`c` are the corner points in owner-local coords; `radius`
    /// rounds all three corners uniformly (`0.0` = sharp). The solid `fill`
    /// fits the reused quad instance lanes; `stroke` sits on the inner edge
    /// like `RoundedRect`'s.
    Triangle(TriangleShape),
    /// Stroked line, Bézier, or circular arc.
    Curve(CurveShape),
    /// Stroked polyline with per-vertex or per-segment coloring. The
    /// framework copies `points` and `colors` into the active tree's
    /// record stores at `add_shape` time, so the borrows only have
    /// to outlive the call. `colors` length is constrained by `mode`
    /// (see [`PolylineColors`]); mismatches panic.
    Polyline(PolylineShape<'a>),
    /// Shaped text owned by the active node. On a leaf it contributes to the
    /// node's desired size. On a container it is paint-only and shapes against
    /// the final padded width; use a [`crate::Text`] child when text should
    /// participate in stack or grid layout.
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
        /// Direct shape authoring uses [`crate::Ui::intern`] or
        /// [`crate::Ui::fmt`] to place the bytes in the text arena.
        /// Widget constructors accept borrowed and owned text directly
        /// because they defer this step until `show`. `Shape<'a>`'s
        /// `'a` parameter doesn't constrain this variant; it's used by
        /// `Polyline.points` / `Mesh.mesh` instead.
        text: InternedStr,
        color: Color,
        font_size_px: f32,
        line_height_px: f32,
        wrap: TextWrap,
        /// Visual placement *and* cache-key discriminator: encoder
        /// positions the glyph bbox inside `base` via both axes (only
        /// when `local_rect = None`), and the layout pipeline always
        /// threads `align.halign()` into cosmic's per-line
        /// `set_align` and text cache key. Same field because
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
    Mesh(MeshShape<'a>),
    /// Textured rectangle painted from a registered [`ImageHandle`].
    /// `local_rect = None` paints into the owner's full arranged rect;
    /// `Some(r)` paints `r` at owner-relative coords (`r.min = (0, 0)`
    /// is the owner's top-left). `fit` (default `Fill`) controls how
    /// the image's intrinsic size maps onto that rect — see
    /// [`ImageFit`]. `min_filter` and `mag_filter` independently pick
    /// the sampling used when the image is painted smaller or larger
    /// than its intrinsic size — bilinear (default) or hard-edged
    /// nearest, see [`ImageFilter`].
    /// `tint` multiplies the sampled pixel in linear-RGB
    /// premultiplied space; `Color::WHITE` is "no tint." `handle` is the
    /// RAII [`ImageHandle`] from [`crate::Ui::register_image`]; hold it to
    /// keep the GPU texture resident (the bytes upload once, then free)
    /// and `clone` it in here each frame.
    Image(ImageShape),
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
    Shadow(ShadowShape),
}

impl<'a> Shape<'a> {
    /// A rounded rectangle painting `rect` (owner-relative). Starts
    /// transparent-filled, strokeless, sharp-cornered — chain
    /// [`RectShape::fill`] / [`RectShape::stroke`] / [`RectShape::corners`].
    pub fn rect(rect: Rect) -> RectShape {
        RectShape::new(RectKind::Rounded, Some(rect))
    }

    /// A rounded rectangle painting the owner's full arranged rect.
    pub fn owner_rect() -> RectShape {
        RectShape::new(RectKind::Rounded, None)
    }

    /// An inverse-mask rectangle over `rect` — the sibling of
    /// [`Self::rect`], same chainable fill/stroke/corners.
    pub fn windowed_rect(rect: Rect) -> RectShape {
        RectShape::new(RectKind::Windowed, Some(rect))
    }

    /// A windowed rectangle painting the owner's full arranged rect.
    pub fn owner_windowed_rect() -> RectShape {
        RectShape::new(RectKind::Windowed, None)
    }

    /// A triangle with corners `a`/`b`/`c` (owner-local). Starts sharp
    /// (radius 0), transparent-filled, strokeless.
    pub fn triangle(a: Vec2, b: Vec2, c: Vec2) -> TriangleShape {
        TriangleShape {
            a,
            b,
            c,
            radius: 0.0,
            fill: Color::TRANSPARENT,
            stroke: Stroke::ZERO,
        }
    }

    /// A `width`-thick straight line from `a` to `b` (`Butt` cap).
    /// Starts transparent.
    pub fn line(a: Vec2, b: Vec2, width: f32) -> CurveShape {
        CurveShape::new(CurveGeometry::Line { a, b }, width)
    }

    /// A stroked polyline through `points`, coloured by `colors` (`Butt`
    /// cap, `Miter` join).
    pub fn polyline(
        points: &'a [Vec2],
        colors: PolylineColors<'a>,
        width: f32,
    ) -> PolylineShape<'a> {
        PolylineShape {
            points,
            colors,
            width,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
        }
    }

    /// A stroked cubic Bézier through control points `p0..=p3` (`Butt`
    /// cap). Starts transparent.
    pub fn cubic_bezier(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, width: f32) -> CurveShape {
        CurveShape::new(CurveGeometry::CubicBezier { p0, p1, p2, p3 }, width)
    }

    /// A stroked quadratic Bézier through `p0`/`p1`/`p2`. See
    /// [`Self::cubic_bezier`].
    pub fn quadratic_bezier(p0: Vec2, p1: Vec2, p2: Vec2, width: f32) -> CurveShape {
        CurveShape::new(CurveGeometry::QuadraticBezier { p0, p1, p2 }, width)
    }

    /// A stroked circular arc sweeping `sweep` radians from
    /// `start_angle` (`Butt` cap). Starts transparent — chain
    /// [`CurveShape::brush`] / [`CurveShape::cap`].
    pub fn arc(center: Vec2, radius: f32, start_angle: f32, sweep: f32, width: f32) -> CurveShape {
        CurveShape::new(
            CurveGeometry::Arc {
                center,
                radius,
                start_angle,
                sweep,
            },
            width,
        )
    }

    /// A stroked full circle — [`Self::arc`] with a `2π` sweep, which
    /// closes seamlessly under the default `Butt` cap.
    pub fn circle(center: Vec2, radius: f32, width: f32) -> CurveShape {
        Self::arc(center, radius, 0.0, TAU, width)
    }

    /// A `shadow` of the owner's full rect.
    pub fn shadow(shadow: Shadow) -> ShadowShape {
        ShadowShape {
            local_rect: None,
            corners: Corners::ZERO,
            shadow,
        }
    }

    /// A textured rect from `handle` painting the owner's full rect at the
    /// default fit/filters, untinted.
    pub fn image(handle: ImageHandle) -> ImageShape {
        ImageShape {
            handle,
            local_rect: None,
            fit: ImageFit::default(),
            min_filter: ImageFilter::default(),
            mag_filter: ImageFilter::default(),
            tint: Color::WHITE,
        }
    }

    /// A colored triangle `mesh` painting the owner's full rect, untinted.
    pub fn mesh(mesh: &'a Mesh) -> MeshShape<'a> {
        MeshShape {
            mesh,
            local_rect: None,
            tint: Color::WHITE,
        }
    }
}

impl<'a> From<RectShape> for Shape<'a> {
    fn from(shape: RectShape) -> Self {
        Self::Rect(shape)
    }
}

impl<'a> From<TriangleShape> for Shape<'a> {
    fn from(shape: TriangleShape) -> Self {
        Self::Triangle(shape)
    }
}

impl<'a> From<CurveShape> for Shape<'a> {
    fn from(shape: CurveShape) -> Self {
        Self::Curve(shape)
    }
}

impl<'a> From<PolylineShape<'a>> for Shape<'a> {
    fn from(shape: PolylineShape<'a>) -> Self {
        Self::Polyline(shape)
    }
}

impl<'a> From<MeshShape<'a>> for Shape<'a> {
    fn from(shape: MeshShape<'a>) -> Self {
        Self::Mesh(shape)
    }
}

impl<'a> From<ImageShape> for Shape<'a> {
    fn from(shape: ImageShape) -> Self {
        Self::Image(shape)
    }
}

impl<'a> From<ShadowShape> for Shape<'a> {
    fn from(shape: ShadowShape) -> Self {
        Self::Shadow(shape)
    }
}

#[inline]
fn local_rect_paint_empty(local_rect: &Option<Rect>) -> bool {
    local_rect.is_some_and(|rect| rect.is_paint_empty())
}

impl Shape<'_> {
    /// True if this shape paints nothing visible. `Ui::add_shape`
    /// filters these out so widgets can push speculatively without
    /// guarding.
    pub(crate) fn is_noop(&self) -> bool {
        match self {
            Shape::Rect(shape) => shape.is_noop(),
            Shape::Triangle(shape) => shape.is_noop(),
            Shape::Curve(shape) => shape.is_noop(),
            Shape::Polyline(shape) => shape.is_noop(),
            Shape::Text {
                text,
                color,
                font_size_px,
                line_height_px,
                ..
            } => {
                text.is_empty()
                    || color.is_noop()
                    || TextMetrics::new(*font_size_px, *line_height_px).is_err()
            }
            Shape::Mesh(shape) => shape.is_noop(),
            Shape::Image(shape) => shape.is_noop(),
            Shape::Shadow(shape) => shape.is_noop(),
        }
    }
}

#[cfg(test)]
mod tests;
