use crate::Ui;
use crate::forest::element::Configure;
use crate::input::InputEvent;
use crate::input::sense::Sense;
use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

#[test]
fn nested_scroll_panels_route_to_innermost_under_pointer() {
    let mut ui = Ui::for_test();
    let surface = UVec2::new(300, 300);
    let build = |ui: &mut Ui| {
        Panel::zstack()
            .id(WidgetId::from_hash("outer"))
            .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
            .sense(Sense::SCROLL)
            .show(ui, |ui| {
                Panel::zstack()
                    .id(WidgetId::from_hash("inner"))
                    .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
                    .sense(Sense::SCROLL)
                    .show(ui, |_| {});
            });
    };
    ui.run_at_acked(surface, build);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    ui.on_input(InputEvent::ScrollPixels(Vec2::new(0.0, 5.0)));
    let inner_id = WidgetId::from_hash("inner");
    let outer_id = WidgetId::from_hash("outer");
    let mut inner_d = Vec2::ZERO;
    let mut outer_d = Vec2::ZERO;
    ui.run_at_acked(surface, |ui| {
        build(ui);
        if inner_d == Vec2::ZERO {
            inner_d = ui.input.scroll_delta_for(inner_id, 40.0);
            outer_d = ui.input.scroll_delta_for(outer_id, 40.0);
        }
    });
    assert_eq!(inner_d, Vec2::new(0.0, 5.0));
    assert_eq!(outer_d, Vec2::ZERO);
}

#[test]
fn scroll_delta_zero_for_non_target() {
    let mut ui = Ui::for_test();
    let surface = UVec2::new(200, 200);
    let build = |ui: &mut Ui| {
        Panel::zstack()
            .id(WidgetId::from_hash("scroller"))
            .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
            .sense(Sense::SCROLL)
            .show(ui, |_| {});
    };
    ui.run_at_acked(surface, build);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    ui.on_input(InputEvent::ScrollPixels(Vec2::new(0.0, 9.0)));
    let unrelated = WidgetId::from_hash("nope");
    let mut d = Vec2::new(1.0, 1.0);
    ui.run_at_acked(surface, |ui| {
        build(ui);
        d = ui.input.scroll_delta_for(unrelated, 40.0);
    });
    // Both passes return zero — the widget id never matches.
    assert_eq!(d, Vec2::ZERO);
}

#[test]
fn pointer_left_clears_scroll_target() {
    let mut ui = Ui::for_test();
    let surface = UVec2::new(200, 200);
    let build = |ui: &mut Ui| {
        Panel::zstack()
            .id(WidgetId::from_hash("scroller"))
            .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
            .sense(Sense::SCROLL)
            .show(ui, |_| {});
    };
    ui.run_at_acked(surface, build);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    ui.on_input(InputEvent::PointerLeft);
    ui.on_input(InputEvent::ScrollPixels(Vec2::new(0.0, 5.0)));
    let id = WidgetId::from_hash("scroller");
    let mut d = Vec2::new(1.0, 1.0);
    ui.run_at_acked(surface, |ui| {
        build(ui);
        d = ui.input.scroll_delta_for(id, 40.0);
    });
    assert_eq!(
        d,
        Vec2::ZERO,
        "PointerLeft drops scroll target so the delta is unclaimed",
    );
}

/// `Sense::SCROLL` widget alone (without `Sense::PINCH`) receives
/// `scroll_delta` for wheel/pinch-pan events but a `1.0` `zoom_factor`
/// for pinch — the routing bits are independent. Note the
/// `response_for` call lives **inside** the record closure: the
/// frame accumulators (`frame_scroll_pixels` / `frame_zoom_delta`)
/// are cleared by `post_record`, so a post-frame read would see
/// zeroes.
#[test]
fn sense_scroll_routes_scroll_but_not_pinch() {
    let mut ui = Ui::for_test();
    let surface = UVec2::new(200, 200);
    let id = WidgetId::from_hash("scroll_only");
    let build = |ui: &mut Ui| {
        Panel::zstack()
            .id(id)
            .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
            .sense(Sense::SCROLL)
            .show(ui, |_| {});
    };
    ui.run_at_acked(surface, build);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    ui.on_input(InputEvent::ScrollPixels(Vec2::new(0.0, 9.0)));
    ui.on_input(InputEvent::Zoom(1.5));
    let mut scroll_delta = Vec2::ZERO;
    let mut zoom_factor = f32::NAN;
    ui.run_at_acked(surface, |ui| {
        build(ui);
        let resp = ui.response_for(id);
        scroll_delta = resp.scroll_delta;
        zoom_factor = resp.zoom_factor;
    });
    assert_eq!(
        scroll_delta,
        Vec2::new(0.0, 9.0),
        "Sense::SCROLL must receive wheel/touchpad scroll deltas",
    );
    assert!(
        (zoom_factor - 1.0).abs() < 1e-6,
        "Sense::SCROLL alone (no PINCH) must NOT receive pinch — \
         zoom_factor stayed at identity; got {zoom_factor}",
    );
}

/// `Sense::PINCH` widget alone (without `Sense::SCROLL`) receives
/// `zoom_factor` for pinch events but `Vec2::ZERO` `scroll_delta` for
/// wheel — the sister of `sense_scroll_routes_scroll_but_not_pinch`.
#[test]
fn sense_pinch_routes_pinch_but_not_scroll() {
    let mut ui = Ui::for_test();
    let surface = UVec2::new(200, 200);
    let id = WidgetId::from_hash("pinch_only");
    let build = |ui: &mut Ui| {
        Panel::zstack()
            .id(id)
            .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
            .sense(Sense::PINCH)
            .show(ui, |_| {});
    };
    ui.run_at_acked(surface, build);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    ui.on_input(InputEvent::ScrollPixels(Vec2::new(0.0, 9.0)));
    ui.on_input(InputEvent::Zoom(1.5));
    let mut scroll_delta = Vec2::new(1.0, 1.0);
    let mut zoom_factor = f32::NAN;
    ui.run_at_acked(surface, |ui| {
        build(ui);
        let resp = ui.response_for(id);
        scroll_delta = resp.scroll_delta;
        zoom_factor = resp.zoom_factor;
    });
    assert_eq!(
        scroll_delta,
        Vec2::ZERO,
        "Sense::PINCH alone (no SCROLL) must NOT receive wheel/touchpad \
         scroll deltas",
    );
    assert!(
        (zoom_factor - 1.5).abs() < 1e-6,
        "Sense::PINCH must receive pinch zoom factor; got {zoom_factor}",
    );
}
