//! Push/pop balance, and when a rounded clip needs the stencil.

use crate::internals::harness::UiHarness;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::paint::stroke::Stroke;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, panel::Panel};
use glam::UVec2;

/// Pin: a clip-only Surface (no painted background) still emits a
/// PushClip/PopClip pair so children get clipped, while contributing zero
/// rect quads of its own.
#[test]
fn clip_only_surface_emits_clip_but_no_draw() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::zstack()
                .id(WidgetId::from_hash("clip_only"))
                .size(50.0)
                .clip_rect()
                .show(ui, |_| {});
        });
    });
    assert_eq!(h.encode_paint().kinds(), ["PushClip", "PopClip"]);
}

#[test]
fn clip_emits_balanced_push_pop() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::zstack()
                .id(WidgetId::from_hash("clip"))
                .size(50.0)
                .clip_rect()
                .show(ui, |ui| {
                    Block::new()
                        .id(WidgetId::from_hash("inner"))
                        .size(40.0)
                        .background(Background::fill(RgbaF32::srgb(0.5, 0.5, 0.5)))
                        .show(ui);
                });
        });
    });
    // The draw sits inside the pair.
    assert_eq!(h.encode_paint().kinds(), ["PushClip", "Quad", "PopClip"]);
}

#[test]
fn clip_rounded_emits_push_clip_rounded_when_background_has_radius() {
    use crate::primitives::geometry::corners::Corners;
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::zstack()
                .id(WidgetId::from_hash("rounded"))
                .size(80.0)
                .background(Background {
                    fill: RgbaF32::srgb(0.2, 0.2, 0.2).into(),
                    border: Stroke::new(RgbaF32::srgb(1.0, 1.0, 1.0), 2.0),
                    corners: Corners::all(8.0),
                    shadow: Shadow::NONE,
                })
                .clip_rounded()
                .show(ui, |ui| {
                    Block::new()
                        .id(WidgetId::from_hash("c"))
                        .size(40.0)
                        .show(ui);
                });
        });
    });
    let cmds = h.encode_paint();
    assert_eq!(cmds.kinds(), ["Quad", "PushClip", "PopClip"]);
    let payload = cmds.calls[1].as_push_clip().unwrap();

    let panel_rect = h.arranged(WidgetId::from_hash("rounded"));
    // Stroke=2 is auto-folded into padding by `Tree::open_node`, so the
    // encoder's `rect.deflated_by(padding)` insets the mask by 2 on
    // every side. Radius reduces by 2 to stay concentric with the
    // painted stroke's inner edge.
    assert_eq!(payload.rect, panel_rect.deflated_by(Spacing::all(2.0)));
    assert_eq!(payload.corners, Corners::all(6.0));
}

#[test]
fn clip_rounded_falls_back_to_scissor_without_background() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::zstack()
                .id(WidgetId::from_hash("rounded_no_bg"))
                .size(80.0)
                .clip_rounded()
                .show(ui, |ui| {
                    Block::new()
                        .id(WidgetId::from_hash("c"))
                        .size(40.0)
                        .show(ui);
                });
        });
    });
    let cmds = h.encode_paint();
    assert_eq!(cmds.kinds(), ["PushClip", "PopClip"]);
    assert!(
        cmds.calls[0].as_push_clip().unwrap().corners.approx_zero(),
        "no background → no radius → falls back to plain scissor",
    );
}

#[test]
fn nested_clips_each_emit_their_own_pair() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::zstack()
                .id(WidgetId::from_hash("outer"))
                .size(Sizing::fixed(100.0))
                .clip_rect()
                .show(ui, |ui| {
                    Panel::zstack()
                        .id(WidgetId::from_hash("inner"))
                        .size(Sizing::fixed(50.0))
                        .clip_rect()
                        .show(ui, |_| {});
                });
        });
    });
    // Nested, not two siblings: the inner pair sits inside the outer one.
    assert_eq!(
        h.encode_paint().kinds(),
        ["PushClip", "PushClip", "PopClip", "PopClip"],
    );
}
