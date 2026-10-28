//! The identity of a backdrop pane across frames.

use crate::primitives::identity::widget_id::WidgetId;

/// A pane across frames: its owner and the shape's ordinal among the owner's shapes, which stays
/// put when other widgets add shapes. A pane's cached blur keys on it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct PaneKey {
    pub(crate) owner: WidgetId,
    pub(crate) shape: u32,
}
