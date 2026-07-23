use aperture::{Color, Configure, Panel, Rect, Shape, Sizing, Ui};
use glam::UVec2;
use image::RgbaImage;

use crate::diff::{Tolerance, diff};
use crate::harness::Harness;

const VIEWPORT: UVec2 = UVec2::new(128, 128);
const CLEAR: Color = Color::WHITE;
const LAYER_RECT: Rect = Rect::new(20.25, 20.25, 80.0, 80.0);

fn add_layer(ui: &mut Ui, color: Color) {
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
                add_layer(ui, Color::rgb(1.0, 0.0, 0.0));
                if split_groups {
                    Panel::canvas()
                        .auto_id()
                        .size((Sizing::FILL, Sizing::FILL))
                        .clip_rect()
                        .show(ui, |ui| add_layer(ui, Color::rgb(0.0, 0.0, 1.0)));
                } else {
                    add_layer(ui, Color::rgb(0.0, 0.0, 1.0));
                }
            });
    })
}

#[test]
fn fractional_opaque_quads_match_unpruned_reference() {
    let optimized = render_fractional_layers(false);
    let unpruned = render_fractional_layers(true);
    let report = diff(
        &optimized,
        &unpruned,
        Tolerance {
            per_channel: 0,
            max_ratio: 0.0,
        },
    );
    assert_eq!(
        report.differing_pixels, 0,
        "max channel delta {}, differing ratio {}",
        report.max_channel_delta, report.differing_ratio,
    );
}
