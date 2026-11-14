//! [`EffectPaint`]: an effect of either kind over a rect, a circle or a triangle. Lowers to
//! a `ShapeRecord::Closed` of `ClosedPaint::Effect`.

use std::fmt;

use crate::effect::MAX_IMAGE_SLOTS;
use crate::image_registry::image_handle::ImageHandle;
use crate::primitives::effect::effect_kind::{BackdropEffect, ColorEffect, EffectKind};
use crate::primitives::effect::effect_name::{CheckedEffectName, EffectName};
use crate::primitives::effect::effect_params::EffectParams;
use crate::primitives::math::domain;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::material::backdrop_fallback::BackdropFallback;
use crate::shape::PaintedBy;
use crate::shape::circle::CircleShape;
use crate::shape::draw::{ClosedPaintRef, Draw};
use crate::shape::geometry::closed_geometry::ClosedGeometry;
use crate::shape::rect::RectShape;
use crate::shape::sealed;
use crate::shape::triangle::TriangleShape;

/// An app-written effect over a [`RectShape`], a [`CircleShape`] or a [`TriangleShape`]
/// ([`Ui::add_shape`](crate::Ui::add_shape)), with one instance of its parameters: a color
/// effect ([`Self::new`] of a color effect's name), or a backdrop effect, which also reads what is
/// painted under it. Paints in record order, as any paint does, over its shape, or around it alone
/// as its halo ([`Self::outside`]); clipping ancestors clip it. A [`Material`](crate::Material) is the theme form of
/// a backdrop effect, which a box wears under its fill and border, and draws as an `EffectPaint`
/// does. Refused: a `Brush` variant, which every brush family would have to draw, and which a
/// theme file saves where no parameters fit.
///
/// A backdrop effect is a backdrop capture: what is painted under it is copied out of the frame at
/// its place in paint order, blurred by its [σ](Self::backdrop_blur), and kept across frames until
/// something under its reads changes, as a material's is; how far past its shape it reads is its
/// source's [reach](crate::EffectSource::reach) over these parameters.
///
/// Its time comes from a [`PaintAnimation`](crate::PaintAnimation) through
/// [`AddedShape::animate`](crate::AddedShape::animate), [`PaintAnimation::clock`] when only
/// the time moves; a turn turns it about its owner's center, as a quad turns. While motion or
/// power use is reduced it paints the alternative its source names for it
/// ([`ReducedEffect`](crate::ReducedEffect)); while transparency is reduced, a backdrop effect paints
/// its [fallback](Self::fallback) over its shape in its place, the theme's by default, as a
/// [`Material`](crate::Material) does.
///
/// [`PaintAnimation::clock`]: crate::PaintAnimation::clock
#[must_use]
pub struct EffectPaint<U: EffectParams, K: EffectKind = ColorEffect> {
    name: EffectName<U, K>,
    params: U,
    /// The σ a backdrop effect blurs what it reads by, logical px; 0 for a color effect, which
    /// reads none.
    backdrop_blur: f32,
    /// What a backdrop effect paints in its place while transparency is reduced; `()` for a color
    /// effect, which reads nothing under it.
    fallback: K::Fallback,
    /// The images bound, in slot order, each slot past the last `None`.
    images: [Option<ImageHandle>; MAX_IMAGE_SLOTS as usize],
    /// A color effect's halo: how far past its shape it paints, logical px, outside the shape
    /// alone; `None` paints over the shape.
    outside: Option<f32>,
}

impl<U: EffectParams, K: EffectKind> EffectPaint<U, K> {
    /// The effect `name` with `params`, its kind the name's.
    pub const fn new(name: EffectName<U, K>, params: U) -> Self {
        Self {
            name,
            params,
            backdrop_blur: 0.0,
            fallback: K::FALLBACK,
            images: [const { None }; MAX_IMAGE_SLOTS as usize],
            outside: None,
        }
    }

    /// Bind `images` to the effect's slots, in order: as many as its source declares
    /// ([`EffectSource::image_slots`](crate::EffectSource::image_slots)), at most
    /// [`EffectSource::MAX_IMAGE_SLOTS`](crate::EffectSource::MAX_IMAGE_SLOTS), which the build
    /// holds. The effect reads them as `effect_image0` and on. An image not on the device, as
    /// after a loss before the app uploads it again, paints nothing, as an [`ImagePaint`](crate::ImagePaint) does.
    pub fn images<const N: usize>(mut self, images: [ImageHandle; N]) -> Self {
        const {
            assert!(
                N <= MAX_IMAGE_SLOTS as usize,
                "an effect binds at most 4 images"
            );
        }
        self.images = [const { None }; MAX_IMAGE_SLOTS as usize];
        for (slot, image) in self.images.iter_mut().zip(images) {
            *slot = Some(image);
        }
        self
    }
}

impl<U: EffectParams> EffectPaint<U, ColorEffect> {
    /// Paint outside the shape alone, out to `reach` past its edge: a halo, as a glow or a ring
    /// around a card, which runs the effect nowhere under the shape. The effect still reads the
    /// shape's own rect and corners, so `EffectInput::pos` lies outside `0..size`, and its
    /// distance from the shape is the rounded rect's of `size` and `corners`; it should fade to
    /// nothing by `reach`, past which the halo stops. Palantir cuts the shape's edge, antialiased,
    /// whatever the effect returns there, as CSS draws an outset `box-shadow` outside the border
    /// box alone. A rect or a circle; a triangle's halo panics, as its effect reads no corners.
    ///
    /// # Panics
    ///
    /// Panics unless `reach` is a [length](crate::widget::domain::length).
    #[track_caller]
    pub const fn outside(mut self, reach: f32) -> Self {
        self.outside = Some(domain::length(reach));
        self
    }
}

