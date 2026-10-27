//! One arrow group.

use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::axis::Axis;

/// One arrow group (see [`Configure::arrow_focus`](crate::Configure::arrow_focus))
/// in paint order across every plane. A node in a disabled or invisible
/// subtree is no group, as it is no stop. Same lifecycle as [`ScopeRow`](crate::cascade::rows::scope_row::ScopeRow).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ArrowGroupRow {
    pub(crate) id: WidgetId,
    pub(crate) axis: Axis,
}
