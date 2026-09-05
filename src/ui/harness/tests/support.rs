//! The one button a harness test drives, and the positions on and off it.

use crate::layout::types::sizing::Sizing;
use crate::ui::harness::*;
use crate::widgets::button::Button;
use crate::widgets::configure::Configure;
use crate::widgets::panel::Panel;

pub(super) const SURFACE: UVec2 = UVec2::new(200, 120);

pub(super) fn target() -> WidgetId {
    WidgetId::from_hash("harness-target")
}

/// A 100×40 button at the surface origin, so a press at (50, 20) lands
/// inside it and (150, 100) does not.
pub(super) fn button(ui: &mut Ui) {
    Panel::hstack().auto_id().show(ui, |ui| {
        Button::new()
            .id(target())
            .label("x")
            .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
            .show(ui);
    });
}

pub(super) const INSIDE: Vec2 = Vec2::new(50.0, 20.0);

pub(super) const OUTSIDE: Vec2 = Vec2::new(150.0, 100.0);
