//! The frame a damage test drives, and the colours it paints with.

use crate::Ui;
use crate::damage::Damage;
use crate::display::Display;
use crate::display::user_scale::UserScale;
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, panel::Panel};
use glam::UVec2;

pub(super) const DISPLAY: Display = Display {
    physical: UVec2::new(200, 200),
    system_scale: 1.0,
    user_scale: UserScale::ONE,
    pixel_snap: true,
    refresh_millihertz: None,
};

/// Run one frame of `f` and return its damage, or `None` when it skips; the previous output is told valid, so damage is incremental.
pub(super) fn frame(h: &mut UiHarness, f: impl FnMut(&mut Ui)) -> Option<Damage> {
    h.frame(f).plan.map(|plan| plan.damage)
}

/// [`frame`] with the previous output lost, as after a failed present.
pub(super) fn frame_without_baseline(h: &mut UiHarness, f: impl FnMut(&mut Ui)) -> Option<Damage> {
    h.frame_without_baseline(f).plan.map(|plan| plan.damage)
}

pub(super) const BLUE: RgbaF32 = RgbaF32::srgb(0.2, 0.4, 0.8);

pub(super) const RED: RgbaF32 = RgbaF32::srgb(0.9, 0.4, 0.8);

pub(super) fn one_frame(ui: &mut Ui, color: RgbaF32) {
    Panel::hstack()
        .id(WidgetId::from_hash("root"))
        .show(ui, |ui| {
            Block::new()
                .id(WidgetId::from_hash("a"))
                .size(50.0)
                .background(Background::fill(color))
                .show(ui);
        });
}

pub(super) const TEST_SURFACE: Rect = Rect::new(0.0, 0.0, 100.0, 100.0);
