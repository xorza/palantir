//! Shapes from authoring to record: geometry builders and the paints a shape takes, the per-tree
//! record buffer, the payload store they lower into, their paint animations, and lowering.

#![expect(
    private_interfaces,
    reason = "every `impl sealed::DrawWith` names the crate-private `Draw` in a reachable signature, which is the seal working as designed; it fires per impl site"
)]

pub(crate) mod arc;
pub(crate) mod circle;
pub(crate) mod curve;
pub(crate) mod draw;
pub(crate) mod effect_paint;
pub(crate) mod geometry;
pub(crate) mod icon_paint;
pub(crate) mod image_paint;
pub(crate) mod lower;
pub(crate) mod mesh;
pub(crate) mod paint;
pub(crate) mod paint_animation;
pub(crate) mod polyline;
pub(crate) mod record;
pub(crate) mod record_store;
pub(crate) mod rect;
pub(crate) mod shapes;
pub(crate) mod stroke_bounds;
pub(crate) mod text;
pub(crate) mod triangle;

use crate::primitives::geometry::cubic_bezier::CubicBezier;
use crate::primitives::geometry::mesh::Mesh;
use crate::primitives::geometry::quadratic_bezier::QuadraticBezier;
use crate::primitives::math::domain;
use crate::primitives::math::domain::vec2;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::brush::gradient::{Gradient, GradientGeometry};
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::paint::Paint;
use crate::primitives::text::interned_str::InternedStr;
use crate::shape::arc::ArcShape;
use crate::shape::circle::CircleShape;
use crate::shape::curve::{CurvePath, CurveShape};
use crate::shape::mesh::MeshShape;
use crate::shape::polyline::PolylineShape;
use crate::shape::rect::RectShape;
use crate::shape::text::TextShape;
use crate::shape::triangle::TriangleShape;
use crate::text::glyph_font::GlyphFont;
use glam::Vec2;

/// A shape that `P` covers, as [`Ui::add_shape`](crate::Ui::add_shape) joins them: each pair a
/// shape's pipeline draws, listed as this trait's implementors. Sealed.
///
/// - A rect takes an [`IntoPaint`], a [`Paint`] or a fill alone; an
///   [`EffectPaint`](crate::widget::EffectPaint); an [`ImagePaint`](crate::widget::ImagePaint);
///   an [`IconPaint`](crate::widget::IconPaint). An image's rect clips it by its corners; an icon
///   takes a square-cornered rect, as its pipeline clips to no corners.
/// - A circle takes an `IntoPaint` as a round box, a [`Stroke`](crate::Stroke) as a closed arc,
///   an `ImagePaint` fit to its square, and an `EffectPaint`, which reads its square and its
///   corners.
/// - A triangle takes an `IntoPaint` of a fill, a border and a material, an `ImagePaint`, and an
///   `EffectPaint`, which reads the triangle's covering rect and no corners.
/// - A line, a Bézier, an arc and a polyline take a `Stroke`.
/// - Text takes its color, a mesh its tint, each an [`RgbaF32`].
pub trait PaintedBy<P>: sealed::DrawWith<P> {}

/// What converts into a box's [`Paint`]: the paint itself, or a color, a
/// [`Brush`] or a gradient as a fill alone. Sealed.
pub trait IntoPaint: sealed::ToPaint {}

impl IntoPaint for Paint {}
impl IntoPaint for RgbaF32 {}
impl IntoPaint for Brush {}
impl<G: GradientGeometry + Clone> IntoPaint for Gradient<G> where Paint: From<Self> {}

pub(crate) mod sealed {
    use crate::primitives::paint::brush::Brush;
    use crate::primitives::paint::brush::gradient::{Gradient, GradientGeometry};
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::paint::Paint;
    use crate::shape::draw::Draw;
    use std::borrow::Cow;

    #[expect(
        private_interfaces,
        unnameable_types,
        reason = "nothing outside the crate can name the sealed trait, so the crate-private types in its signature cannot leak"
    )]
    pub trait DrawWith<P> {
        /// Panic unless `paint` holds its kinds and is one this shape's pipeline draws, at the
        /// caller's `add_shape`; the shape checked its own values where it was built.
        #[track_caller]
        fn check(&self, paint: &P);

        /// This shape covered by `paint`, as the one [`Draw`] the record store records.
        fn draw<'a>(&'a self, paint: &'a P) -> Draw<'a>;
    }

    #[expect(
        unnameable_types,
        reason = "the seal: `IntoPaint` names it as its supertrait"
    )]
    pub trait ToPaint {
        /// This value as a box's paint, borrowed where it is one.
        fn to_paint(&self) -> Cow<'_, Paint>;
    }

    impl ToPaint for Paint {
        fn to_paint(&self) -> Cow<'_, Paint> {
            Cow::Borrowed(self)
        }
    }

    impl ToPaint for RgbaF32 {
        fn to_paint(&self) -> Cow<'_, Paint> {
            Cow::Owned(Paint::from(*self))
        }
    }

    impl ToPaint for Brush {
        fn to_paint(&self) -> Cow<'_, Paint> {
            Cow::Owned(Paint::from(self.clone()))
        }
    }

    impl<G: GradientGeometry + Clone> ToPaint for Gradient<G>
    where
        Paint: From<Self>,
    {
        fn to_paint(&self) -> Cow<'_, Paint> {
            Cow::Owned(Paint::from(self.clone()))
        }
    }
}

