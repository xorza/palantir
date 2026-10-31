//! Push/pop balance, and when a rounded clip needs the stencil.

use crate::host::harness::Harness;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::backdrop_filter::BackdropFilter;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::paint::stroke::Stroke;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, panel::Panel};
use glam::UVec2;

/// A clip-only Surface (no painted chrome) still emits a PushClip/PopClip pair so children are clipped, with zero rect quads of its own.
#[test]
fn clip_only_surface_emits_clip_but_no_draw() {
    let mut h = Harness::mono(UVec2::new(200, 200));
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
    let mut h = Harness::mono(UVec2::new(200, 200));
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
                        .chrome(Chrome::fill(RgbaF32::srgb(0.5, 0.5, 0.5)))
                        .show(ui);
                });
        });
    });
    assert_eq!(h.encode_paint().kinds(), ["PushClip", "Quad", "PopClip"]);
}

#[test]
fn clip_rounded_emits_push_clip_rounded_when_chrome_has_radius() {
    use crate::primitives::geometry::corners::Corners;
    let mut h = Harness::mono(UVec2::new(200, 200));
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::zstack()
                .id(WidgetId::from_hash("rounded"))
                .size(80.0)
                .chrome(Chrome {
                    fill: RgbaF32::srgb(0.2, 0.2, 0.2).into(),
                    border: Stroke::new(RgbaF32::srgb(1.0, 1.0, 1.0), 2.0),
                    corners: Corners::all(8.0),
                    shadow: Shadow::NONE,
                    backdrop_filter: BackdropFilter::NONE,
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
    // Stroke=2 folds into padding, so the mask insets by 2 and its radius drops by 2 to stay concentric with the stroke's inner edge.
    assert_eq!(payload.rect, panel_rect.deflated_by(Spacing::all(2.0)));
    assert_eq!(payload.corners, Corners::all(6.0));
}

/// The mask fits the radii to the box before it insets them. On 100×30 a radius of 20 fits to
/// `20 · 30 / 40 = 15`, as the chrome paints it; each corner then loses its larger inset, 10, so
/// the mask's radius is 5. Insetting the unfitted 20 gave 10, a curve outside the painted one.
#[test]
fn clip_rounded_fits_overflowing_radii_before_the_uneven_inset() {
    use crate::primitives::geometry::corners::Corners;
    let mut h = Harness::mono(UVec2::new(200, 200));
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::zstack()
                .id(WidgetId::from_hash("pill"))
                .size((100.0, 30.0))
                .padding(Spacing::xy(10.0, 2.0))
                .chrome(Chrome::rounded(
                    RgbaF32::srgb(0.2, 0.2, 0.2),
                    Corners::all(20.0),
                ))
                .clip_rounded()
                .show(ui, |ui| {
                    Block::new()
                        .id(WidgetId::from_hash("c"))
                        .size(10.0)
                        .show(ui);
                });
        });
    });
    let cmds = h.encode_paint();
    let payload = cmds
        .calls
        .iter()
        .find_map(|call| call.as_push_clip())
        .expect("a rounded clip");
    let rect = h.arranged(WidgetId::from_hash("pill"));
    assert_eq!(payload.rect, rect.deflated_by(Spacing::xy(10.0, 2.0)));
    assert_eq!(payload.corners, Corners::all(5.0));
}

#[test]
fn clip_rounded_falls_back_to_scissor_without_chrome() {
    let mut h = Harness::mono(UVec2::new(200, 200));
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
        cmds.calls[0]
            .as_push_clip()
            .unwrap()
            .corners
            .is_approx_zero(),
        "no background → no radius → falls back to plain scissor",
    );
}

#[test]
fn nested_clips_each_emit_their_own_pair() {
    let mut h = Harness::mono(UVec2::new(200, 200));
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
    assert_eq!(
        h.encode_paint().kinds(),
        ["PushClip", "PushClip", "PopClip", "PopClip"],
    );
}
