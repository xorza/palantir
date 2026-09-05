use crate::Ui;
use crate::input::response::scroll_delta::ScrollDelta;
use crate::input::sense::Sense;
use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::ui::harness::UiHarness;
use crate::widgets::configure::Configure;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

fn build_two_gesture_targets(ui: &mut Ui) {
    Panel::hstack()
        .id(WidgetId::from_hash("root"))
        .show(ui, |ui| {
            for name in ["a", "b"] {
                Panel::zstack()
                    .id(WidgetId::from_hash(name))
                    .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
                    .sense(Sense::SCROLL | Sense::PINCH)
                    .show(ui, |_| {});
            }
        });
}

fn route_across_two_targets(second_delta: bool) -> [ScrollDelta; 2] {
    let surface = UVec2::new(200, 100);
    let mut h = UiHarness::new(surface);
    h.frame(build_two_gesture_targets);

    h.scroll_pixels_at(Vec2::new(50.0, 50.0), Vec2::new(2.0, 3.0));
    h.move_to(Vec2::new(150.0, 50.0));
    if second_delta {
        h.scroll_lines(Vec2::new(4.0, 5.0));
    }

    let mut observed = None;
    h.frame(|ui| {
        build_two_gesture_targets(ui);
        if observed.is_none() {
            observed = Some([
                ui.response_for(WidgetId::from_hash("a")).scroll,
                ui.response_for(WidgetId::from_hash("b")).scroll,
            ]);
        }
    });
    observed.unwrap()
}

#[test]
fn scroll_deltas_stay_with_their_event_time_targets() {
    let without_second = route_across_two_targets(false);
    assert_eq!(
        without_second,
        [
            ScrollDelta {
                pixels: Vec2::new(2.0, 3.0),
                ..ScrollDelta::default()
            },
            ScrollDelta::default(),
        ],
        "moving to B must not reassign A's earlier delta",
    );

    let with_second = route_across_two_targets(true);
    assert_eq!(
        with_second,
        [
            ScrollDelta {
                pixels: Vec2::new(2.0, 3.0),
                ..ScrollDelta::default()
            },
            ScrollDelta {
                lines: Vec2::new(4.0, 5.0),
                ..ScrollDelta::default()
            },
        ],
        "each target must receive only the deltas that arrived over it",
    );
}

#[test]
fn pointer_leave_after_scroll_keeps_the_pending_target_delta() {
    let surface = UVec2::new(200, 100);
    let mut h = UiHarness::new(surface);
    h.frame(build_two_gesture_targets);
    h.scroll_pixels_at(Vec2::new(50.0, 50.0), Vec2::new(7.0, 11.0));
    h.pointer_left();

    let mut observed = None;
    h.frame(|ui| {
        build_two_gesture_targets(ui);
        if observed.is_none() {
            observed = Some(ui.response_for(WidgetId::from_hash("a")).scroll);
        }
    });
    assert_eq!(
        observed.unwrap(),
        ScrollDelta {
            pixels: Vec2::new(7.0, 11.0),
            ..ScrollDelta::default()
        },
    );
}

#[test]
fn pinch_products_accumulate_independently_per_event_time_target() {
    let surface = UVec2::new(200, 100);
    let mut h = UiHarness::new(surface);
    h.frame(build_two_gesture_targets);
    h.pinch_at(Vec2::new(50.0, 50.0), 1.1);
    h.pinch(1.05);
    h.pinch_at(Vec2::new(150.0, 50.0), 0.5);

    let mut observed = None;
    h.frame(|ui| {
        build_two_gesture_targets(ui);
        if observed.is_none() {
            observed = Some([
                ui.response_for(WidgetId::from_hash("a")).scroll.zoom,
                ui.response_for(WidgetId::from_hash("b")).scroll.zoom,
            ]);
        }
    });
    let [a, b] = observed.unwrap();
    assert!((a - 1.155).abs() < 1e-6, "A product: {a}");
    assert!((b - 0.5).abs() < 1e-6, "B product: {b}");
}

#[test]
fn nested_scroll_panels_route_to_innermost_under_pointer() {
    let surface = UVec2::new(300, 300);
    let mut h = UiHarness::new(surface);
    let build = |ui: &mut Ui| {
        Panel::zstack()
            .id(WidgetId::from_hash("outer"))
            .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
            .sense(Sense::SCROLL)
            .show(ui, |ui| {
                Panel::zstack()
                    .id(WidgetId::from_hash("inner"))
                    .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
                    .sense(Sense::SCROLL)
                    .show(ui, |_| {});
            });
    };
    h.frame(build);
    h.scroll_pixels_at(Vec2::new(50.0, 50.0), Vec2::new(0.0, 5.0));
    let inner_id = WidgetId::from_hash("inner");
    let outer_id = WidgetId::from_hash("outer");
    let mut inner_d = Vec2::ZERO;
    let mut outer_d = Vec2::ZERO;
    h.frame(|ui| {
        build(ui);
        if inner_d == Vec2::ZERO {
            inner_d = ui.input().scroll_delta_for(inner_id).pixels;
            outer_d = ui.input().scroll_delta_for(outer_id).pixels;
        }
    });
    assert_eq!(inner_d, Vec2::new(0.0, 5.0));
    assert_eq!(outer_d, Vec2::ZERO);
}

