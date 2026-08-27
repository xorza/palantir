use crate::input::pointer::PointerButton;
use crate::input::response::pointer_edge::PointerEdge;
use crate::primitives::widget_id::WidgetId;

/// One thing the pointer did to one widget this frame.
///
/// The collation half of the input API, against
/// [`Ui::response_for`](crate::Ui::response_for)'s polling
/// half — see [`Ui::pointer_actions`](crate::Ui::pointer_actions) for which to
/// reach for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointerAction {
    /// The widget it happened to.
    pub id: WidgetId,
    pub button: PointerButton,
    pub edge: PointerEdge,
}
