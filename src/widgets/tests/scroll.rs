use crate::input::InputEvent;
use crate::layout::types::display::Display;
use crate::layout::types::sizing::Sizing;
use crate::support::testing::ui_at;
use crate::tree::element::Configure;
use crate::tree::widget_id::WidgetId;
use crate::widgets::frame::Frame;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::{Scroll, ScrollState};
use glam::{UVec2, Vec2};

const SURFACE: UVec2 = UVec2::new(400, 600);

fn surface_display() -> Display {
    Display::from_physical(SURFACE, 1.0)
}

/// Wrap the scroll under a `Panel::vstack` root so its `Sizing::Fixed`
/// is honored. The root expands to surface; the panel's `vstack` slot
/// then hands the scroll exactly its declared size.
fn build(ui: &mut crate::ui::Ui, viewport_h: f32, content_h: f32) {
    Panel::vstack().with_id("root").show(ui, |ui| {
        Scroll::vertical()
            .with_id("scroll")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(viewport_h)))
            .show(ui, |ui| {
                Frame::new()
                    .with_id("content")
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(content_h)))
                    .show(ui);
            });
    });
}

fn read_state(ui: &mut crate::ui::Ui) -> ScrollState {
    *ui.state
        .get_or_insert_with::<ScrollState, _>(WidgetId::from_hash("scroll"), Default::default)
}

#[test]
fn scroll_state_records_viewport_and_content_after_arrange() {
    let mut ui = ui_at(SURFACE);
    build(&mut ui, 200.0, 800.0);
    ui.end_frame();

    let row = read_state(&mut ui);
    assert_eq!(row.viewport_h, 200.0);
    assert_eq!(row.content_h, 800.0);
    assert_eq!(row.offset, 0.0, "no wheel input → offset stays at 0");
}

#[test]
fn wheel_delta_advances_offset_clamped_to_max() {
    let mut ui = ui_at(SURFACE);
    build(&mut ui, 200.0, 800.0);
    ui.end_frame();

    // Pointer over scroll viewport (root vstack starts at (0,0); scroll is
    // the only child; viewport is the top 200px of the surface).
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    ui.on_input(InputEvent::Scroll(Vec2::new(0.0, 50.0)));

    ui.begin_frame(surface_display());
    build(&mut ui, 200.0, 800.0);
    ui.end_frame();

    assert_eq!(
        read_state(&mut ui).offset,
        50.0,
        "wheel delta accumulates into offset",
    );

    // Huge wheel push → clamps to (content - viewport) = 600.
    ui.on_input(InputEvent::Scroll(Vec2::new(0.0, 9_999.0)));
    ui.begin_frame(surface_display());
    build(&mut ui, 200.0, 800.0);
    ui.end_frame();

    assert_eq!(
        read_state(&mut ui).offset,
        600.0,
        "offset clamps to content_h - viewport_h",
    );
}

#[test]
fn non_overflowing_content_keeps_offset_at_zero() {
    let mut ui = ui_at(SURFACE);
    build(&mut ui, 300.0, 100.0);
    ui.end_frame();

    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    ui.on_input(InputEvent::Scroll(Vec2::new(0.0, 500.0)));

    ui.begin_frame(surface_display());
    build(&mut ui, 300.0, 100.0);
    ui.end_frame();

    assert_eq!(
        read_state(&mut ui).offset,
        0.0,
        "wheel input over non-overflowing content has nowhere to go",
    );
}
