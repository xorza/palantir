//! The effect builder, of either kind. Lowers to `ShapeRecord::Effect`.

use std::fmt;
use std::hash::Hasher as _;

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::effect::MAX_IMAGE_SLOTS;
use crate::effect::effect_registry::EffectRegistry;
use crate::image_registry::image_handle::ImageHandle;
use crate::primitives::effect::effect_kind::{BackdropEffect, ColorEffect, EffectKind, KindId};
use crate::primitives::effect::effect_name::EffectName;
use crate::primitives::effect::effect_params::EffectParams;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::effect_id::EffectId;
use crate::primitives::math::domain;
use crate::primitives::paint::material::reduced_backdrop::ReducedBackdrop;
use crate::primitives::reduction::Reductions;
use crate::shape::record::{EffectBackdrop, EffectPane, ShapeRecord};
use crate::shape::record_store::RecordStore;
use crate::shape::sealed;

/// An app-written effect over a rounded rect, with one instance of its parameters: a color effect
/// ([`Shape::effect`](crate::widget::Shape::effect)), or a backdrop effect
/// ([`Shape::backdrop_effect`](crate::widget::Shape::backdrop_effect)), which also reads what is
/// painted under it. Paints in record order, as any shape does, and its rect may exceed its node,
/// as a ring does; clipping ancestors clip it. Refused: a `Brush` variant, which every brush family
/// would have to draw; a `Chrome` field, which no ring past the rect fits: a box's backdrop effect
/// is its [`Material`](crate::Material).
///
/// A backdrop effect is a pane: what is painted under it is copied out of the frame at its place
/// in paint order, blurred by its [σ](Self::blur), and kept across frames until something under
/// its [reach](Self::reach) changes, as a material's is.
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
pub struct EffectShape<U: EffectParams, K: EffectKind = ColorEffect> {
    name: EffectName<U, K>,
    params: U,
    local_rect: Option<Rect>,
    corners: Corners,
    /// What a backdrop effect reads under it; zero for a color effect, which reads none.
    pane: EffectPane,
    /// What a backdrop effect paints while transparency is reduced; a color effect reads none.
    reduced_transparency: ReducedBackdrop,
    /// The images bound, in slot order, each slot past the last `None`.
    images: [Option<ImageHandle>; MAX_IMAGE_SLOTS as usize],
    /// What `Ui::add_shape` finds the name paints.
    paint: EffectPaint,
}

/// What an [`EffectShape`] paints, which only the resolve before it lowers knows.
#[derive(Clone, Copy, Debug)]
enum EffectPaint {
    Unresolved,
    Effect(EffectId, Reads),
    /// The alternative it paints for what is reduced is nothing.
    Nothing,
}

/// What a backdrop effect reads this record; a color effect reads neither.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reads {
    /// What is painted under it, captured.
    Capture,
    /// The window's ground, while transparency is reduced.
    Ground,
}

impl<U: EffectParams, K: EffectKind> EffectShape<U, K> {
    pub(super) const fn new(name: EffectName<U, K>, params: U) -> Self {
        Self {
            name,
            params,
            local_rect: None,
            corners: Corners::ZERO,
            pane: EffectPane::ZERO,
            reduced_transparency: ReducedBackdrop::Ground,
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

impl<U: EffectParams> EffectShape<U, BackdropEffect> {
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

impl<U: EffectParams, K: EffectKind> fmt::Debug for EffectShape<U, K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EffectShape")
            .field("name", &self.name)
            .field("local_rect", &self.local_rect)
            .field("corners", &self.corners)
            .field("pane", &self.pane)
            .field("reduced_transparency", &self.reduced_transparency)
            .field("images", &self.images)
            .finish_non_exhaustive()
    }
}

impl<U: EffectParams, K: EffectKind> sealed::LowerShape for EffectShape<U, K> {
    const CAN_BE_PANE: bool = matches!(K::ID, KindId::Backdrop);
    const READS_TIME: bool = true;

    fn is_noop(&self) -> bool {
        matches!(self.paint, EffectPaint::Nothing)
            || self.local_rect.is_some_and(Rect::is_paint_empty)
    }

    #[track_caller]
    fn resolve(&mut self, effects: &EffectRegistry, reductions: Reductions) {
        let reduced = matches!(K::ID, KindId::Backdrop) && reductions.transparency.is_reduced();
        let reads = if reduced {
            Reads::Ground
        } else {
            Reads::Capture
        };
        // Resolved and checked in every mode, so a logic error panics whatever is reduced.
        self.paint = match effects.resolve(self.name, reductions) {
            Some(resolved) => {
                let bound = self.images.iter().flatten().count();
                assert!(
                    bound == usize::from(resolved.image_slots),
                    "`{}` paints an effect that binds {} images, and the shape binds {bound}",
                    self.name.name(),
                    resolved.image_slots,
                );
                if reduced && self.reduced_transparency == ReducedBackdrop::Nothing {
                    EffectPaint::Nothing
                } else {
                    EffectPaint::Effect(resolved.id, reads)
                }
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
            pane,
            reduced_transparency: _,
            images,
            paint,
        } = self;
        let EffectPaint::Effect(effect, reads) = paint else {
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
        ShapeRecord::Effect {
            backdrop,
            local_rect,
            corners,
            effect,
            params: store.stage_effect_params(bytes),
            images: store.stage_effect_images(images.iter().flatten()),
            content_hash: ContentHash::finish(&h),
        }
    }
}
