//! The effect builder. Lowers to `ShapeRecord::Effect`.

use std::fmt;
use std::hash::Hasher as _;

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::effect::effect_kind::ColorEffect;
use crate::effect::effect_name::EffectName;
use crate::effect::effect_params::EffectParams;
use crate::effect::effect_registry::EffectRegistry;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::effect_id::EffectId;
use crate::shape::record::ShapeRecord;
use crate::shape::record_store::RecordStore;
use crate::shape::sealed;

/// An app-written color effect over a rounded rect, with one instance of its parameters. Paints in
/// record order, as any shape does, and its rect may exceed its node, as a ring does; clipping
/// ancestors clip it. Refused: a `Brush` variant, which every brush family would have to draw; a
/// `Chrome` field, which no ring past the rect fits.
#[must_use]
pub struct EffectShape<U: EffectParams> {
    name: EffectName<U, ColorEffect>,
    params: U,
    local_rect: Option<Rect>,
    corners: Corners,
    /// The id `Ui::add_shape` finds the name loaded under.
    id: Option<EffectId>,
}

impl<U: EffectParams> EffectShape<U> {
    pub(super) const fn new(name: EffectName<U, ColorEffect>, params: U) -> Self {
        Self {
            name,
            params,
            local_rect: None,
            corners: Corners::ZERO,
            id: None,
        }
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
            .finish_non_exhaustive()
    }
}

impl<U: EffectParams> sealed::LowerShape for EffectShape<U> {
    fn is_noop(&self) -> bool {
        self.local_rect.is_some_and(Rect::is_paint_empty)
    }

    #[track_caller]
    fn resolve(&mut self, effects: &EffectRegistry) {
        self.id = Some(effects.resolve(self.name));
    }

    fn lower(self, store: &mut RecordStore) -> ShapeRecord {
        let Self {
            name: _,
            params,
            local_rect,
            corners,
            id,
        } = self;
        let effect = id.expect("`Ui::add_shape` resolves an effect's name before it lowers");
        let bytes = bytemuck::bytes_of(&params);
        let mut h = Hasher::new();
        h.write_u32(effect.0);
        h.pod_slice(bytes);
        ShapeRecord::Effect {
            local_rect,
            corners,
            effect,
            params: store.stage_effect_params(bytes),
            content_hash: ContentHash::finish(&h),
        }
    }
}
