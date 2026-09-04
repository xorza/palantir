//! Disabled and hidden subtrees, against the hit index that mirrors them.

use crate::Ui;
use crate::input::input_event::InputEvent;
use crate::input::pointer::PointerButton;
use crate::input::sense::Sense;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{color::RgbaF32, translate_scale::TranslateScale};
use crate::renderer::frontend::encoder::tests::support::screen_rects_by_fill;
use crate::scene::node::configure::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::{frame::Frame, panel::Panel};
use glam::{UVec2, Vec2};

#[test]
fn cascade_matches_hit_index_for_visible_disabled_and_hidden() {
    // Visible and disabled get the same effective screen rect; hidden is
    // skipped by encoder but tracked by hit index. Clicks land on visible
    // and are suppressed for both disabled (sense cascade) and hidden
    // (visibility cascade).
    let v_color = RgbaF32::srgb(1.0, 0.0, 0.0);
    let d_color = RgbaF32::srgb(0.0, 1.0, 0.0);
    let h_color = RgbaF32::srgb(0.0, 0.0, 1.0);
    let xform = TranslateScale::new(Vec2::new(5.0, 7.0), 2.0);

    let surface = UVec2::new(400, 400);
    let build = |ui: &mut Ui, capture: &mut (bool, bool, bool)| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::canvas()
                .id(WidgetId::from_hash("mid"))
                .size(200.0)
                .clip_rect()
                .transform(xform)
                .show(ui, |ui| {
                    capture.0 |= Frame::new()
                        .id(WidgetId::from_hash("V"))
                        .position((0.0, 0.0))
                        .size(30.0)
                        .background(Background {
                            fill: v_color.into(),
                            ..Default::default()
                        })
                        .sense(Sense::CLICK)
                        .show(ui)
                        .left
                        .clicked();
                    capture.1 |= Frame::new()
                        .id(WidgetId::from_hash("D"))
                        .position((40.0, 0.0))
                        .size(30.0)
                        .background(Background {
                            fill: d_color.into(),
                            ..Default::default()
                        })
                        .sense(Sense::CLICK)
                        .disabled(true)
                        .show(ui)
                        .left
                        .clicked();
                    capture.2 |= Frame::new()
                        .id(WidgetId::from_hash("H"))
                        .position((80.0, 0.0))
                        .size(30.0)
                        .background(Background {
                            fill: h_color.into(),
                            ..Default::default()
                        })
                        .sense(Sense::CLICK)
                        .hidden()
                        .show(ui)
                        .left
                        .clicked();
                });
        });
    };

    let mut h = UiHarness::new(surface);
    let mut sink = (false, false, false);
    h.frame(|ui| build(ui, &mut sink));

    let cmds = h.encode_paint();
    let drawn = screen_rects_by_fill(&cmds);

    // Encoder stores fills as `RgbaF16` now; encode the expected
    // colours the same way for bit-exact comparison.
    use crate::primitives::color::RgbaF16;
    let v_color_f16: RgbaF16 = v_color.into();
    let d_color_f16: RgbaF16 = d_color.into();
    let h_color_f16: RgbaF16 = h_color.into();

    let v_id = WidgetId::from_hash("V");
    let v_screen = drawn
        .iter()
        .find(|(c, _)| *c == v_color_f16)
        .map(|(_, r)| *r)
        .expect("visible node should emit a rect quad");
    let v_hit = h.ui.response_for(v_id).rect.expect("visible has hit rect");
    assert_eq!(v_screen, v_hit, "encoder vs hit-index rect for V");

    let d_id = WidgetId::from_hash("D");
    let d_screen = drawn
        .iter()
        .find(|(c, _)| *c == d_color_f16)
        .map(|(_, r)| *r)
        .expect("disabled node should still paint");
    let d_hit = h.ui.response_for(d_id).rect.expect("disabled has rect");
    assert_eq!(d_screen, d_hit, "encoder vs hit-index rect for D");

    let h_id = WidgetId::from_hash("H");
    assert!(
        !drawn.iter().any(|(c, _)| *c == h_color_f16),
        "hidden node must not emit a rect quad"
    );
    assert!(h.ui.response_for(h_id).rect.is_some());

    fn press_and_release_at(ui: &mut Ui, p: Vec2) {
        ui.on_input(InputEvent::PointerMoved(p));
        ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
        ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
    }
    press_and_release_at(
        &mut h.ui,
        v_hit.min + Vec2::new(v_hit.size.w, v_hit.size.h) * 0.5,
    );
    press_and_release_at(
        &mut h.ui,
        d_hit.min + Vec2::new(d_hit.size.w, d_hit.size.h) * 0.5,
    );
    let h_hit = h.ui.response_for(h_id).rect.unwrap();
    press_and_release_at(
        &mut h.ui,
        h_hit.min + Vec2::new(h_hit.size.w, h_hit.size.h) * 0.5,
    );

    let mut got = (false, false, false);
    h.frame(|ui| build(ui, &mut got));
    assert!(got.0, "visible widget should click");
    assert!(!got.1, "disabled widget must not click (sense cascade)");
    assert!(!got.2, "hidden widget must not click (visibility cascade)");
}

#[test]
fn disabled_ancestor_propagates_disabled_flag_to_descendants() {
    let mut h = UiHarness::new(UVec2::new(100, 100));
    let child = h.frame_value(|ui| {
        Panel::vstack()
            .auto_id()
            .disabled(true)
            .show(ui, |ui| {
                Frame::new()
                    .auto_id()
                    .size(Sizing::fixed(40.0))
                    .background(Background {
                        fill: RgbaF32::srgb(1.0, 0.0, 0.0).into(),
                        ..Default::default()
                    })
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
