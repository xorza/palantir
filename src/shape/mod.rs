//! Shapes from authoring to record: builders, the per-tree record buffer, and lowering.

#![expect(
    private_interfaces,
    reason = "every `impl sealed::LowerShape` names the crate-private `RecordStore` and `ShapeRecord` in a reachable signature, which is the seal working as designed; it fires per impl site"
)]

pub(crate) mod curve;
pub(crate) mod hash;
pub(crate) mod icon;
pub(crate) mod image;
pub(crate) mod lower;
pub(crate) mod mesh;
pub(crate) mod paint;
pub(crate) mod polyline;
pub(crate) mod record;
pub(crate) mod rect;
pub(crate) mod shadow;
pub(crate) mod shapes;
pub(crate) mod stroke_bounds;
pub(crate) mod style;
pub(crate) mod text;
pub(crate) mod triangle;

use crate::icons::icon_set::IconHandle;
use crate::primitives::geometry::mesh::Mesh;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::domain::{self, vec2};
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::paint::stroke::Stroke;
use crate::primitives::text::interned_str::InternedStr;
use crate::renderer::image_registry::image_handle::ImageHandle;
use crate::shape::curve::{CurveGeometry, CurveShape};
use crate::shape::icon::IconShape;
use crate::shape::image::ImageShape;
use crate::shape::mesh::MeshShape;
use crate::shape::polyline::PolylineShape;
use crate::shape::rect::{RectKind, RectShape};
use crate::shape::shadow::ShadowShape;
use crate::shape::text::TextShape;
use crate::shape::triangle::TriangleShape;
use crate::text::glyph_font::GlyphFont;
use glam::Vec2;
use std::f32::consts::TAU;

/// Lowers an authoring shape into the frame's record buffer.
///
/// The bound on [`crate::Ui::add_shape`]. Sealed.
pub trait Lower: sealed::LowerShape {}

impl<T: sealed::LowerShape> Lower for T {}

mod sealed {
    use crate::scene::record_store::RecordStore;
    use crate::shape::record::ShapeRecord;
    use std::fmt;

    #[expect(
        private_interfaces,
        reason = "nothing outside the crate can name the sealed trait, so the crate-private types in its signature cannot leak"
    )]
    pub trait LowerShape: fmt::Debug {
        /// True if this shape paints nothing; checked before [`Self::lower`].
        fn is_noop(&self) -> bool;

        /// True if any authored input carries a NaN; checked before lowering. `O(1)`: bulk inputs use their memoized bbox, where a NaN vertex yields a NaN bbox.
        fn has_nan(&self) -> bool;

        /// Convert to the stored form. Every impl destructures `Self`, so an unrecorded authoring field is a build error.
        fn lower(self, store: &mut RecordStore) -> ShapeRecord;
    }
}

/// Constructor namespace for the paint primitives; each returns the concrete shape it names.
#[derive(Clone, Copy, Debug)]
pub struct Shape;

impl Shape {
    /// A rounded rectangle painting `rect` (owner-relative); starts transparent, borderless, sharp.
    ///
    /// # Panics
    ///
    /// Panics unless every component of `rect` is an [offset](crate::widget::domain::offset).
    #[track_caller]
    pub const fn rect(rect: Rect) -> RectShape {
        rect.validate();
        RectShape::new(RectKind::Rounded, Some(rect))
    }

    /// A rounded rectangle painting the owner's full arranged rect.
    pub const fn owner_rect() -> RectShape {
        RectShape::new(RectKind::Rounded, None)
    }

    /// An inverse-mask rectangle over `rect`; chains like [`Self::rect`].
    ///
    /// # Panics
    ///
    /// As [`Self::rect`].
    #[track_caller]
    pub const fn windowed_rect(rect: Rect) -> RectShape {
        rect.validate();
        RectShape::new(RectKind::Windowed, Some(rect))
    }

    /// A windowed rectangle painting the owner's full arranged rect.
    pub const fn owner_windowed_rect() -> RectShape {
        RectShape::new(RectKind::Windowed, None)
    }

    /// A triangle with corners `a`/`b`/`c` (owner-local). Starts sharp, transparent, borderless.
    ///
    /// # Panics
    ///
    /// Panics unless every corner is an [offset](crate::widget::domain::offset).
    #[track_caller]
    pub const fn triangle(a: Vec2, b: Vec2, c: Vec2) -> TriangleShape {
        TriangleShape::new(vec2::offset(a), vec2::offset(b), vec2::offset(c))
    }

