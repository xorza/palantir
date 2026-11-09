//! Shapes from authoring to record: builders, the per-tree record buffer, the payload store they
//! lower into, their paint animations, and lowering.

#![expect(
    private_interfaces,
    reason = "every `impl sealed::LowerShape` names the crate-private `RecordStore` and `ShapeRecord` in a reachable signature, which is the seal working as designed; it fires per impl site"
)]

pub(crate) mod curve;
pub(crate) mod effect;
pub(crate) mod icon;
pub(crate) mod image;
pub(crate) mod lower;
pub(crate) mod mesh;
pub(crate) mod paint;
pub(crate) mod paint_animation;
pub(crate) mod polyline;
pub(crate) mod record;
pub(crate) mod record_store;
pub(crate) mod rect;
pub(crate) mod shadow;
pub(crate) mod shapes;
pub(crate) mod stroke_bounds;
pub(crate) mod style;
pub(crate) mod text;
pub(crate) mod triangle;

use crate::image_registry::image_handle::ImageHandle;
use crate::primitives::effect::effect_kind::{BackdropEffect, ColorEffect};
use crate::primitives::effect::effect_name::EffectName;
use crate::primitives::effect::effect_params::EffectParams;
use crate::primitives::geometry::circular_arc::CircularArc;
use crate::primitives::geometry::cubic_bezier::CubicBezier;
use crate::primitives::geometry::mesh::Mesh;
use crate::primitives::geometry::quadratic_bezier::QuadraticBezier;
use crate::primitives::identity::icon_handle::IconHandle;
use crate::primitives::math::domain::vec2;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::text::interned_str::InternedStr;
use crate::shape::curve::{CurveGeometry, CurveShape};
use crate::shape::effect::EffectShape;
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
    use crate::effect::effect_registry::EffectRegistry;
    use crate::primitives::reduction::Reductions;
    use crate::shape::record::ShapeRecord;
    use crate::shape::record_store::RecordStore;
    use std::fmt;

    #[expect(
        private_interfaces,
        unnameable_types,
        reason = "nothing outside the crate can name the sealed trait, so the crate-private types in its signature cannot leak"
    )]
    pub trait LowerShape: fmt::Debug {
        /// Whether a shape of this kind can be a pane, which the record notes; only a rect's
        /// material and a backdrop effect capture, so every other kind skips the check.
        const CAN_BE_PANE: bool = false;

        /// Whether the shape reads a paint animation's time, so an animation that drives no
        /// channel moves it; only an effect does.
        const READS_TIME: bool = false;

        /// True if this shape paints nothing; checked before [`Self::lower`].
        fn is_noop(&self) -> bool;

        /// Resolve what the shape names in the host's registries, before [`Self::is_noop`] and
        /// [`Self::lower`]: an effect's id, or the id of the alternative it paints for what
        /// `reductions` reduces. Nothing by default.
        fn resolve(&mut self, _effects: &EffectRegistry, _reductions: Reductions) {}

        /// Convert to the stored form. Every impl destructures `Self`, so an unrecorded authoring field is a build error.
        fn lower(self, store: &mut RecordStore) -> ShapeRecord;
    }
}

/// Constructor namespace for the paint primitives; each returns the concrete shape it names.
#[derive(Clone, Copy, Debug)]
pub struct Shape;

impl Shape {
    /// The color effect `name` with `params`, over the owner's arranged rect, or
    /// [`EffectShape::at`] a rect of its own; sharp until [`EffectShape::corners`].
    ///
    /// # Panics
    ///
    /// At [`Ui::add_shape`](crate::Ui::add_shape), when no effect is loaded under `name`
    /// ([`Ui::load_effect`](crate::Ui::load_effect)), or one with other parameters or kind.
    pub const fn effect<U: EffectParams>(
        name: EffectName<U, ColorEffect>,
        params: U,
    ) -> EffectShape<U> {
        EffectShape::new(name, params)
    }

