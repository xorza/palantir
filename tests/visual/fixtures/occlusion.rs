//! Occlusion pruning at fractional coordinates: an opaque quad over another must paint what the pair paints when no pruning can apply.

use glam::UVec2;
use palantir::golden::image::RgbaImage;
use palantir::widget::Shape;
use palantir::{Configure, Panel, Rect, RgbaF32, Sizing, Ui};

use crate::fixtures::canvas;
use crate::goldens::assert_same;
use crate::harness::Harness;

const VIEWPORT: UVec2 = UVec2::new(128, 128);
const CLEAR: RgbaF32 = RgbaF32::WHITE;
const LAYER_RECT: Rect = Rect::new(20.25, 20.25, 80.0, 80.0);

fn add_layer(ui: &mut Ui, color: RgbaF32) {
    ui.add_shape(Shape::rect(LAYER_RECT).fill(color));
}

fn render_fractional_layers(split_groups: bool) -> RgbaImage {
    Harness::new_with_pixel_snap(false)
        .size(VIEWPORT)
        .clear(CLEAR)
        .frame(|ui| {
            assert!(!ui.display().pixel_snap);
            canvas(ui, |ui| {
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
        .image
}

#[test]
fn fractional_opaque_quads_match_unpruned_reference() {
    let optimized = render_fractional_layers(false);
    let unpruned = render_fractional_layers(true);
    assert_same("occlusion_fractional_opaque_quads", &optimized, &unpruned);
}
