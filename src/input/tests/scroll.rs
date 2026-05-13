use crate::input::{InputEvent, InputState};
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
    // both so palantir's +delta means "advance the scroll offset." Line
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
fn scroll_delta_for_combines_pixels_and_lines_by_line_step() {
    use crate::primitives::widget_id::WidgetId;
    let mut state = InputState::new();
    let cascades = Cascades::default();
    let id = WidgetId::from_hash("scroll");
    state.scroll_target = Some(id);
    state.on_input(InputEvent::ScrollPixels(Vec2::new(0.0, 5.0)), &cascades);
    state.on_input(InputEvent::ScrollLines(Vec2::new(0.0, 2.0)), &cascades);
    // 5 px + 2 lines × 19.2 px/line = 43.4 px.
    let d = state.scroll_delta_for(id, 19.2);
    assert!((d.y - 43.4).abs() < 1e-4, "got {d:?}");
    // 2 real lines + 5 px / 19.2 ≈ 2.2604 virtual notches.
    let n = state.scroll_notches_for(id, 19.2);
    assert!((n.y - (2.0 + 5.0 / 19.2)).abs() < 1e-4, "got {n:?}");
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
    let mut state = InputState::new();
    let cascades = Cascades::default();
    state.on_input(InputEvent::ScrollPixels(Vec2::new(0.0, 40.0)), &cascades);
    state.on_input(InputEvent::ScrollPixels(Vec2::new(5.0, -10.0)), &cascades);
    assert_eq!(state.frame_scroll_pixels, Vec2::new(5.0, 30.0));
}

#[test]
fn pinch_gesture_accumulates_zoom_delta() {
    let mut state = InputState::new();
    let cascades = Cascades::default();
    state.on_input(InputEvent::Zoom(1.1), &cascades);
    state.on_input(InputEvent::Zoom(1.05), &cascades);
    assert!((state.frame_zoom_delta - 1.155).abs() < 1e-5);
}

#[test]
fn post_record_resets_zoom_delta_to_identity() {
    let mut state = InputState::new();
    let cascades = Cascades::default();
    state.on_input(InputEvent::Zoom(1.2), &cascades);
    assert!((state.frame_zoom_delta - 1.2).abs() < 1e-5);
    state.post_record(&cascades);
    assert_eq!(state.frame_zoom_delta, 1.0);
}

#[test]
fn post_record_clears_scroll_delta() {
    let mut state = InputState::new();
    let cascades = Cascades::default();
    state.on_input(InputEvent::ScrollPixels(Vec2::new(7.0, 7.0)), &cascades);
    assert_eq!(state.frame_scroll_pixels, Vec2::new(7.0, 7.0));
    state.post_record(&cascades);
    assert_eq!(state.frame_scroll_pixels, Vec2::ZERO);
}
