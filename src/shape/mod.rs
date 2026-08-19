pub(crate) mod curve;
pub(crate) mod icon;
pub(crate) mod image;
pub(crate) mod mesh;
pub(crate) mod polyline;
pub(crate) mod rect;
pub(crate) mod shadow;
pub(crate) mod stroke_bounds;
pub(crate) mod style;
pub(crate) mod text;
pub(crate) mod triangle;

use crate::icons::icon_set::IconHandle;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::image::{ImageDownsample, ImageFilter, ImageFit};
use crate::primitives::interned_str::InternedStr;
use crate::primitives::mesh::Mesh;
use crate::primitives::rect::Rect;
use crate::primitives::shadow::Shadow;
use crate::primitives::stroke::Stroke;
use crate::renderer::image_registry::ImageHandle;
use crate::shape::curve::{CurveGeometry, CurveShape};
use crate::shape::icon::{IconFit, IconShape};
use crate::shape::image::ImageShape;
use crate::shape::mesh::MeshShape;
use crate::shape::polyline::{PolylineColors, PolylineShape};
use crate::shape::rect::{RectKind, RectShape};
use crate::shape::shadow::ShadowShape;
use crate::shape::style::{LineCap, LineJoin};
use crate::shape::text::TextShape;
use crate::shape::triangle::TriangleShape;
use glam::Vec2;
use std::f32::consts::TAU;

/// Lowers an authoring shape into the frame's record buffer.
///
/// The bound on [`crate::Ui::add_shape`], and the reason there is no
/// `Shape` enum: every kind is a concrete type that knows how to lower
/// itself, so adding one is a struct plus this impl — no variant, no
/// `From`, and no arm in a dispatch match to keep in step.
///
/// Sealed. The real methods live on a supertrait in a private module,
/// which is what lets them name the crate-private `RecordStore` and
/// `ShapeRecord` while the bound itself stays public. Implementing it
/// outside the crate would mean building a `ShapeRecord`, which is not
/// reachable, so sealing costs callers nothing they could have used.
pub trait Lower: sealed::Lower {}

impl<T: sealed::Lower> Lower for T {}

pub(crate) mod sealed {
    use crate::scene::record_store::RecordStore;
    use crate::scene::shapes::record::ShapeRecord;

    // `pub` is what seals it: a public trait cannot have a private
    // supertrait, so this has to be `pub` even though the private module
    // keeps it unreachable from outside. That unreachability *is* the
    // seal, so the lint is describing the intent rather than a mistake.
    // Two lints fire on this pattern and both are describing the seal
    // rather than a mistake:
    //
    // - `unreachable_pub`: the trait must be `pub` because a public
    //   trait cannot have a private supertrait, yet the private module
    //   keeps it unnameable from outside. That unreachability *is* the
    //   seal.
    // - `private_interfaces`: making it a supertrait of the public
    //   `Lower` leaks it into public *reachability*, so rustc flags the
    //   crate-private `RecordStore` / `ShapeRecord` in these signatures.
    //   Nothing outside the crate can name the trait to call or
    //   implement it, so the exposure it warns about cannot occur. Each
    //   `impl` repeats the allow because the lint fires per impl site.
    #[allow(unreachable_pub, private_interfaces)]
    pub trait Lower {
        /// True if this shape paints nothing visible. Checked before
        /// [`Self::lower`] so a no-op never pays for payload staging,
        /// mesh hashing, or text interning.
        fn is_noop(&self) -> bool;

        /// Convert to the stored form, appending any bulk payload
        /// (polyline points, mesh vertices, gradients, text bytes) to
        /// `store` on the way.
        fn lower(self, store: &RecordStore) -> ShapeRecord;
    }
}

/// Constructor namespace for the paint primitives.
///
/// Not a type you hold — every constructor returns the concrete shape
/// it names (`Shape::rect` a [`RectShape`], `Shape::circle` a
/// [`CurveShape`]), which is what [`crate::Ui::add_shape`] takes via
/// [`Lower`]. There is no erased `Shape` value: a shape is built and
/// consumed in one expression, so erasing it only cost a match to
/// undo.
#[derive(Clone, Copy, Debug)]
pub struct Shape;

impl Shape {
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
    pub fn polyline<'a>(
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

    /// A shaped text run at `font_size_px` with `line_height_px`
    /// leading, both in logical px. Starts white, single-line,
    /// top-left, sans regular — chain [`TextShape::color`] /
    /// [`TextShape::wrap`] / [`TextShape::align`] and friends.
    ///
    /// `text` comes from [`crate::Ui::intern`] or [`crate::Ui::fmt`],
    /// which place the bytes in the frame's text arena. Widget
    /// constructors take borrowed or owned text directly because they
    /// defer interning until `show`.
    pub fn text(text: InternedStr, font_size_px: f32, line_height_px: f32) -> TextShape {
        TextShape::new(text, font_size_px, line_height_px)
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
            downsample: ImageDownsample::default(),
            tint: Color::WHITE,
        }
    }

    /// A baked SVG icon painting the owner's full rect, aspect preserved and
    /// untinted. The icon is rasterized at the exact physical pixel size the
    /// rect resolves to, so it is crisp at every display scale.
    ///
    /// `handle` comes from [`IconSet::handle`](crate::IconSet::handle).
    pub fn icon(handle: IconHandle) -> IconShape {
        IconShape {
            handle,
            local_rect: None,
            fit: IconFit::default(),
            tint: Color::WHITE,
        }
    }

    /// A colored triangle `mesh` painting the owner's full rect, untinted.
    pub fn mesh(mesh: &Mesh) -> MeshShape<'_> {
        MeshShape {
            mesh,
            local_rect: None,
            tint: Color::WHITE,
        }
    }
}

#[inline]
fn local_rect_paint_empty(local_rect: &Option<Rect>) -> bool {
    local_rect.is_some_and(|rect| rect.is_paint_empty())
}

#[cfg(test)]
mod tests;
