//! The effect builder. Lowers to `ShapeRecord::Effect`.

use std::fmt;
use std::hash::Hasher as _;

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::effect::MAX_IMAGE_SLOTS;
use crate::effect::effect_kind::ColorEffect;
use crate::effect::effect_name::EffectName;
use crate::effect::effect_params::EffectParams;
use crate::effect::effect_registry::EffectRegistry;
use crate::image_registry::image_handle::ImageHandle;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::effect_id::EffectId;
use crate::primitives::reduction::Reduction;
use crate::shape::record::ShapeRecord;
use crate::shape::record_store::RecordStore;
use crate::shape::sealed;

/// An app-written color effect over a rounded rect, with one instance of its parameters. Paints in
/// record order, as any shape does, and its rect may exceed its node, as a ring does; clipping
/// ancestors clip it. Refused: a `Brush` variant, which every brush family would have to draw; a
/// `Chrome` field, which no ring past the rect fits.
///
/// Its time comes from a [`PaintAnimation`](crate::widget::PaintAnimation) through
/// [`Ui::add_shape_animated`](crate::Ui::add_shape_animated), [`PaintAnimation::clock`] when only
/// the time moves; a turn on it panics, as the coverage is computed in the unturned rect. While
/// power is reduced ([`Ui::is_power_reduced`](crate::Ui::is_power_reduced)) it paints the
/// alternative its source names ([`PowerSaverPaint`](crate::PowerSaverPaint)).
///
/// [`PaintAnimation::clock`]: crate::widget::PaintAnimation::clock
#[must_use]
pub struct EffectShape<U: EffectParams> {
    name: EffectName<U, ColorEffect>,
    params: U,
    local_rect: Option<Rect>,
    corners: Corners,
    /// The images bound, in slot order, each slot past the last `None`.
    images: [Option<ImageHandle>; MAX_IMAGE_SLOTS as usize],
    /// What `Ui::add_shape` finds the name paints.
    paint: EffectPaint,
}

/// What an [`EffectShape`] paints, which only the resolve before it lowers knows.
#[derive(Clone, Copy, Debug)]
enum EffectPaint {
    Unresolved,
    Effect(EffectId),
    /// The power-saver alternative is nothing.
    Nothing,
}

impl<U: EffectParams> EffectShape<U> {
    pub(super) const fn new(name: EffectName<U, ColorEffect>, params: U) -> Self {
        Self {
            name,
            params,
            local_rect: None,
            corners: Corners::ZERO,
            images: [const { None }; MAX_IMAGE_SLOTS as usize],
            paint: EffectPaint::Unresolved,
        }
    }

    /// Bind `images` to the effect's slots, in order: as many as its source declares
    /// ([`EffectSource::image_slots`](crate::EffectSource::image_slots)), at most
    /// [`EffectSource::MAX_IMAGE_SLOTS`](crate::EffectSource::MAX_IMAGE_SLOTS), which the build
    /// holds. The effect reads them as `effect_image0` and on. An image not on the device, as
    /// after a loss before the app uploads it again, paints nothing, as an image shape does.
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

    /// Paint into `rect` in owner-relative coords instead of the owner's arranged rect; it may
    /// reach past the owner.
    ///
    /// # Panics
    ///
    /// Panics unless every component of `rect` is an [offset](crate::widget::domain::offset).
    #[track_caller]
    pub const fn at(mut self, rect: Rect) -> Self {
        rect.validate();
        self.local_rect = Some(rect);
        self
    }

    /// Round the rect's corners, which bound the effect's coverage.
    ///
    /// # Panics
    ///
    /// Panics unless each radius is a [length](crate::widget::domain::length).
    #[track_caller]
    pub fn corners(mut self, corners: impl Into<Corners>) -> Self {
        let corners = corners.into();
        corners.validate();
        self.corners = corners;
        self
    }
}

impl<U: EffectParams> fmt::Debug for EffectShape<U> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EffectShape")
            .field("name", &self.name)
            .field("local_rect", &self.local_rect)
            .field("corners", &self.corners)
            .field("images", &self.images)
            .finish_non_exhaustive()
    }
}

impl<U: EffectParams> sealed::LowerShape for EffectShape<U> {
    const READS_TIME: bool = true;
    const TURNS: bool = false;

    fn is_noop(&self) -> bool {
        matches!(self.paint, EffectPaint::Nothing)
            || self.local_rect.is_some_and(Rect::is_paint_empty)
    }

    #[track_caller]
    fn resolve(&mut self, effects: &EffectRegistry, power: Reduction) {
        self.paint = match effects.resolve(self.name, power) {
            Some(resolved) => {
                let bound = self.images.iter().flatten().count();
                assert!(
                    bound == usize::from(resolved.image_slots),
                    "`{}` paints an effect that binds {} images, and the shape binds {bound}",
                    self.name.name(),
                    resolved.image_slots,
                );
                EffectPaint::Effect(resolved.id)
            }
            None => EffectPaint::Nothing,
        };
    }

    fn lower(self, store: &mut RecordStore) -> ShapeRecord {
        let Self {
            name: _,
            params,
            local_rect,
            corners,
            images,
            paint,
        } = self;
        let EffectPaint::Effect(effect) = paint else {
            unreachable!(
                "`Ui::add_shape` resolves an effect's name before it lowers, and nothing is a noop: {paint:?}"
            );
        };
        let bytes = bytemuck::bytes_of(&params);
        let mut h = Hasher::new();
        h.write_u32(effect.0);
        h.pod_slice(bytes);
        // Each image by its id and generation, so an update repaints as an image shape's does.
        for image in images.iter().flatten() {
            h.write_u64(image.id().0);
            h.write_u32(image.generation());
        }
        ShapeRecord::Effect {
            local_rect,
            corners,
            effect,
            params: store.stage_effect_params(bytes),
            images: store.stage_effect_images(images.iter().flatten()),
            content_hash: ContentHash::finish(&h),
        }
    }
}
