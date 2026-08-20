//! Opening the menu and reading its rows back.

use crate::primitives::widget_id::WidgetId;
use glam::UVec2;

pub(super) const SURFACE: UVec2 = UVec2::new(400, 400);

pub(super) fn trigger_id() -> WidgetId {
    WidgetId::from_hash("trigger")
}
