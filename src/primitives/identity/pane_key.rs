//! The identity of a backdrop pane across frames.

use crate::primitives::identity::widget_id::WidgetId;
use std::cmp;

/// A pane across frames: its owner and its row among the owner's paint rows, `0` for the owner's
/// chrome, which stays put when other widgets add shapes. A pane's cached blur keys on it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct PaneKey {
    pub(crate) owner: WidgetId,
    pub(crate) shape: u32,
}

/// By owner, then by row: an order of its own, so [`PaneKeys`](crate::primitives::identity::pane_keys::PaneKeys)
/// can search, which `WidgetId`, a public type, does not offer.
impl Ord for PaneKey {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        (self.owner.0, self.shape).cmp(&(other.owner.0, other.shape))
    }
}

impl PartialOrd for PaneKey {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}