/// Constructor namespace for the shapes: geometry alone, laid out in the owner's arranged rect or
/// a rect of its own, which [`Ui::add_shape`](crate::Ui::add_shape) joins with a paint.
#[derive(Clone, Copy, Debug)]
pub struct Shape;

impl Shape {
    /// The owner's arranged rect, or [`RectShape::at`] a rect of its own; square-cornered until
    /// [`RectShape::corners`].
    pub const fn rect() -> RectShape {
        RectShape::new()
    }

    /// The largest circle centered in the owner's arranged rect, or [`CircleShape::at`] a rect of
    /// its own, as SwiftUI's `Circle` fits the rect it is offered.
    pub const fn circle() -> CircleShape {
        CircleShape::new()
    }

    /// An arc of the circle [`Self::circle`] makes, from `start_angle` turning by `sweep`, both in
    /// radians clockwise from the right, as Compose's `drawArc` runs on its rect's oval. A sweep
    /// past a full circle, `±2π`, draws the full circle.
    ///
    /// # Panics
    ///
    /// Panics unless both are [angles](crate::widget::domain::angle).
    #[track_caller]
    pub const fn arc(start_angle: f32, sweep: f32) -> ArcShape {
        ArcShape::new(domain::angle(start_angle), domain::angle(sweep))
    }

    /// A triangle with corners `a`, `b` and `c`, logical px from its rect's top-left. Starts sharp.
    ///
    /// # Panics
    ///
    /// Panics unless every corner is an [offset](crate::widget::domain::offset).
    #[track_caller]
    pub const fn triangle(a: Vec2, b: Vec2, c: Vec2) -> TriangleShape {
        TriangleShape::new(vec2::offset(a), vec2::offset(b), vec2::offset(c))
    }

    /// A straight line from `a` to `b`, logical px from its rect's top-left.
    ///
    /// # Panics
    ///
    /// Panics unless every point is an [offset](crate::widget::domain::offset).
    #[track_caller]
    pub const fn line(a: Vec2, b: Vec2) -> CurveShape {
        let (a, b) = (vec2::offset(a), vec2::offset(b));
        CurveShape::new(CurvePath::Line { a, b })
    }

    /// A polyline through `points`, logical px from its rect's top-left.
    ///
    /// # Panics
    ///
    /// As [`Self::line`].
    #[track_caller]
    pub fn polyline(points: &[Vec2]) -> PolylineShape<'_> {
        let shape = PolylineShape::new(points);
        // One check of the folded box covers every point.
        shape.bbox.validate();
        shape
    }

    /// A cubic Bézier `bezier`, logical px from its rect's top-left.
    ///
    /// # Panics
    ///
    /// As [`Self::line`].
    #[track_caller]
    pub const fn cubic_bezier(bezier: CubicBezier) -> CurveShape {
        bezier.validate();
        CurveShape::new(CurvePath::CubicBezier(bezier))
    }

    /// A quadratic Bézier `bezier`, logical px from its rect's top-left.
    ///
    /// # Panics
    ///
    /// As [`Self::line`].
    #[track_caller]
    pub const fn quadratic_bezier(bezier: QuadraticBezier) -> CurveShape {
        bezier.validate();
        CurveShape::new(CurvePath::QuadraticBezier(bezier))
    }

    /// A shaped text run in `font`, single-line, top-left.
    ///
    /// `text` comes from [`crate::Ui::intern`] or [`crate::Ui::fmt`]. An unusable font (size or leading not finite and above the UI epsilon) shapes nothing.
    pub const fn text(text: InternedStr, font: GlyphFont) -> TextShape {
        TextShape::new(text, font)
    }

    /// A colored triangle `mesh`, its vertices logical px from its rect's top-left.
    pub const fn mesh(mesh: &Mesh) -> MeshShape<'_> {
        MeshShape::new(mesh)
    }
}

#[cfg(test)]
mod tests;
