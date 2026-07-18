use crate::input::{InputEvent, InputState};
use crate::primitives::widget_id::WidgetId;
use crate::ui::cascade::Cascades;
use glam::Vec2;
use winit::dpi::PhysicalPosition;
use winit::event::{DeviceId, MouseScrollDelta, TouchPhase, WindowEvent};

fn wheel(delta: MouseScrollDelta) -> WindowEvent {
    WindowEvent::MouseWheel {
        device_id: DeviceId::dummy(),
        delta,
        phase: TouchPhase::Moved,
    }
}

#[test]
fn from_winit_line_delta_emits_scroll_lines_with_flipped_signs() {
    // winit's +y wheel = rotation away from user = scroll up; +x wheel
    // = swipe right (reveal content right = pan offset forward). We flip
    // both so aperture's +delta means "advance the scroll offset." Line
    // count flows through unscaled — the consuming widget multiplies by
    // its own font-derived line step.
    let mut got = None;
    InputEvent::from_winit(&wheel(MouseScrollDelta::LineDelta(2.0, 1.0)), 1.0, |ev| {
        got = Some(ev);
    });
    match got.expect("wheel produces a Scroll event") {
        InputEvent::ScrollLines(d) => assert_eq!(d, Vec2::new(-2.0, -1.0)),
        ev => panic!("expected ScrollLines, got {ev:?}"),
    }
}

#[test]
fn scroll_delta_for_preserves_raw_pixels_and_lines() {
    let mut state = InputState::default();
    let cascades = Cascades::default();
    let id = WidgetId::from_hash("scroll");
    state.scroll_target = Some(id);
    state.on_input(InputEvent::ScrollPixels(Vec2::new(0.0, 5.0)), &cascades);
    state.on_input(InputEvent::ScrollLines(Vec2::new(0.0, 2.0)), &cascades);
    let delta = state.scroll_delta_for(id);
    assert_eq!(delta.pixels, Vec2::new(0.0, 5.0));
    assert_eq!(delta.lines, Vec2::new(0.0, 2.0));
    assert_eq!(delta.zoom, 1.0);
}

#[test]
fn from_winit_pixel_delta_divides_by_scale_factor_and_flips_both_axes() {
    let mut got = None;
    InputEvent::from_winit(
        &wheel(MouseScrollDelta::PixelDelta(PhysicalPosition::new(
            60.0, -120.0,
        ))),
        2.0,
        |ev| got = Some(ev),
    );
    match got.expect("pixel-delta wheel produces a Scroll event") {
        InputEvent::ScrollPixels(d) => {
            // x: -(60 / 2) = -30. y: -(-120 / 2) = 60.
            assert_eq!(d, Vec2::new(-30.0, 60.0));
        }
        ev => panic!("expected Scroll, got {ev:?}"),
    }
}

#[test]
fn on_input_accumulates_scroll_delta() {
    let mut state = InputState::default();
    let cascades = Cascades::default();
    let id = WidgetId::from_hash("scroll");
    state.scroll_target = Some(id);
    state.on_input(InputEvent::ScrollPixels(Vec2::new(0.0, 40.0)), &cascades);
    state.on_input(InputEvent::ScrollPixels(Vec2::new(5.0, -10.0)), &cascades);
    assert_eq!(state.scroll_delta_for(id).pixels, Vec2::new(5.0, 30.0));
}

#[test]
fn end_frame_clears_target_deltas_without_releasing_capacity() {
    let mut state = InputState::default();
    let cascades = Cascades::default();
    for index in 0..8 {
        state.scroll_target = Some(WidgetId::from_hash(("scroll", index)));
        state.on_input(InputEvent::ScrollPixels(Vec2::ONE), &cascades);
    }
    assert_eq!(state.frame_target_deltas.len(), 8);
    let capacity = state.frame_target_deltas.capacity();

    state.end_frame(&cascades);
    assert!(state.frame_target_deltas.is_empty());
    assert_eq!(state.frame_target_deltas.capacity(), capacity);

    for index in 0..8 {
        state.scroll_target = Some(WidgetId::from_hash(("next", index)));
        state.on_input(InputEvent::ScrollLines(Vec2::new(0.0, 1.0)), &cascades);
    }
    assert_eq!(state.frame_target_deltas.len(), 8);
    assert_eq!(state.frame_target_deltas.capacity(), capacity);
}
