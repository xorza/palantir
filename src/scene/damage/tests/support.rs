//! The frame a damage test drives, and the colours it paints with.

use crate::Ui;
use crate::display::Display;
use crate::primitives::background::Background;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{color::Color, rect::Rect};
use crate::renderer::render_plan::{RenderKind, RenderPlan};
use crate::scene::damage::Damage;
use crate::scene::node::configure::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::{frame::Frame, panel::Panel};
use glam::UVec2;

pub(super) const DISPLAY: Display = Display {
    physical: UVec2::new(200, 200),
    scale_factor: 1.0,
    pixel_snap: true,
    refresh_millihertz: None,
};

/// Drive one frame through the real `Ui::record` path, simulate a
/// successful `WgpuBackend::submit` so the next frame's auto-rewind
/// doesn't fire, and return the damage decision for the just-completed
/// frame. Test sites that care about the damage shape bind the return;
/// the rest ignore it.
pub(super) fn frame(h: &mut UiHarness, f: impl FnMut(&mut Ui)) -> Damage {
    let report = h.frame(f);
    match report.plan {
        None => Damage::Skip,
        Some(RenderPlan {
            kind: RenderKind::Full,
            ..
        }) => Damage::Full,
        Some(RenderPlan {
            kind: RenderKind::Partial { damage },
            ..
        }) => Damage::Partial(damage),
    }
}

/// The standard "root with one 50×50 frame" tree used by most damage
/// tests. Color flips between frames to drive minimal authoring
/// changes.
pub(super) const BLUE: Color = Color::rgb(0.2, 0.4, 0.8);

pub(super) const RED: Color = Color::rgb(0.9, 0.4, 0.8);

pub(super) fn one_frame(ui: &mut Ui, color: Color) {
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
