//! [`PointPlacement`]: the rect a point shape lays out in, and how its points fill it.

use crate::common::hash::Hasher;
use crate::primitives::geometry::fit::{Fit, FitMap};
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::math::float_hash::FloatHash;
use crate::primitives::math::nan::NanCheck;
use glam::Vec2;
use std::hash::Hasher as _;

/// A point shape's rect, the owner's arranged rect where `None`, and the fit its points take in
/// it; resolved against the owner where the shape is drawn, as the owner's size is known there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PointPlacement {
    pub(crate) local_rect: Option<Rect>,
    pub(crate) fit: Fit,
}

impl PointPlacement {
    /// The owner's rect, its points as given.
    pub(crate) const NATURAL: Self = Self {
        local_rect: None,
        fit: Fit::Natural,
    };

    /// The map that fits points spanning `extent` into the placement's rect in an owner of
    /// `owner_size`, owner-local.
    pub(crate) fn map(self, extent: Rect, owner_size: Size) -> FitMap {
        let rect = self.local_rect.unwrap_or(Rect {
            min: Vec2::ZERO,
            size: owner_size,
        });
        self.fit.map(extent, rect)
    }

    /// Feed the placement into a record's hash.
    pub(crate) fn hash_into(self, h: &mut Hasher) {
        match self.local_rect {
            None => h.write_u8(self.fit as u8),
            Some(rect) => {
                h.write_u8(0x80 | self.fit as u8);
                rect.hash_visual(h);
            }
        }
    }
}

impl NanCheck for PointPlacement {
    fn has_nan(&self) -> bool {
        self.local_rect.has_nan()
    }
}
