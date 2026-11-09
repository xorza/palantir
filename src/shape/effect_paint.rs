//! [`EffectPaint`]: an effect of either kind over a rect. Lowers to `ShapeRecord::Effect`.

use std::fmt;
use std::hash::Hasher as _;

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::effect::MAX_IMAGE_SLOTS;
use crate::image_registry::image_handle::ImageHandle;
use crate::primitives::effect::effect_kind::{BackdropEffect, ColorEffect, EffectKind, KindId};
use crate::primitives::effect::effect_name::EffectName;
use crate::primitives::effect::effect_params::EffectParams;
use crate::primitives::geometry::coverage::Coverage;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::domain;
use crate::primitives::paint::material::reduced_backdrop::ReducedBackdrop;
use crate::shape::record::{EffectBackdrop, EffectPane, ShapeRecord};
use crate::shape::record_store::RecordStore;
use crate::shape::rect::RectShape;
use crate::shape::sealed::{self, Resolve};

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
/// [`Ui::add_shape_animated`](crate::Ui::add_shape_animated), [`PaintAnimation::clock`] when only
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
    pane: EffectPane,
    /// What a backdrop effect paints while transparency is reduced; a color effect reads none.
    reduced_transparency: ReducedBackdrop,
    /// The images bound, in slot order, each slot past the last `None`.
    images: [Option<ImageHandle>; MAX_IMAGE_SLOTS as usize],
}

/// What a backdrop effect reads this record; a color effect reads neither.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reads {
    /// What is painted under it, captured.
    Capture,
    /// The window's ground, while transparency is reduced.
    Ground,
}

impl<U: EffectParams, K: EffectKind> EffectPaint<U, K> {
    /// The effect `name` with `params`, its kind the name's.
    pub const fn new(name: EffectName<U, K>, params: U) -> Self {
        Self {
            name,
            params,
            pane: EffectPane::ZERO,
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
        self.pane.sigma = domain::length(sigma);
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
        self.pane.reach = domain::length(px);
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
            .field("pane", &self.pane)
            .field("reduced_transparency", &self.reduced_transparency)
            .field("images", &self.images)
            .finish_non_exhaustive()
    }
}

/// An effect over a rect, which bounds it by its corners and covers its inside.
impl<U: EffectParams, K: EffectKind> sealed::LowerShape for (RectShape, EffectPaint<U, K>) {
    const CAN_BE_PANE: bool = matches!(K::ID, KindId::Backdrop);
    const READS_TIME: bool = true;

    /// # Panics
    ///
    /// Unless the rect covers its inside, which an effect's pipeline draws.
    #[track_caller]
    fn validate(&self) {
        assert!(
            self.0.coverage == Coverage::Inside,
            "an effect paints the inside of its rect"
        );
    }

    #[track_caller]
    fn lower(self, resolve: Resolve<'_>, store: &mut RecordStore) -> Option<ShapeRecord> {
        let (
            RectShape {
                local_rect,
                corners,
                coverage: _,
            },
            EffectPaint {
                name,
                params,
                pane,
                reduced_transparency,
                images,
            },
        ) = self;
        let reduced =
            matches!(K::ID, KindId::Backdrop) && resolve.reductions.transparency.is_reduced();
        // Resolved and checked in every mode and before the empty test, so a logic error panics
        // whatever is reduced and wherever it lies.
        let resolved = resolve.effects.resolve(name, resolve.reductions)?;
        let bound = images.iter().flatten().count();
        assert!(
            bound == usize::from(resolved.image_slots),
            "`{}` paints an effect that binds {} images, and the paint binds {bound}",
            name.name(),
            resolved.image_slots,
        );
        if local_rect.is_some_and(Rect::is_paint_empty)
            || (reduced && reduced_transparency == ReducedBackdrop::Nothing)
        {
            return None;
        }
        let reads = if reduced {
            Reads::Ground
        } else {
            Reads::Capture
        };
        let effect = resolved.id;
        let bytes = bytemuck::bytes_of(&params);
        let mut h = Hasher::new();
        h.write_u32(effect.0);
        h.pod_slice(bytes);
        // Each image by its id and generation, so an update repaints as an image's does.
        for image in images.iter().flatten() {
            h.write_u64(image.id().0);
            h.write_u32(image.generation());
        }
        let backdrop = matches!(K::ID, KindId::Backdrop).then_some(match reads {
            Reads::Capture => EffectBackdrop::Capture(pane),
            Reads::Ground => EffectBackdrop::Ground,
        });
        match backdrop {
            Some(EffectBackdrop::Capture(pane)) => {
                h.write_u8(1);
                h.write_u32(pane.sigma.to_bits());
                h.write_u32(pane.reach.to_bits());
            }
            Some(EffectBackdrop::Ground) => h.write_u8(2),
            None => {}
        }
        Some(ShapeRecord::Effect {
            backdrop,
            local_rect,
            corners,
            effect,
            params: store.stage_effect_params(bytes),
            images: store.stage_effect_images(images.iter().flatten()),
            content_hash: ContentHash::finish(&h),
        })
    }
}
