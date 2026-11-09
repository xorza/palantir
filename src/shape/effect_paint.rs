//! [`EffectPaint`]: an effect of either kind over a rect. Lowers to `ShapeRecord::Effect`.

use std::fmt;

use crate::effect::MAX_IMAGE_SLOTS;
use crate::image_registry::image_handle::ImageHandle;
use crate::primitives::effect::effect_kind::{BackdropEffect, ColorEffect, EffectKind, KindId};
use crate::primitives::effect::effect_name::{EffectName, TypedName};
use crate::primitives::effect::effect_params::EffectParams;
use crate::primitives::geometry::coverage::Coverage;
use crate::primitives::math::domain;
use crate::primitives::paint::material::reduced_backdrop::ReducedBackdrop;
use crate::shape::PaintedBy;
use crate::shape::draw::Draw;
use crate::shape::record::EffectCapture;
use crate::shape::rect::RectShape;
use crate::shape::sealed;

/// An app-written effect over a [`RectShape`] ([`Ui::add_shape`](crate::Ui::add_shape)), with one
/// instance of its parameters: a color effect ([`Self::new`] of a color effect's name), or a
/// backdrop effect, which also reads what is painted under it. Paints in record order, as any
/// paint does, and its rect may exceed its node, as a ring does; clipping ancestors clip it.
/// Refused: a `Brush` variant, which every brush family would have to draw, and which a theme
/// file saves where no parameters fit: a box's backdrop effect is its [`Material`](crate::Material).
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
    capture: EffectCapture,
    /// What a backdrop effect paints while transparency is reduced; a color effect reads none.
    reduced_transparency: ReducedBackdrop,
    /// The images bound, in slot order, each slot past the last `None`.
    images: [Option<ImageHandle>; MAX_IMAGE_SLOTS as usize],
}

impl<U: EffectParams, K: EffectKind> EffectPaint<U, K> {
    /// The effect `name` with `params`, its kind the name's.
    pub const fn new(name: EffectName<U, K>, params: U) -> Self {
        Self {
            name,
            params,
            capture: EffectCapture::ZERO,
            reduced_transparency: ReducedBackdrop::Ground,
            images: [const { None }; MAX_IMAGE_SLOTS as usize],
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
    /// (SwiftUI's `maxSampleOffset`); a read past it reads its edge. 0 by default: the effect
    /// reads under its own rect alone. The capture grows by it, so damage under it repaints.
    /// Refused: one bound per effect, which its largest shape sets for all; the whole surface, as
    /// Flutter reads without a `ClipRect`, which captures and blurs what no shape reads.
    ///
    /// # Panics
    ///
    /// Panics unless `px` is a [length](crate::widget::domain::length).
    #[track_caller]
    pub const fn reach(mut self, px: f32) -> Self {
        self.capture.reach = domain::length(px);
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
            .finish_non_exhaustive()
    }
}

impl<U: EffectParams, K: EffectKind> PaintedBy<EffectPaint<U, K>> for RectShape {}

/// An effect over a rect, which bounds it by its corners and covers its inside.
impl<U: EffectParams, K: EffectKind> sealed::DrawWith<EffectPaint<U, K>> for RectShape {
    /// # Panics
    ///
    /// Unless the rect covers its inside, which an effect's pipeline draws.
    #[track_caller]
    fn check(&self, _paint: &EffectPaint<U, K>) {
        assert!(
            self.coverage == Coverage::Inside,
            "an effect paints the inside of its rect"
        );
    }

    fn draw<'a>(&'a self, paint: &'a EffectPaint<U, K>) -> Draw<'a> {
        Draw::Effect {
            rect: self,
            paint: paint.erased(),
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
        } = self;
        ErasedEffectPaint {
            name: name.typed(),
            kind: K::ID,
            params: bytemuck::bytes_of(params),
            capture: *capture,
            reduced_transparency: *reduced_transparency,
            images,
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
    pub(crate) capture: EffectCapture,
    pub(crate) reduced_transparency: ReducedBackdrop,
    pub(crate) images: &'a [Option<ImageHandle>; MAX_IMAGE_SLOTS as usize],
}
