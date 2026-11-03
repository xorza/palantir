//! The pointer layer: a cursor the app draws over everything, which no input reaches.

use glam::{UVec2, Vec2};

use crate::gpu::test_gpu::headless_test_gpu;
use crate::host::harness::Harness;
use crate::host::harness::harness_builder::Depth;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::scene::layer::Layer;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::button::Button;

/// A block of `color` over the window's top-left 8 × 8 px in `layer`.
fn block(ui: &mut Ui, layer: Layer, color: RgbaF32) {
    ui.layer(layer).show(|ui| {
        Block::new()
            .id_salt(("block", layer))
            .size(8.0)
            .chrome(Chrome::fill(color))
            .show(ui);
    });
}

/// Over one spot, blocks in `Tooltip`, `Debug` and `Pointer`, recorded pointer first: the
/// pointer's blue shows, as the layer paints after the other two whatever the record order.
#[test]
fn the_pointer_layer_paints_over_tooltip_and_debug() {
    let gpu = headless_test_gpu();
    let mut h = Harness::builder(UVec2::new(16, 16))
        .depth(Depth::gpu(gpu.handles()))
        .build();
    let blue = RgbaF32::new(0.0, 0.0, 1.0, 1.0);
    let mut app = |ui: &mut Ui| {
        block(ui, Layer::Pointer, blue);
        block(ui, Layer::Debug, RgbaF32::new(1.0, 0.0, 0.0, 1.0));
        block(ui, Layer::Tooltip, RgbaF32::new(0.0, 1.0, 0.0, 1.0));
    };
    let mut pixels = Vec::new();
    h.capture(&mut app, &mut pixels).unwrap();
    assert_eq!(pixels[(4 * 16 + 4) * 4..][..4], [0, 0, 255, 255]);
}

/// A click under a cursor drawn in the pointer layer presses the button beneath, and the
/// cursor's own sensing widget takes neither the click nor the hover.
#[test]
fn a_click_under_the_cursor_reaches_the_widget() {
    let mut h = Harness::mono(UVec2::new(200, 100));
    let button = WidgetId::from_hash("under");
    let cursor = WidgetId::from_hash("cursor");
    let mut clicked = false;
    let record = |ui: &mut Ui, clicked: &mut bool| {
        *clicked |= Button::new()
            .id(button)
            .label("Apply")
            .size(80.0)
            .show(ui)
            .clicked();
        ui.layer(Layer::Pointer).show(|ui| {
            Button::new().id(cursor).label("").size(80.0).show(ui);
        });
    };
    h.frame(|ui| record(ui, &mut false));
    let at = Vec2::new(20.0, 20.0);
    assert_eq!(h.hit_at(at), Some(button));
    h.click(at);
    h.frame(|ui| record(ui, &mut clicked));
    assert!(clicked);
}
