//! One Tab stop.

use crate::primitives::identity::widget_id::WidgetId;
use crate::scene::layer::Layer;

/// One Tab stop, in record order across every layer. A node is a stop when it
/// is focusable, kept its [`Configure::tab_stop`](crate::Configure::tab_stop),
/// and is neither disabled nor invisible (the rule [`HitRow::focusable`](crate::cascade::rows::hit_row::HitRow::focusable)
/// applies to a press). Same lifecycle as [`ScopeRow`](crate::cascade::rows::scope_row::ScopeRow).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TabStopRow {
    pub(crate) layer: Layer,
    /// The root of the tree the stop was recorded under; what a trap domain is named by.
    pub(crate) root: WidgetId,
    pub(crate) id: WidgetId,
    pub(crate) idx: i16,
}