    /// A straight line from `a` to `b` in `stroke` (`Butt` cap).
    ///
    /// # Panics
    ///
    /// Panics unless every point is an *offset*, the stroke width a *length* and its colour a *colour*.
    #[track_caller]
    pub const fn line(a: Vec2, b: Vec2, stroke: Stroke) -> CurveShape {
        stroke.validate();
        let (a, b) = (vec2::offset(a), vec2::offset(b));
        CurveShape::new(CurveGeometry::Line { a, b }, stroke)
    }

    /// A polyline through `points` in `stroke` (`Butt` cap, `Miter` join).
    ///
    /// # Panics
    ///
    /// As [`Self::line`].
    #[track_caller]
    pub fn polyline(points: &[Vec2], stroke: Stroke) -> PolylineShape<'_> {
        stroke.validate();
        let shape = PolylineShape::new(points, stroke);
        // One check of the folded box covers every point.
        shape.bbox.validate();
        shape
    }

    /// A cubic Bézier through control points `p0..=p3` in `stroke` (`Butt` cap).
    ///
    /// # Panics
    ///
    /// As [`Self::line`].
    #[track_caller]
    pub const fn cubic_bezier(
        p0: Vec2,
        p1: Vec2,
        p2: Vec2,
        p3: Vec2,
        stroke: Stroke,
    ) -> CurveShape {
        stroke.validate();
        let (p0, p1) = (vec2::offset(p0), vec2::offset(p1));
        let (p2, p3) = (vec2::offset(p2), vec2::offset(p3));
        CurveShape::new(CurveGeometry::CubicBezier { p0, p1, p2, p3 }, stroke)
    }

    /// A quadratic Bézier through `p0`/`p1`/`p2`.
    ///
    /// # Panics
    ///
    /// As [`Self::line`].
    #[track_caller]
    pub const fn quadratic_bezier(p0: Vec2, p1: Vec2, p2: Vec2, stroke: Stroke) -> CurveShape {
        stroke.validate();
        let (p0, p1, p2) = (vec2::offset(p0), vec2::offset(p1), vec2::offset(p2));
        CurveShape::new(CurveGeometry::QuadraticBezier { p0, p1, p2 }, stroke)
    }

    /// A circular arc sweeping `sweep` radians from `start_angle` in `stroke` (`Butt` cap). `radius`: a *length*; angles in radians.
    ///
    /// # Panics
    ///
    /// Panics unless each value holds its kind.
    #[track_caller]
    pub const fn arc(
        center: Vec2,
        radius: f32,
        start_angle: f32,
        sweep: f32,
        stroke: Stroke,
    ) -> CurveShape {
        stroke.validate();
        CurveShape::new(
            CurveGeometry::Arc {
                center: vec2::offset(center),
                radius: domain::length(radius),
                start_angle: domain::angle(start_angle),
                sweep: domain::angle(sweep),
            },
            stroke,
        )
    }

    /// A full circle: [`Self::arc`] with a `2π` sweep.
    #[track_caller]
    pub const fn circle(center: Vec2, radius: f32, stroke: Stroke) -> CurveShape {
        Self::arc(center, radius, 0.0, TAU, stroke)
    }

    /// A shaped text run in `font`. Starts white, single-line, top-left.
    ///
    /// `text` comes from [`crate::Ui::intern`] or [`crate::Ui::fmt`]. An unusable font (size or leading not finite and above the UI epsilon) shapes nothing.
    pub const fn text(text: InternedStr, font: GlyphFont) -> TextShape {
        TextShape::new(text, font)
    }

    /// A `shadow` of the owner's full rect.
    ///
    /// # Panics
    ///
    /// Panics unless colour, offset, spread and blur hold their kinds.
    #[track_caller]
    pub const fn shadow(shadow: Shadow) -> ShadowShape {
        shadow.validate();
        ShadowShape::new(shadow)
    }

    /// A textured rect from `handle` painting the owner's full rect, untinted.
    pub fn image(handle: ImageHandle) -> ImageShape {
        ImageShape::new(handle)
    }

    /// A baked SVG icon painting the owner's full rect, aspect preserved, untinted.
    pub fn icon(handle: IconHandle) -> IconShape {
        IconShape::new(handle)
    }

    /// A colored triangle `mesh` painting the owner's full rect, untinted.
    pub const fn mesh(mesh: &Mesh) -> MeshShape<'_> {
        MeshShape::new(mesh)
    }
}

#[cfg(test)]
mod tests;
