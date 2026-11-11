//! [`EffectPaint`]: an effect of either kind over a rect, a circle or a triangle. Lowers to
//! a `ShapeRecord::Closed` of `ClosedPaint::Effect`.

use std::fmt;

use crate::effect::MAX_IMAGE_SLOTS;
use crate::image_registry::image_handle::ImageHandle;
use crate::primitives::effect::effect_kind::{BackdropEffect, ColorEffect, EffectKind, KindId};
use crate::primitives::effect::effect_name::{EffectName, TypedName};
use crate::primitives::effect::effect_params::EffectParams;
use crate::primitives::math::domain;
use crate::primitives::paint::material::reduced_backdrop::ReducedBackdrop;
use crate::shape::PaintedBy;
use crate::shape::circle::CircleShape;
use crate::shape::draw::{ClosedPaintRef, Draw};
use crate::shape::geometry::closed_geometry::ClosedGeometry;
use crate::shape::paint::capture_params::{CaptureParams, CaptureReach};
use crate::shape::rect::RectShape;
use crate::shape::sealed;
use crate::shape::triangle::TriangleShape;

/// An app-written effect over a [`RectShape`], a [`CircleShape`] or a [`TriangleShape`]
/// ([`Ui::add_shape`](crate::Ui::add_shape)), with one instance of its parameters: a color
/// effect ([`Self::new`] of a color effect's name), or a backdrop effect, which also reads what is
/// painted under it. Paints in record order, as any paint does, and its rect may exceed its node,
/// as a ring does; clipping ancestors clip it. A [`Material`](crate::Material) is the theme form of
/// a backdrop effect, which a box wears under its fill and border, and draws as an `EffectPaint`
/// does. Refused: a `Brush` variant, which every brush family would have to draw, and which a
/// theme file saves where no parameters fit.
///
/// A backdrop effect is a backdrop capture: what is painted under it is copied out of the frame at
/// its place in paint order, blurred by its [σ](Self::blur), and kept across frames until
/// something under its [reach](Self::reach) changes, as a material's is.
///
/// Its time comes from a [`PaintAnimation`](crate::widget::PaintAnimation) through
/// [`AddedShape::animate`](crate::widget::AddedShape::animate), [`PaintAnimation::clock`] when only
/// the time moves; a turn turns it about its owner's center, as a quad turns. While motion or
/// power use is reduced it paints the alternative its source names for it
/// ([`ReducedPaint`](crate::ReducedPaint)); while transparency is reduced, a backdrop effect does
/// what its [`reduced_transparency`](Self::reduced_transparency) says.
///
/// [`PaintAnimation::clock`]: crate::widget::PaintAnimation::clock
#[must_use]
pub struct EffectPaint<U: EffectParams, K: EffectKind = ColorEffect> {
    name: EffectName<U, K>,
    params: U,
    /// What a backdrop effect reads under it; zero for a color effect, which reads none.
    capture: CaptureParams,
    /// What a backdrop effect paints while transparency is reduced; a color effect reads none.
    reduced_transparency: ReducedBackdrop,
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
            capture: CaptureParams::ZERO,
            reduced_transparency: ReducedBackdrop::Ground,
            images: [const { None }; MAX_IMAGE_SLOTS as usize],
            outside: None,
        }
    }

    /// Bind `images` to the effect's slots, in order: as many as its source declares
    /// ([`EffectSource::image_slots`](crate::EffectSource::image_slots)), at most
    /// [`EffectSource::MAX_IMAGE_SLOTS`](crate::EffectSource::MAX_IMAGE_SLOTS), which the build
    /// holds. The effect reads them as `effect_image0` and on. An image not on the device, as
    /// after a loss before the app uploads it again, paints nothing, as an [`ImagePaint`](crate::widget::ImagePaint) does.
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
    /// Blur what the effect reads by a Gaussian of σ `sigma`; 0, the default, reads it sharp.
    ///
    /// # Panics
    ///
    /// Panics unless `sigma` is a [length](crate::widget::domain::length).
    #[track_caller]
    pub const fn blur(mut self, sigma: f32) -> Self {
        self.capture.sigma = domain::length(sigma);
        self
    }

    /// Read up to `px` past the effect's rect, as a ring's glow or a lens's bent ray does
    /// (SwiftUI's `maxSampleOffset`), rounded out to whole physical px; a read past it reads its
    /// edge. 0 by default: the effect reads under its own rect alone. The capture grows by it, so
    /// damage under it repaints.
    /// Refused: one bound per effect, which its largest shape sets for all; the whole surface, as
    /// Flutter reads without a `ClipRect`, which captures and blurs what no shape reads.
    ///
    /// # Panics
    ///
    /// Panics unless `px` is a [length](crate::widget::domain::length).
    #[track_caller]
    pub const fn reach(mut self, px: f32) -> Self {
        self.capture.reach = CaptureReach::Px(domain::length(px));
        self
    }

    /// What the effect paints while transparency is reduced: by default it reads the window's
    /// ground in place of what is painted under it ([`ReducedBackdrop::Ground`]), or it paints
    /// nothing ([`ReducedBackdrop::Nothing`]). Neither captures or blurs.
    pub const fn reduced_transparency(mut self, backdrop: ReducedBackdrop) -> Self {
        self.reduced_transparency = backdrop;
        self
    }
}

impl<U: EffectParams, K: EffectKind> fmt::Debug for EffectPaint<U, K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EffectPaint")
            .field("name", &self.name)
            .field("capture", &self.capture)
            .field("reduced_transparency", &self.reduced_transparency)
            .field("images", &self.images)
            .field("outside", &self.outside)
            .finish_non_exhaustive()
    }
}

impl<U: EffectParams, K: EffectKind> PaintedBy<EffectPaint<U, K>> for RectShape {}

/// An effect over a rect, which bounds it by its corners.
impl<U: EffectParams, K: EffectKind> sealed::DrawWith<EffectPaint<U, K>> for RectShape {
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
            capture,
            reduced_transparency,
            images,
            outside,
        } = self;
        ErasedEffectPaint {
            name: name.typed(),
            kind: K::ID,
            params: bytemuck::bytes_of(params),
            capture: *capture,
            reduced_transparency: *reduced_transparency,
            images,
            outside: *outside,
        }
    }
}

/// An [`EffectPaint`] with its parameter type and kind erased, as a [`Draw`] holds it, so its
/// lowering compiles once.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ErasedEffectPaint<'a> {
    pub(crate) name: TypedName,
    pub(crate) kind: KindId,
    pub(crate) params: &'a [u8],
    pub(crate) capture: CaptureParams,
    pub(crate) reduced_transparency: ReducedBackdrop,
    pub(crate) images: &'a [Option<ImageHandle>; MAX_IMAGE_SLOTS as usize],
    /// A halo's reach, logical px; `None` paints over the shape.
    pub(crate) outside: Option<f32>,
}
