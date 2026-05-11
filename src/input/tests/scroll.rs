use crate::input::{InputEvent, InputState};
use crate::ui::cascade::CascadeResult;
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
fn from_winit_line_delta_scales_by_step_pixels_and_flips_both_axes() {
    // winit's +y wheel = rotation away from user = scroll up; +x wheel
    // = swipe right (reveal content right = pan offset forward). We flip
    // both so palantir's +delta means "advance the scroll offset."
    let ev = InputEvent::from_winit(&wheel(MouseScrollDelta::LineDelta(2.0, 1.0)), 1.0)
        .expect("wheel produces a Scroll event");
    match ev {
        InputEvent::Scroll(d) => {
            assert_eq!(d.x, -80.0, "2 lines right → -2·SCROLL_LINE_PIXELS");
            assert_eq!(d.y, -40.0, "1 line up → -SCROLL_LINE_PIXELS");
        }
        _ => panic!("expected Scroll, got {ev:?}"),
    }
}

#[test]
fn from_winit_pixel_delta_divides_by_scale_factor_and_flips_both_axes() {
    let ev = InputEvent::from_winit(
        &wheel(MouseScrollDelta::PixelDelta(PhysicalPosition::new(
            60.0, -120.0,
        ))),
        2.0,
    )
    .expect("pixel-delta wheel produces a Scroll event");
    match ev {
        InputEvent::Scroll(d) => {
            // x: -(60 / 2) = -30. y: -(-120 / 2) = 60.
            assert_eq!(d, Vec2::new(-30.0, 60.0));
        }
        _ => panic!("expected Scroll, got {ev:?}"),
    }
}

#[test]
fn on_input_accumulates_scroll_delta() {
    let mut state = InputState::new();
    let cascades = CascadeResult::default();
    state.on_input(InputEvent::Scroll(Vec2::new(0.0, 40.0)), &cascades);
    state.on_input(InputEvent::Scroll(Vec2::new(5.0, -10.0)), &cascades);
    assert_eq!(state.frame_scroll_delta, Vec2::new(5.0, 30.0));
}

#[test]
fn pinch_gesture_accumulates_zoom_delta() {
    let mut state = InputState::new();
    let cascades = CascadeResult::default();
    state.on_input(InputEvent::Zoom(1.1), &cascades);
    state.on_input(InputEvent::Zoom(1.05), &cascades);
    assert!((state.frame_zoom_delta - 1.155).abs() < 1e-5);
}

#[test]
fn post_record_resets_zoom_delta_to_identity() {
    let mut state = InputState::new();
    let cascades = CascadeResult::default();
    state.on_input(InputEvent::Zoom(1.2), &cascades);
    assert!((state.frame_zoom_delta - 1.2).abs() < 1e-5);
    state.post_record(&cascades);
    assert_eq!(state.frame_zoom_delta, 1.0);
}

#[test]
fn post_record_clears_scroll_delta() {
    let mut state = InputState::new();
    let cascades = CascadeResult::default();
    state.on_input(InputEvent::Scroll(Vec2::new(7.0, 7.0)), &cascades);
    assert_eq!(state.frame_scroll_delta, Vec2::new(7.0, 7.0));
    state.post_record(&cascades);
    assert_eq!(state.frame_scroll_delta, Vec2::ZERO);
}