#[test]
fn scroll_delta_zero_for_non_target() {
    let surface = UVec2::new(200, 200);
    let mut h = UiHarness::new(surface);
    let build = |ui: &mut Ui| {
        Panel::zstack()
            .id(WidgetId::from_hash("scroller"))
            .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
            .sense(Sense::SCROLL)
            .show(ui, |_| {});
    };
    h.frame(build);
    h.scroll_pixels_at(Vec2::new(50.0, 50.0), Vec2::new(0.0, 9.0));
    let unrelated = WidgetId::from_hash("nope");
    let mut d = Vec2::new(1.0, 1.0);
    h.frame(|ui| {
        build(ui);
        d = ui.input().scroll_delta_for(unrelated).pixels;
    });
    // Both passes return zero — the widget id never matches.
    assert_eq!(d, Vec2::ZERO);
}

#[test]
fn pointer_left_clears_scroll_target() {
    let surface = UVec2::new(200, 200);
    let mut h = UiHarness::new(surface);
    let build = |ui: &mut Ui| {
        Panel::zstack()
            .id(WidgetId::from_hash("scroller"))
            .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
            .sense(Sense::SCROLL)
            .show(ui, |_| {});
    };
    h.frame(build);
    h.move_to(Vec2::new(50.0, 50.0));
    h.pointer_left();
    h.scroll_pixels(Vec2::new(0.0, 5.0));
    let id = WidgetId::from_hash("scroller");
    let mut d = Vec2::new(1.0, 1.0);
    h.frame(|ui| {
        build(ui);
        d = ui.input().scroll_delta_for(id).pixels;
    });
    assert_eq!(
        d,
        Vec2::ZERO,
        "PointerLeft drops scroll target so the delta is unclaimed",
    );
}

#[test]
fn scroll_over_inert_area_is_not_delivered_to_a_later_target() {
    let surface = UVec2::new(200, 200);
    let mut h = UiHarness::new(surface);
    let id = WidgetId::from_hash("scroller");
    let build = |ui: &mut Ui| {
        Panel::zstack()
            .id(id)
            .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
            .sense(Sense::SCROLL)
            .show(ui, |_| {});
    };
    h.frame(build);

    let scroll = h.scroll_pixels_at(Vec2::new(150.0, 150.0), Vec2::new(0.0, 12.0));
    assert!(
        !scroll.requests_repaint,
        "scroll with no current target must be discarded",
    );
    h.move_to(Vec2::new(50.0, 50.0));

    let mut delivered = Vec2::new(f32::NAN, f32::NAN);
    h.frame(|ui| {
        build(ui);
        delivered = ui.input().scroll_delta_for(id).pixels;
    });
    assert_eq!(
        delivered,
        Vec2::ZERO,
        "a later scroll target must not receive an earlier inert-area event",
    );
}

/// `Sense::SCROLL` widget alone (without `Sense::PINCH`) receives
/// `scroll_delta` for wheel/pinch-pan events but a `1.0` `zoom_factor`
/// for pinch — the routing bits are independent. Note the
/// `response_for` call lives **inside** the record closure: the
/// frame target-delta rows are cleared by `post_record`, so a
/// post-frame read would see identity values.
#[test]
fn sense_scroll_routes_scroll_but_not_pinch() {
    let surface = UVec2::new(200, 200);
    let mut h = UiHarness::new(surface);
    let id = WidgetId::from_hash("scroll_only");
    let build = |ui: &mut Ui| {
        Panel::zstack()
            .id(id)
            .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
            .sense(Sense::SCROLL)
            .show(ui, |_| {});
    };
    h.frame(build);
    h.scroll_pixels_at(Vec2::new(50.0, 50.0), Vec2::new(0.0, 9.0));
    h.pinch(1.5);
    let mut scroll_pixels = Vec2::ZERO;
    let mut zoom_factor = f32::NAN;
    h.frame(|ui| {
        build(ui);
        let resp = ui.response_for(id);
        scroll_pixels = resp.scroll.pixels;
        zoom_factor = resp.scroll.zoom;
    });
    assert_eq!(
        scroll_pixels,
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
    let surface = UVec2::new(200, 200);
    let mut h = UiHarness::new(surface);
    let id = WidgetId::from_hash("pinch_only");
    let build = |ui: &mut Ui| {
        Panel::zstack()
            .id(id)
            .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
            .sense(Sense::PINCH)
            .show(ui, |_| {});
    };
    h.frame(build);
    h.scroll_pixels_at(Vec2::new(50.0, 50.0), Vec2::new(0.0, 9.0));
    h.pinch(1.5);
    let mut scroll_pixels = Vec2::new(1.0, 1.0);
    let mut zoom_factor = f32::NAN;
    h.frame(|ui| {
        build(ui);
        let resp = ui.response_for(id);
        scroll_pixels = resp.scroll.pixels;
        zoom_factor = resp.scroll.zoom;
    });
    assert_eq!(
        scroll_pixels,
        Vec2::ZERO,
        "Sense::PINCH alone (no SCROLL) must NOT receive wheel/touchpad \
         scroll deltas",
    );
    assert!(
        (zoom_factor - 1.5).abs() < 1e-6,
        "Sense::PINCH must receive pinch zoom factor; got {zoom_factor}",
    );
}
