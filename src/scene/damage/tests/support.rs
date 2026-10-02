//! The frame a damage test drives, and the colours it paints with.

use crate::Ui;
use crate::display::Display;
use crate::display::user_scale::UserScale;
use crate::primitives::background::Background;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{color::RgbaF32, rect::Rect};
use crate::scene::damage::Damage;
use crate::ui::harness::UiHarness;
use crate::widgets::configure::Configure;
use crate::widgets::{block::Block, panel::Panel};
use glam::UVec2;

pub(super) const DISPLAY: Display = Display {
    physical: UVec2::new(200, 200),
    system_scale: 1.0,
    user_scale: UserScale::ONE,
    pixel_snap: true,
    refresh_millihertz: None,
};

/// Run one frame of `f` and return its damage, or `None` when the frame
/// skips. The frame is told its previous output is valid, as a host
/// tells it after a present, so the damage is incremental against it.
pub(super) fn frame(h: &mut UiHarness, f: impl FnMut(&mut Ui)) -> Option<Damage> {
    h.frame(f).plan.map(|plan| plan.damage)
}

/// [`frame`] told its previous output is lost, as after a failed
/// present: the damage starts over from nothing.
pub(super) fn frame_without_baseline(h: &mut UiHarness, f: impl FnMut(&mut Ui)) -> Option<Damage> {
    h.frame_without_baseline(f).plan.map(|plan| plan.damage)
}

/// The two fills [`one_frame`] flips between to drive a minimal authoring
/// change.
pub(super) const BLUE: RgbaF32 = RgbaF32::srgb(0.2, 0.4, 0.8);

pub(super) const RED: RgbaF32 = RgbaF32::srgb(0.9, 0.4, 0.8);

/// The standard "root with one 50×50 frame" tree most damage tests use,
/// its frame filled with `color`.
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

/// A surface for the region-arithmetic tests that build rects by hand
/// rather than through a frame, and so do not draw on [`DISPLAY`]. A
/// frame test clamps to `DISPLAY.logical_rect()` instead.
pub(super) const TEST_SURFACE: Rect = Rect::new(0.0, 0.0, 100.0, 100.0);
