//! The frame a damage test drives, and the colours it paints with.

use crate::Ui;
use crate::display::Display;
use crate::display::user_scale::UserScale;
use crate::primitives::background::Background;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{color::RgbaF32, rect::Rect};
use crate::scene::damage::Damage;
use crate::scene::node::configure::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::{frame::Frame, panel::Panel};
use glam::UVec2;

pub(super) const DISPLAY: Display = Display {
    physical: UVec2::new(200, 200),
    system_scale: 1.0,
    user_scale: UserScale::ONE,
    pixel_snap: true,
    refresh_millihertz: None,
};

/// Drive one frame through the real `Ui::record` path, simulate a
/// successful `WgpuBackend::submit` so the next frame's auto-rewind
/// doesn't fire, and return the damage decision for the just-completed
/// frame. Test sites that care about the damage shape bind the return;
/// the rest ignore it.
pub(super) fn frame(h: &mut UiHarness, f: impl FnMut(&mut Ui)) -> Option<Damage> {
    h.frame(f).plan.map(|plan| plan.damage)
}

/// The standard "root with one 50×50 frame" tree used by most damage
/// tests. RgbaF32 flips between frames to drive minimal authoring
/// changes.
pub(super) const BLUE: RgbaF32 = RgbaF32::srgb(0.2, 0.4, 0.8);

pub(super) const RED: RgbaF32 = RgbaF32::srgb(0.9, 0.4, 0.8);

pub(super) fn one_frame(ui: &mut Ui, color: RgbaF32) {
    Panel::hstack()
        .id(WidgetId::from_hash("root"))
        .show(ui, |ui| {
            Frame::new()
                .id(WidgetId::from_hash("a"))
                .size(50.0)
                .background(Background {
                    fill: color.into(),
                    ..Default::default()
                })
                .show(ui);
        });
}

pub(super) const TEST_SURFACE: Rect = Rect::new(0.0, 0.0, 100.0, 100.0);
