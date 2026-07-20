use crate::input::{InputEvent, InputState, wheel_zoom_factor, zoom_factor_is_valid};
use crate::primitives::widget_id::WidgetId;
use crate::scene::cascade::Cascades;

fn pinch_state() -> InputState {
    InputState {
        pinch_target: Some(pinch_id()),
        ..InputState::default()
    }
}

fn pinch_id() -> WidgetId {
    WidgetId::from_hash("pinch")
}

#[test]
fn native_zoom_ingress_rejects_every_invalid_factor_class() {
    let mut state = pinch_state();
    let cascades = Cascades::default();

    for factor in [0.0, -0.0, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let delta = state.on_input(InputEvent::Zoom(factor), &cascades);
        assert!(!delta.requests_repaint, "invalid factor {factor:?}");
        assert_eq!(
            state.scroll_delta_for(pinch_id()).zoom,
            1.0,
            "invalid factor {factor:?}"
        );
        assert!(state.frame_target_deltas.is_empty());
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
    assert!((state.scroll_delta_for(pinch_id()).zoom - 1.155).abs() < 1e-5);
}

#[test]
fn long_valid_pinch_and_wheel_sequences_remain_positive_and_finite() {
    let cascades = Cascades::default();
    for factor in [1.1, 0.9] {
        let mut state = pinch_state();
        for _ in 0..10_000 {
            state.on_input(InputEvent::Zoom(factor), &cascades);
            assert!(zoom_factor_is_valid(
                state.scroll_delta_for(pinch_id()).zoom
            ));
        }
        let expected = if factor > 1.0 {
            f32::MAX
        } else {
            f32::MIN_POSITIVE
        };
        assert_eq!(state.scroll_delta_for(pinch_id()).zoom, expected);
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
    assert!((state.scroll_delta_for(pinch_id()).zoom - 1.2).abs() < 1e-5);
    state.end_frame(&cascades);
    assert_eq!(state.scroll_delta_for(pinch_id()).zoom, 1.0);
    assert!(state.frame_target_deltas.is_empty());
}
