//! The identity of a backdrop across frames.

use crate::primitives::identity::widget_id::WidgetId;
use std::cmp;

/// A backdrop across frames: its owner and its row among the owner's paint rows, `0` for the
/// owner's chrome, which stays put when other widgets add shapes. A backdrop's cached blur keys on
/// it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct BackdropKey {
    pub(crate) owner: WidgetId,
    pub(crate) shape: u32,
}

/// The fill of `tinyvec`'s empty slots, which no read reaches.
impl Default for BackdropKey {
    fn default() -> Self {
        Self {
            owner: WidgetId::VIEWPORT,
            shape: 0,
        }
    }
}

/// By owner, then by row: an order of its own, so
/// [`BackdropKeys`](crate::primitives::identity::backdrop_keys::BackdropKeys) can search, which
/// `WidgetId`, a public type, does not offer.
impl Ord for BackdropKey {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        (self.owner.as_hash(), self.shape).cmp(&(other.owner.as_hash(), other.shape))
    }
}

impl PartialOrd for BackdropKey {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}
