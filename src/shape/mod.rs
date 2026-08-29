//! Authoring shapes: one concrete builder type per paint primitive, each
//! lowering itself into a `ShapeRecord`.
//!
//! `private_interfaces` is allowed module-wide: every `impl
//! sealed::LowerShape` names the crate-private `RecordStore` and
//! `ShapeRecord` in a publicly *reachable* signature, which is the seal
//! working as designed (see `sealed` below). The lint fires per impl site,
//! so the decision belongs here rather than over each one.
#![allow(private_interfaces)]

/// Chainable builder setters — `self.field = value.into(); self` — for the
/// authoring surface a shape kind exposes.
///
/// A macro because the bodies are identical across kinds while the names are
/// not — `at`, `tint`, `corners`, `cap` and `stroke` repeat over four or
/// five kinds each. Spelled out, that is how one of them ends up taking a
/// concrete type instead of `impl Into`, or setting the wrong field.
macro_rules! shape_setters {
    ($ty:ty {
        $(
            $(#[$meta:meta])*
            $name:ident: $arg:ty => $($field:ident).+,
        )*
    }) => {
        impl $ty {
            $(
                $(#[$meta])*
                pub fn $name(mut self, $name: impl Into<$arg>) -> Self {
                    self.$($field).+ = $name.into();
                    self
                }
            )*
        }
    };
}

/// The owner-relative paint rect the rect-shaped kinds carry: the `is_noop`
/// clause that reads it, and — for the kinds that let a caller author one —
/// the `at` setter, named in the invocation.
///
/// `Shape::rect` and `Shape::owner_rect` take theirs up front, so `RectShape`
/// asks for the clause alone; a second way to say the same thing would only
/// make `Shape::rect(a).at(b)` expressible.
macro_rules! local_rect_shape {
    ($ty:ty) => {
        impl $ty {
            /// True when an explicit paint rect was authored and it covers
            /// no pixels — what every such kind's `is_noop` opens with.
            fn rect_is_noop(&self) -> bool {
                self.local_rect.is_some_and(|rect| rect.is_paint_empty())
            }
        }
    };
    ($ty:ty, at) => {
        local_rect_shape!($ty);

        impl $ty {
            /// Paint into `rect`, in owner-relative coords, instead of the
            /// owner's whole arranged rect.
            pub fn at(mut self, rect: impl Into<$crate::primitives::rect::Rect>) -> Self {
                self.local_rect = Some(rect.into());
                self
            }
        }
    };
}

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
    use crate::scene::shapes::record::ShapeRecord;

    // `unreachable_pub` and `private_interfaces` both fire here, and both
    // describe the seal rather than a mistake: the trait must be `pub`
    // because a public trait cannot have a private supertrait, and making
    // it one puts the crate-private `RecordStore` / `ShapeRecord` into a
    // publicly *reachable* signature. Nothing outside the crate can name
    // the trait to call or implement it, so neither exposure can occur.
    // Each `impl` repeats the second allow, which fires per impl site.
    /// `Debug` is a supertrait so the NaN gate can name the shape it
    /// dropped. Every authoring kind derives it already.
    #[allow(unreachable_pub, private_interfaces)]
    pub trait LowerShape: std::fmt::Debug {
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
        /// bbox. See [`Aabb`](crate::primitives::rect::aabb::Aabb).
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
        PolylineShape::new(points, colors, width)
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
    pub fn text(text: InternedStr, font: GlyphFont) -> TextShape {
        TextShape::new(text, font)
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
            desaturate: false,
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

#[cfg(test)]
mod tests;
