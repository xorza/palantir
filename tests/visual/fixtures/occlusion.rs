use glam::UVec2;
use image::RgbaImage;
use palantir::{Configure, Panel, Rect, RgbaF32, Shape, Sizing, Ui};

use crate::harness::Harness;
use palantir::golden::Tolerance;

const VIEWPORT: UVec2 = UVec2::new(128, 128);
const CLEAR: RgbaF32 = RgbaF32::WHITE;
const LAYER_RECT: Rect = Rect::new(20.25, 20.25, 80.0, 80.0);

fn add_layer(ui: &mut Ui, color: RgbaF32) {
    ui.add_shape(Shape::rect(LAYER_RECT).fill(color));
}

fn render_fractional_layers(split_groups: bool) -> RgbaImage {
    let mut harness = Harness::new_with_pixel_snap(false);
    harness.render(VIEWPORT, 1.0, CLEAR, |ui| {
        assert!(!ui.display().pixel_snap);
        Panel::canvas()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                add_layer(ui, RgbaF32::srgb(1.0, 0.0, 0.0));
                if split_groups {
                    Panel::canvas()
                        .auto_id()
                        .size((Sizing::FILL, Sizing::FILL))
                        .clip_rect()
                        .show(ui, |ui| add_layer(ui, RgbaF32::srgb(0.0, 0.0, 1.0)));
                } else {
                    add_layer(ui, RgbaF32::srgb(0.0, 0.0, 1.0));
                }
            });
    })
}

#[test]
fn fractional_opaque_quads_match_unpruned_reference() {
    let optimized = render_fractional_layers(false);
    let unpruned = render_fractional_layers(true);
    let report = Tolerance {
        per_channel: 0,
        max_ratio: 0.0,
    }
    .diff(&optimized, &unpruned);
    assert_eq!(
        report.differing_pixels, 0,
        "max channel delta {}, differing ratio {}",
        report.max_channel_delta, report.differing_ratio,
    );
}
