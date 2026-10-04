//! Shapes from authoring to record: one concrete builder type per paint
//! primitive, each lowering itself into a [`ShapeRecord`](record::ShapeRecord);
//! the per-tree [`Shapes`](shapes::Shapes) buffer that holds the records; and
//! the lowering, hashing and paint-side code over them.

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
use crate::primitives::math::domain;
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
/// The bound on [`crate::Ui::add_shape`], and the reason there is no
/// `Shape` enum: every kind is a concrete type that knows how to lower
/// itself, so an authoring kind is a struct plus this impl — no
/// variant, no `From`, and no authoring-side dispatch to keep in step.
///
/// That buys the *authoring* surface only, and only for a kind that
/// lowers into an existing `ShapeRecord` variant — which is what `Shape::circle`, `line` and `cubic_bezier`
/// do (all `Curve`), and `rect`, `shadow` and `triangle` (all `Quad`).
/// A kind that needs a *new* record variant is a different job: the
/// record enum is the pipeline's dispatch point, and a new variant has
/// to be answered in `bbox_local`, `NanCheck`, `compute_record_hash`,
/// the encoder's `emit_one_shape`, and cascade's `compute_paint_rect`.
/// All five are exhaustive matches, so the compiler names them; none of
/// them is optional.
///
/// Sealed: the methods live on `sealed::LowerShape`, in a module private
/// to `crate::shape`, which is what lets them name the crate-private
/// `RecordStore` and `ShapeRecord` while the bound itself stays public.
/// Implementing it outside the crate would mean building a `ShapeRecord`,
/// which is not reachable, so sealing costs callers nothing they could
/// have used.
pub trait Lower: sealed::LowerShape {}

impl<T: sealed::LowerShape> Lower for T {}

mod sealed {
    use crate::scene::record_store::RecordStore;
    use crate::shape::record::ShapeRecord;
    use std::fmt;

