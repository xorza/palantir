use crate::forest::element::Configure;
use crate::input::InputEvent;
use crate::input::sense::Sense;
use crate::layout::types::sizing::Sizing;
use crate::support::testing::ui_at;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

#[test]
fn nested_scroll_panels_route_to_innermost_under_pointer() {
    // Outer scroll panel (200×200) wraps an inner scroll panel (100×100)
    // at the top-left. Pointer over the inner area must claim the
    // delta on the inner; pointer outside inner but inside outer
    // routes to outer.
    let mut ui = ui_at(UVec2::new(300, 300));
    Panel::zstack()
        .id_salt("outer")
        .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
        .sense(Sense::SCROLL)
        .show(&mut ui, |ui| {
            Panel::zstack()
                .id_salt("inner")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
                .sense(Sense::SCROLL)
                .show(ui, |_| {});
        });
    ui.record_phase();
    ui.paint_phase();
    ui.pre_record(crate::layout::types::display::Display::default());
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    ui.on_input(InputEvent::Scroll(Vec2::new(0.0, 5.0)));
    Panel::zstack()
        .id_salt("outer")
        .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
        .sense(Sense::SCROLL)
        .show(&mut ui, |ui| {
            Panel::zstack()
                .id_salt("inner")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
                .sense(Sense::SCROLL)
                .show(ui, |_| {});
        });
    let inner_id = crate::forest::widget_id::WidgetId::from_hash("inner");
    let outer_id = crate::forest::widget_id::WidgetId::from_hash("outer");
    assert_eq!(ui.input.scroll_delta_for(inner_id), Vec2::new(0.0, 5.0));
    assert_eq!(ui.input.scroll_delta_for(outer_id), Vec2::ZERO);
    ui.record_phase();
    ui.paint_phase();
}

#[test]
fn scroll_delta_zero_for_non_target() {
    let mut ui = ui_at(UVec2::new(200, 200));
    Panel::zstack()
        .id_salt("scroller")
        .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
        .sense(Sense::SCROLL)
        .show(&mut ui, |_| {});
    ui.record_phase();
    ui.paint_phase();
    ui.pre_record(crate::layout::types::display::Display::default());
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    ui.on_input(InputEvent::Scroll(Vec2::new(0.0, 9.0)));
    Panel::zstack()
        .id_salt("scroller")
        .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
        .sense(Sense::SCROLL)
        .show(&mut ui, |_| {});
    let unrelated = crate::forest::widget_id::WidgetId::from_hash("nope");
    assert_eq!(ui.input.scroll_delta_for(unrelated), Vec2::ZERO);
    ui.record_phase();
    ui.paint_phase();
}

#[test]
fn pointer_left_clears_scroll_target() {
    let mut ui = ui_at(UVec2::new(200, 200));
    Panel::zstack()
        .id_salt("scroller")
        .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
        .sense(Sense::SCROLL)
        .show(&mut ui, |_| {});
    ui.record_phase();
    ui.paint_phase();
    ui.pre_record(crate::layout::types::display::Display::default());
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    ui.on_input(InputEvent::PointerLeft);
    ui.on_input(InputEvent::Scroll(Vec2::new(0.0, 5.0)));
    Panel::zstack()
        .id_salt("scroller")
        .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
        .sense(Sense::SCROLL)
        .show(&mut ui, |_| {});
    let id = crate::forest::widget_id::WidgetId::from_hash("scroller");
    assert_eq!(
        ui.input.scroll_delta_for(id),
        Vec2::ZERO,
        "PointerLeft drops scroll target so the delta is unclaimed",
    );
    ui.record_phase();
    ui.paint_phase();
}
