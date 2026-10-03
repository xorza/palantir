//! The deviceless frontend paints what a frame planned, and only that.

use crate::internals::harness::frontend_harness::FrontendHarness;
use crate::internals::harness::tests::support::SURFACE;
use crate::internals::harness::*;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::ui::frame_report::FramePaint;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;

fn tile(ui: &mut Ui) {
    Block::new()
        .id_salt("tile")
        .size(20.0)
        .background(Background::fill(RgbaF32::WHITE))
        .show(ui);
}

#[test]
fn frame_paints_its_plan_and_paint_full_paints_the_scene() {
    let mut h = FrontendHarness::new(UiHarness::new(SURFACE));
    let quads = |h: &FrontendHarness| {
        h.frontend
            .buffer
            .quads
            .iter()
            .map(|quad| quad.rect)
            .collect::<Vec<_>>()
    };
    // The tile sits at the surface origin, scale 1: physical = logical.
    let tile_rect = Rect::new(0.0, 0.0, 20.0, 20.0);

    assert_eq!(h.frame(tile).paint(), FramePaint::Full);
    assert_eq!(quads(&h), [tile_rect]);

    // A still frame plans no paint, so the buffer is left as it was —
    // emptied here, so a rebuild would show.
    h.frontend.buffer.quads.clear();
    assert_eq!(h.frame(tile).paint(), FramePaint::Skip);
    assert_eq!(quads(&h), []);

    h.paint_full();
    assert_eq!(quads(&h), [tile_rect]);
}