    // `unreachable_pub` and `private_interfaces` both fire here, and both
    // describe the seal rather than a mistake: the trait must be `pub`
    // because a public trait cannot have a private supertrait, and making
    // it one puts the crate-private `RecordStore` / `ShapeRecord` into a
    // publicly *reachable* signature. Nothing outside the crate can name
    // the trait to call or implement it, so neither exposure can occur.
    /// `Debug` is a supertrait so the NaN gate can name the shape it
    /// dropped. Every authoring kind derives it already.
    #[expect(
        private_interfaces,
        reason = "nothing outside the crate can name the sealed trait, so the crate-private types in its signature cannot leak"
    )]
    pub trait LowerShape: fmt::Debug {
        /// True if this shape paints nothing visible. Checked before
        /// [`Self::lower`] so a no-op never pays for payload staging,
        /// mesh hashing, or text interning.
        fn is_noop(&self) -> bool;

        /// True if any authored input carries a NaN — the crate's NaN
        /// screen, run by `Shapes::add` beside [`Self::is_noop`] and for
        /// the same reason: both answers are known before lowering, and
        /// lowering is what stages mesh vertices, interns a gradient, and
        /// copies text into the arena. A shape rejected after that leaves
        /// the bytes behind for the frame.
        ///
        /// **`O(1)` for every kind.** Bulk inputs are not scanned here;
        /// they are read off the AABB they were already folded into —
        /// memoized on a `Mesh`, computed once at construction for a
        /// polyline — under the contract that a NaN vertex yields a NaN
        /// bbox. See [`Aabb`](crate::primitives::geometry::rect::aabb::Aabb).
        ///
        /// Separate from `is_noop` rather than folded into it: "paints
        /// nothing" and "carries a NaN" are different facts about a
        /// shape, and only one of them is worth a debug assert.
        fn has_nan(&self) -> bool;

        /// Convert to the stored form, appending any bulk payload
        /// (polyline points, mesh vertices, gradients, text bytes) to
        /// `store` on the way.
        ///
        /// **Every impl opens by destructuring `Self`.** Taking `self` by
        /// value and reading `self.field` one at a time makes an
        /// authoring field that never reaches the record compile clean:
        /// `dead_code` only catches one nothing reads at all, and a field
        /// `is_noop` validates counts as read. Naming them all turns the
        /// omission into a build error, which is the enforcement the
        /// record side already gets for free from its struct literal.
        fn lower(self, store: &mut RecordStore) -> ShapeRecord;
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
    /// transparent-filled, borderless, sharp-cornered — chain
    /// [`RectShape::fill`] / [`RectShape::border`] / [`RectShape::corners`].
    pub const fn rect(rect: Rect) -> RectShape {
        RectShape::new(RectKind::Rounded, Some(rect))
    }

    /// A rounded rectangle painting the owner's full arranged rect.
    pub const fn owner_rect() -> RectShape {
        RectShape::new(RectKind::Rounded, None)
    }

    /// An inverse-mask rectangle over `rect` — the sibling of
    /// [`Self::rect`], same chainable fill/border/corners.
    pub const fn windowed_rect(rect: Rect) -> RectShape {
        RectShape::new(RectKind::Windowed, Some(rect))
    }

    /// A windowed rectangle painting the owner's full arranged rect.
    pub const fn owner_windowed_rect() -> RectShape {
        RectShape::new(RectKind::Windowed, None)
    }

    /// A triangle with corners `a`/`b`/`c` (owner-local). Starts sharp
    /// (radius 0), transparent-filled, borderless.
    pub const fn triangle(a: Vec2, b: Vec2, c: Vec2) -> TriangleShape {
        TriangleShape::new(a, b, c)
    }

    /// A straight line from `a` to `b` in `stroke` (`Butt` cap).
    ///
    /// # Panics
    ///
    /// Panics unless the stroke's width is a *length* and its colour a
    /// *colour*. The same holds for every stroked shape below.
    #[track_caller]
    pub const fn line(a: Vec2, b: Vec2, stroke: Stroke) -> CurveShape {
        stroke.validate();
        CurveShape::new(CurveGeometry::Line { a, b }, stroke)
    }

    /// A polyline through `points` in `stroke` (`Butt` cap, `Miter`
    /// join). Chain [`PolylineShape::per_point`] or
    /// [`PolylineShape::per_segment`] to vary the colour along it.
    ///
    /// # Panics
    ///
    /// As [`Self::line`].
    #[track_caller]
    pub fn polyline(points: &[Vec2], stroke: Stroke) -> PolylineShape<'_> {
        stroke.validate();
        PolylineShape::new(points, stroke)
    }

    /// A cubic Bézier through control points `p0..=p3` in `stroke`
    /// (`Butt` cap).
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
        CurveShape::new(CurveGeometry::CubicBezier { p0, p1, p2, p3 }, stroke)
    }

    /// A quadratic Bézier through `p0`/`p1`/`p2`. See
    /// [`Self::cubic_bezier`].
    ///
    /// # Panics
    ///
    /// As [`Self::line`].
    #[track_caller]
    pub const fn quadratic_bezier(p0: Vec2, p1: Vec2, p2: Vec2, stroke: Stroke) -> CurveShape {
        stroke.validate();
        CurveShape::new(CurveGeometry::QuadraticBezier { p0, p1, p2 }, stroke)
    }

    /// A circular arc sweeping `sweep` radians from `start_angle` in
    /// `stroke` (`Butt` cap) — chain [`CurveShape::ramp`] /
    /// [`CurveShape::cap`]. `radius`: a *length*; `start_angle` and
    /// `sweep`: *angles*.
    ///
    /// # Panics
    ///
    /// Panics unless each value holds its kind, as [`Self::line`] says of
    /// the stroke.
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
                center,
                radius: domain::length(radius),
                start_angle: domain::angle(start_angle),
                sweep: domain::angle(sweep),
            },
            stroke,
        )
    }

    /// A full circle — [`Self::arc`] with a `2π` sweep, which closes
    /// seamlessly under the default `Butt` cap.
    #[track_caller]
    pub const fn circle(center: Vec2, radius: f32, stroke: Stroke) -> CurveShape {
        Self::arc(center, radius, 0.0, TAU, stroke)
    }

    /// A shaped text run in `font`. Starts white, single-line, top-left
    /// — chain [`TextShape::color`] / [`TextShape::wrap`] /
    /// [`TextShape::align`] and friends.
    ///
    /// One [`GlyphFont`] rather than a size and a leading: it is the same
    /// value the record stores and the shape cache is keyed on, and a
    /// theme-driven caller gets it from
    /// [`TextStyle::font`](crate::TextStyle::font) rather than pairing
    /// two numbers itself.
    ///
    /// `text` comes from [`crate::Ui::intern`] or [`crate::Ui::fmt`],
    /// which place the bytes in the frame's text arena. Widget
    /// constructors take borrowed or owned text directly because they
    /// defer interning until `show`.
    pub const fn text(text: InternedStr, font: GlyphFont) -> TextShape {
        TextShape::new(text, font)
    }

    /// A `shadow` of the owner's full rect.
    ///
    /// # Panics
    ///
    /// Panics unless the shadow's colour is a *colour*, its offset and
    /// spread *offsets*, and its blur a *length*.
    #[track_caller]
    pub const fn shadow(shadow: Shadow) -> ShadowShape {
        shadow.validate();
        ShadowShape::new(shadow)
    }

    /// A textured rect from `handle` painting the owner's full rect at the
    /// default fit/filters, untinted.
    pub fn image(handle: ImageHandle) -> ImageShape {
        ImageShape::new(handle)
    }

    /// A baked SVG icon painting the owner's full rect, aspect preserved and
    /// untinted. The icon is rasterized at the exact physical pixel size the
    /// rect resolves to, so it is crisp at every display scale.
    ///
    /// `handle` comes from [`IconSet::handle`](crate::IconSet::handle).
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
