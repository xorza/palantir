//! The frame group, which every pipeline of the main pass reads at one slot.

use crate::gpu::test_gpu::headless_test_gpu;
use crate::host::offscreen::OffscreenHost;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::Shape;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::text::Text;
use glam::{UVec2, Vec2};

/// A quad, a text run and a polyline replay in that order, through three pipelines: the frame
/// group binds once for the pass and the atlas once past it, where the gradient group at slot 0
/// was bound again after the atlas took the slot. One draw each.
#[test]
fn the_frame_group_binds_once_a_pass() {
    let gpu = headless_test_gpu();
    let mut host = OffscreenHost::builder(gpu.handles()).build();
    let target = gpu.target("palantir.test.frame_group", UVec2::new(64, 32));
    let points = [Vec2::new(0.0, 0.0), Vec2::new(30.0, 20.0)];
    host.frame(target.as_target(), &mut |ui: &mut Ui| {
        ui.add_shape(
            Shape::rect().at(Rect::new(40.0, 4.0, 20.0, 20.0)),
            RgbaF32::BLACK,
        );
        Text::new("Label").id_salt("label").show(ui);
        ui.add_shape(
            Shape::polyline(&points).at(Rect::new(2.0, 2.0, 30.0, 20.0)),
            Stroke::new(RgbaF32::WHITE, 2.0),
        );
    });
    let counts = host.ui().gpu_pass_stats().last_pass_counts();
    assert_eq!(counts.draws, 3, "{counts:?}");
    assert_eq!(counts.pipelines, 3, "{counts:?}");
    assert_eq!(
        counts.groups, 2,
        "the frame group and the atlas: {counts:?}"
    );
}