impl<U: EffectParams> EffectPaint<U, BackdropEffect> {
    /// Blur what the effect reads by a Gaussian of σ `sigma`; 0, the default, reads it sharp. As
    /// [`Material::with_backdrop_blur`](crate::Material::with_backdrop_blur) names it.
    ///
    /// # Panics
    ///
    /// Panics unless `sigma` is a [length](crate::widget::domain::length).
    #[track_caller]
    pub const fn backdrop_blur(mut self, sigma: f32) -> Self {
        self.backdrop_blur = domain::length(sigma);
        self
    }

    /// Paint `fallback` over the shape in the effect's place while transparency is reduced, in
    /// place of the theme's, as [`Material::with_fallback`](crate::Material::with_fallback) does;
    /// [`Brush::TRANSPARENT`] paints nothing then.
    pub fn fallback(mut self, fallback: impl Into<Brush>) -> Self {
        self.fallback = BackdropFallback::Brush(fallback.into());
        self
    }
}

impl<U: EffectParams, K: EffectKind> fmt::Debug for EffectPaint<U, K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EffectPaint")
            .field("name", &self.name)
            .field("backdrop_blur", &self.backdrop_blur)
            .field("fallback", &self.fallback)
            .field("images", &self.images)
            .field("outside", &self.outside)
            .finish_non_exhaustive()
    }
}

impl<U: EffectParams, K: EffectKind> PaintedBy<EffectPaint<U, K>> for RectShape {}

/// An effect over a rect, which bounds it by its corners.
impl<U: EffectParams, K: EffectKind> sealed::DrawWith<EffectPaint<U, K>> for RectShape {
    type Prepared = EffectPaint<U, K>;

    fn prepare(paint: EffectPaint<U, K>) -> EffectPaint<U, K> {
        paint
    }

    fn check(&self, _paint: &EffectPaint<U, K>) {}

    fn draw<'a>(&'a self, paint: &'a EffectPaint<U, K>) -> Draw<'a> {
        Draw::Closed {
            geometry: ClosedGeometry::Box(self.geometry()),
            paint: ClosedPaintRef::Effect(paint.erased()),
        }
    }
}

impl<U: EffectParams, K: EffectKind> PaintedBy<EffectPaint<U, K>> for CircleShape {}

/// An effect over a circle: its `EffectInput` holds the circle's square, its corners each half its
/// side.
impl<U: EffectParams, K: EffectKind> sealed::DrawWith<EffectPaint<U, K>> for CircleShape {
    type Prepared = EffectPaint<U, K>;

    fn prepare(paint: EffectPaint<U, K>) -> EffectPaint<U, K> {
        paint
    }

    fn check(&self, _paint: &EffectPaint<U, K>) {}

    fn draw<'a>(&'a self, paint: &'a EffectPaint<U, K>) -> Draw<'a> {
        Draw::Closed {
            geometry: ClosedGeometry::Box(self.geometry()),
            paint: ClosedPaintRef::Effect(paint.erased()),
        }
    }
}

impl<U: EffectParams, K: EffectKind> PaintedBy<EffectPaint<U, K>> for TriangleShape {}

/// An effect over a triangle: its `EffectInput` holds the triangle's covering rect, with no corners,
/// and palantir clips the effect to the triangle.
impl<U: EffectParams, K: EffectKind> sealed::DrawWith<EffectPaint<U, K>> for TriangleShape {
    type Prepared = EffectPaint<U, K>;

    fn prepare(paint: EffectPaint<U, K>) -> EffectPaint<U, K> {
        paint
    }

    /// Unless the paint paints over the triangle: its halo would need corners its effect reads
    /// none of.
    #[track_caller]
    fn check(&self, paint: &EffectPaint<U, K>) {
        assert!(paint.outside.is_none(), "a triangle's effect has no halo");
    }

    fn draw<'a>(&'a self, paint: &'a EffectPaint<U, K>) -> Draw<'a> {
        Draw::Closed {
            geometry: ClosedGeometry::Triangle(self.geometry()),
            paint: ClosedPaintRef::Effect(paint.erased()),
        }
    }
}

impl<U: EffectParams, K: EffectKind> EffectPaint<U, K> {
    /// This paint with its types erased: its name typed at run time, its parameters as bytes.
    fn erased(&self) -> ErasedEffectPaint<'_> {
        let Self {
            name,
            params,
            backdrop_blur,
            fallback,
            images,
            outside,
        } = self;
        ErasedEffectPaint {
            name: name.checked(),
            params: bytemuck::bytes_of(params),
            backdrop_blur: *backdrop_blur,
            fallback: K::fallback(fallback),
            images,
            outside: *outside,
        }
    }
}

/// An [`EffectPaint`] with its parameter type and kind erased, as a [`Draw`] holds it, so its
/// lowering compiles once.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ErasedEffectPaint<'a> {
    pub(crate) name: CheckedEffectName,
    pub(crate) params: &'a [u8],
    /// A backdrop effect's σ, logical px; 0 for a color effect.
    pub(crate) backdrop_blur: f32,
    /// A backdrop effect's fallback; `None` for a color effect, which reads nothing under it.
    pub(crate) fallback: Option<&'a BackdropFallback>,
    pub(crate) images: &'a [Option<ImageHandle>; MAX_IMAGE_SLOTS as usize],
    /// A halo's reach, logical px; `None` paints over the shape.
    pub(crate) outside: Option<f32>,
}
