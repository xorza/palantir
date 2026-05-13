use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::widget_id::WidgetId;
use crate::input::InputEvent;
use crate::input::sense::Sense;
use crate::layout::types::sizing::Sizing;
use crate::support::testing::run_at_acked;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

#[test]
fn nested_scroll_panels_route_to_innermost_under_pointer() {
    let mut ui = Ui::new();
    let surface = UVec2::new(300, 300);
    let build = |ui: &mut Ui| {
        Panel::zstack()
            .id_salt("outer")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
            .sense(Sense::SCROLL)
            .show(ui, |ui| {
                Panel::zstack()
                    .id_salt("inner")
                    .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
                    .sense(Sense::SCROLL)
                    .show(ui, |_| {});
            });
    };
    run_at_acked(&mut ui, surface, build);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    ui.on_input(InputEvent::ScrollPixels(Vec2::new(0.0, 5.0)));
    let inner_id = WidgetId::from_hash("inner");
    let outer_id = WidgetId::from_hash("outer");
    let mut inner_d = Vec2::ZERO;
    let mut outer_d = Vec2::ZERO;
    run_at_acked(&mut ui, surface, |ui| {
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
    let mut ui = Ui::new();
    let surface = UVec2::new(200, 200);
    let build = |ui: &mut Ui| {
        Panel::zstack()
            .id_salt("scroller")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
            .sense(Sense::SCROLL)
            .show(ui, |_| {});
    };
    run_at_acked(&mut ui, surface, build);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    ui.on_input(InputEvent::ScrollPixels(Vec2::new(0.0, 9.0)));
    let unrelated = WidgetId::from_hash("nope");
    let mut d = Vec2::new(1.0, 1.0);
    run_at_acked(&mut ui, surface, |ui| {
        build(ui);
        d = ui.input.scroll_delta_for(unrelated, 40.0);
    });
    // Both passes return zero — the widget id never matches.
    assert_eq!(d, Vec2::ZERO);
}

#[test]
fn pointer_left_clears_scroll_target() {
    let mut ui = Ui::new();
    let surface = UVec2::new(200, 200);
    let build = |ui: &mut Ui| {
        Panel::zstack()
            .id_salt("scroller")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
            .sense(Sense::SCROLL)
            .show(ui, |_| {});
    };
    run_at_acked(&mut ui, surface, build);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    ui.on_input(InputEvent::PointerLeft);
    ui.on_input(InputEvent::ScrollPixels(Vec2::new(0.0, 5.0)));
    let id = WidgetId::from_hash("scroller");
    let mut d = Vec2::new(1.0, 1.0);
    run_at_acked(&mut ui, surface, |ui| {
        build(ui);
        d = ui.input.scroll_delta_for(id, 40.0);
    });
    assert_eq!(
        d,
        Vec2::ZERO,
        "PointerLeft drops scroll target so the delta is unclaimed",
    );
}