    /// The backdrop effect `name` with `params`, over the owner's arranged rect, or
    /// [`EffectShape::at`] a rect of its own; sharp until [`EffectShape::corners`]. It reads what
    /// is painted under it through `effect_backdrop`, sharp until
    /// [`EffectShape::blur`](EffectShape#method.blur) and within its rect until
    /// [`EffectShape::reach`](EffectShape#method.reach).
    ///
    /// # Panics
    ///
    /// At [`Ui::add_shape`](crate::Ui::add_shape), when no effect is loaded under `name`
    /// ([`Ui::load_effect`](crate::Ui::load_effect)), or one with other parameters or kind.
    pub const fn backdrop_effect<U: EffectParams>(
        name: EffectName<U, BackdropEffect>,
        params: U,
    ) -> EffectShape<U, BackdropEffect> {
        EffectShape::new(name, params)
    }

    /// The box `chrome` paints over the owner's arranged rect, or [`RectShape::at`] a rect of its
    /// own: its fill, border, corners, shadow and material, as a node's chrome paints them. A
    /// color or a brush is a box that only fills.
    ///
    /// # Panics
    ///
    /// Panics unless `chrome` passes its checks, as a widget's `chrome` does; at
    /// [`Ui::add_shape`](crate::Ui::add_shape), when no effect is loaded under its material's
    /// name.
    #[track_caller]
    pub fn rect(chrome: impl Into<Chrome>) -> RectShape {
        RectShape::new(RectKind::Rounded, chrome.into())
    }

    /// An inverse-mask box: `chrome` paints outside its rounded boundary. Placed as
    /// [`Self::rect`].
    ///
    /// # Panics
    ///
    /// As [`Self::rect`].
    #[track_caller]
    pub fn windowed_rect(chrome: impl Into<Chrome>) -> RectShape {
        RectShape::new(RectKind::Windowed, chrome.into())
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
    /// Panics unless every point is an *offset*, the stroke width a *length* and its color a *color*.
    #[track_caller]
    pub const fn line(a: Vec2, b: Vec2, stroke: Border) -> CurveShape {
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
    pub fn polyline(points: &[Vec2], stroke: Border) -> PolylineShape<'_> {
        stroke.validate();
        let shape = PolylineShape::new(points, stroke);
        // One check of the folded box covers every point.
        shape.bbox.validate();
        shape
    }

    /// A cubic Bézier `bezier` in `stroke` (`Butt` cap).
    ///
    /// # Panics
    ///
    /// As [`Self::line`].
    #[track_caller]
    pub const fn cubic_bezier(bezier: CubicBezier, stroke: Border) -> CurveShape {
        stroke.validate();
        bezier.validate();
        CurveShape::new(CurveGeometry::CubicBezier(bezier), stroke)
    }

    /// A quadratic Bézier `bezier` in `stroke` (`Butt` cap).
    ///
    /// # Panics
    ///
    /// As [`Self::line`].
    #[track_caller]
    pub const fn quadratic_bezier(bezier: QuadraticBezier, stroke: Border) -> CurveShape {
        stroke.validate();
        bezier.validate();
        CurveShape::new(CurveGeometry::QuadraticBezier(bezier), stroke)
    }

    /// A circular `arc` in `stroke` (`Butt` cap). A sweep past a full circle, `±2π`, draws the full circle.
    ///
    /// # Panics
    ///
    /// Panics unless the stroke holds its kinds as in [`Self::line`], and the arc's center is an *offset*, its radius a *length* and its angles *angles*.
    #[track_caller]
    pub const fn arc(arc: CircularArc, stroke: Border) -> CurveShape {
        stroke.validate();
        arc.validate();
        CurveShape::new(CurveGeometry::Arc(arc.coerced()), stroke)
    }

    /// A full circle: [`Self::arc`] with a `2π` sweep from angle 0.
    ///
    /// # Panics
    ///
    /// As [`Self::arc`].
    #[track_caller]
    pub const fn circle(center: Vec2, radius: f32, stroke: Border) -> CurveShape {
        Self::arc(
            CircularArc {
                center,
                radius,
                start_angle: 0.0,
                sweep: TAU,
            },
            stroke,
        )
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
    /// Panics unless the shadow holds the kinds [`Chrome`] lists for one.
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
