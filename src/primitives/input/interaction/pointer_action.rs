//! One thing the pointer did to one widget this frame, for callers collating edges.

use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::interaction::pointer_edge::PointerEdge;
use crate::primitives::input::pointer::PointerButton;

/// One thing the pointer did to one widget this frame; the collation half against [`Ui::response_for`](crate::Ui::response_for)'s polling half (see [`Ui::pointer_actions`](crate::Ui::pointer_actions)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointerAction {
    /// The widget it happened to.
    pub id: WidgetId,
    /// Which button the edge belongs to.
    pub button: PointerButton,
    /// What happened.
    pub edge: PointerEdge,
}
