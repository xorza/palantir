//! Disabled and hidden subtrees, against the hit index that mirrors them.

use crate::Ui;
use crate::input::sense::Sense;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{color::RgbaF32, translate_scale::TranslateScale};
use crate::renderer::frontend::encoder::tests::support::{rect_with_fill, screen_rects_by_fill};
use crate::ui::harness::UiHarness;
use crate::widgets::configure::Configure;
use crate::widgets::{block::Block, panel::Panel};
use glam::{UVec2, Vec2};

#[test]
fn cascade_matches_hit_index_for_visible_disabled_and_hidden() {
    // Visible and disabled get the same effective screen rect; hidden is
    // skipped by encoder but tracked by hit index. Clicks land on visible
    // and are suppressed for both disabled (the response fold) and hidden
    // (visibility cascade).
    let v_color = RgbaF32::srgb(1.0, 0.0, 0.0);
    let d_color = RgbaF32::srgb(0.0, 1.0, 0.0);
    let h_color = RgbaF32::srgb(0.0, 0.0, 1.0);
    let xform = TranslateScale::new(Vec2::new(5.0, 7.0), 2.0);

    let surface = UVec2::new(400, 400);
    // Whether V, D and H clicked this pass.
    let build = |ui: &mut Ui| {
        Panel::hstack()
            .auto_id()
            .show(ui, |ui| {
                Panel::canvas()
                    .id(WidgetId::from_hash("mid"))
                    .size(200.0)
                    .clip_rect()
                    .transform(xform)
                    .show(ui, |ui| {
                        let v = Block::new()
                            .id(WidgetId::from_hash("V"))
                            .position((0.0, 0.0))
                            .size(30.0)
                            .background(Background::fill(v_color))
                            .sense(Sense::CLICK)
                            .show(ui)
                            .left
                            .clicked();
                        let d = Block::new()
                            .id(WidgetId::from_hash("D"))
                            .position((40.0, 0.0))
                            .size(30.0)
                            .background(Background::fill(d_color))
                            .sense(Sense::CLICK)
                            .disabled(true)
                            .show(ui)
                            .left
                            .clicked();
                        let h = Block::new()
                            .id(WidgetId::from_hash("H"))
                            .position((80.0, 0.0))
                            .size(30.0)
                            .background(Background::fill(h_color))
                            .sense(Sense::CLICK)
                            .hidden()
                            .show(ui)
                            .left
                            .clicked();
                        [v, d, h]
                    })
                    .inner
            })
            .inner
    };

    let mut h = UiHarness::new(surface);
    h.frame(|ui| {
        build(ui);
    });

    let cmds = h.encode_paint();
    let drawn = screen_rects_by_fill(&cmds);

    let v_id = WidgetId::from_hash("V");
    let v_screen = rect_with_fill(&drawn, v_color).expect("visible node should emit a rect quad");
    let v_hit = h.rect(v_id).expect("visible has hit rect");
    assert_eq!(v_screen, v_hit, "encoder vs hit-index rect for V");

    let d_id = WidgetId::from_hash("D");
    let d_screen = rect_with_fill(&drawn, d_color).expect("disabled node should still paint");
    let d_hit = h.rect(d_id).expect("disabled has rect");
    assert_eq!(d_screen, d_hit, "encoder vs hit-index rect for D");

    let h_id = WidgetId::from_hash("H");
    assert_eq!(
        rect_with_fill(&drawn, h_color),
        None,
        "hidden node must not emit a rect quad"
    );
    assert!(h.rect(h_id).is_some());

    // A frame per gesture: one release slot per button, so three
    // uninterrupted gestures would leave only the last one to read, and
    // `D` absorbing its press is exactly what makes that visible.
    let h_hit = h.rect(h_id).unwrap();
    for (target, expected, why) in [
        (v_hit, [true, false, false], "the visible widget clicks"),
        (
            d_hit,
            [false, false, false],
            "a disabled widget absorbs its press without clicking",
        ),
        (
            h_hit,
            [false, false, false],
            "a hidden widget does not click (visibility cascade)",
        ),
    ] {
        h.click_at(target.min + Vec2::new(target.size.w, target.size.h) * 0.5);
        assert_eq!(h.frame_value(build), expected, "{why}");
    }
}

#[test]
fn disabled_ancestor_propagates_disabled_flag_to_descendants() {
    let mut h = UiHarness::new(UVec2::new(100, 100));
    let child = h.frame_value(|ui| {
        Panel::vstack()
            .auto_id()
            .disabled(true)
            .show(ui, |ui| {
                Block::new()
                    .auto_id()
                    .size(Sizing::fixed(40.0))
                    .background(Background::fill(RgbaF32::srgb(1.0, 0.0, 0.0)))
                    .show(ui)
                    .node()
            })
            .inner
    });
    let cascade = &h.ui.cascade();
    // Main is first in `Layer::PAINT_ORDER`, so its `entries_base` is 0
    // and the node index doubles as the entry index.
    assert!(
        cascade.entries[child.idx()].disabled,
        "a disabled ancestor must flatten into the descendant's effective disabled",
    );
    // A cascaded-off node is never pushed to `hits`, so it cannot be
    // hit-tested — the behaviour the flattened flag exists to produce.
    assert!(
        cascade
            .hit_test(glam::Vec2::splat(20.0), |_| true)
            .is_none(),
    );
}
