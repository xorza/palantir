use crate::input::{InputEvent, InputState, wheel_zoom_factor, zoom_factor_is_valid};
use crate::primitives::widget_id::WidgetId;
use crate::ui::cascade::Cascades;
use winit::event::{DeviceId, TouchPhase, WindowEvent};

fn pinch(delta: f64) -> WindowEvent {
    WindowEvent::PinchGesture {
        device_id: DeviceId::dummy(),
        delta,
        phase: TouchPhase::Moved,
    }
}

fn pinch_state() -> InputState {
    InputState {
        pinch_target: Some(WidgetId::from_hash("pinch")),
        ..InputState::default()
    }
}

#[test]
fn from_winit_emits_only_positive_finite_zoom_factors() {
    let mut emitted = None;
    InputEvent::from_winit(&pinch(0.5), 1.0, |event| emitted = Some(event));
    assert!(matches!(emitted, Some(InputEvent::Zoom(1.5))));

    for delta in [-1.0, -2.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut count = 0;
        InputEvent::from_winit(&pinch(delta), 1.0, |_| count += 1);
        assert_eq!(count, 0, "invalid pinch delta {delta:?} emitted an event");
    }
}

#[test]
fn native_zoom_ingress_rejects_every_invalid_factor_class() {
    let mut state = pinch_state();
    let cascades = Cascades::default();

    for factor in [0.0, -0.0, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let delta = state.on_input(InputEvent::Zoom(factor), &cascades);
        assert!(!delta.requests_repaint, "invalid factor {factor:?}");
        assert_eq!(state.frame_zoom_delta, 1.0, "invalid factor {factor:?}");
        assert!(
            !state.had_input_since_last_frame,
            "invalid factor {factor:?}"
        );
    }
}

#[test]
fn pinch_gesture_accumulates_zoom_delta() {
    let mut state = pinch_state();
    let cascades = Cascades::default();
    state.on_input(InputEvent::Zoom(1.1), &cascades);
    state.on_input(InputEvent::Zoom(1.05), &cascades);
    assert!((state.frame_zoom_delta - 1.155).abs() < 1e-5);
}

#[test]
fn long_valid_pinch_and_wheel_sequences_remain_positive_and_finite() {
    let cascades = Cascades::default();
    for factor in [1.1, 0.9] {
        let mut state = pinch_state();
        for _ in 0..10_000 {
            state.on_input(InputEvent::Zoom(factor), &cascades);
            assert!(zoom_factor_is_valid(state.frame_zoom_delta));
        }
        let expected = if factor > 1.0 {
            f32::MAX
        } else {
            f32::MIN_POSITIVE
        };
        assert_eq!(state.frame_zoom_delta, expected);
    }

    for direction in [-1.0, 1.0] {
        let mut notches = 0.0;
        let mut factor = 1.0;
        for _ in 0..10_000 {
            notches += direction;
            factor = wheel_zoom_factor(1.03, notches);
            assert!(zoom_factor_is_valid(factor));
        }
        let expected = if direction < 0.0 {
            f32::MAX
        } else {
            f32::MIN_POSITIVE
        };
        assert_eq!(factor, expected);
    }
}

#[test]
fn post_record_resets_zoom_delta_to_identity() {
    let mut state = pinch_state();
    let cascades = Cascades::default();
    state.on_input(InputEvent::Zoom(1.2), &cascades);
    assert!((state.frame_zoom_delta - 1.2).abs() < 1e-5);
    state.end_frame(&cascades);
    assert_eq!(state.frame_zoom_delta, 1.0);
}
