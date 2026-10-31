//! Occlusion pruning at fractional coordinates: an opaque quad over another must paint what the pair paints when no pruning can apply.

use glam::UVec2;
use image::RgbaImage;
use palantir::widget::Shape;
use palantir::{Configure, Panel, Rect, RgbaF32, Sizing, Ui};

use crate::fixtures;
use crate::goldens;
use crate::visual_rig::VisualRig;

const VIEWPORT: UVec2 = UVec2::new(128, 128);
const CLEAR: RgbaF32 = RgbaF32::WHITE;
const LAYER_RECT: Rect = Rect::new(20.25, 20.25, 80.0, 80.0);

fn add_layer(ui: &mut Ui, color: RgbaF32) {
    ui.add_shape(Shape::rect(LAYER_RECT).fill(color));
}

/// Where the second layer records relative to the first.
#[derive(Clone, Copy, Debug)]
enum Grouping {
    /// One draw group, where the opaque top layer may prune the one under it.
    Shared,
    /// A clipped panel of its own, a separate group no pruning crosses.
    Split,
}

fn render_fractional_layers(grouping: Grouping) -> RgbaImage {
    VisualRig::unsnapped()
        .size(VIEWPORT)
        .clear(CLEAR)
        .frame(|ui| {
            assert!(!ui.display().pixel_snap);
            fixtures::canvas(ui, |ui| {
                add_layer(ui, RgbaF32::srgb(1.0, 0.0, 0.0));
                match grouping {
                    Grouping::Shared => add_layer(ui, RgbaF32::srgb(0.0, 0.0, 1.0)),
                    Grouping::Split => {
                        Panel::canvas()
                            .auto_id()
                            .size((Sizing::FILL, Sizing::FILL))
                            .clip_rect()
                            .show(ui, |ui| add_layer(ui, RgbaF32::srgb(0.0, 0.0, 1.0)));
                    }
                }
            });
        })
        .image
}

#[test]
fn fractional_opaque_quads_match_unpruned_reference() {
    let optimized = render_fractional_layers(Grouping::Shared);
    let unpruned = render_fractional_layers(Grouping::Split);
    goldens::assert_same("occlusion_fractional_opaque_quads", &optimized, &unpruned);
}
