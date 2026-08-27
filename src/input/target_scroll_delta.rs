//! A scroll accumulation bound to the widget it was routed to.

use crate::input::response::scroll_delta::ScrollDelta;
use crate::primitives::widget_id::WidgetId;

/// Scroll accumulated this frame for one routed target. Held per
/// scroll target so events arriving before a retarget stay with the
/// widget that was under the pointer when they landed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TargetScrollDelta {
    pub(crate) target: WidgetId,
    pub(crate) delta: ScrollDelta,
}

impl TargetScrollDelta {
    pub(crate) fn new(target: WidgetId) -> Self {
        Self {
            target,
            delta: ScrollDelta::default(),
        }
    }
}
