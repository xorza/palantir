use crate::input::input_event::InputEvent;
use crate::input::input_state::InputState;
use crate::input::policy::InputSignal;
use crate::input::zoom_factor::ZoomFactor;
use crate::primitives::widget_id::WidgetId;
use crate::scene::cascade::Cascade;

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

    for factor in [0.0, -0.0, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let delta = state.feed(InputEvent::Zoom(factor));
        assert!(!delta.requests_repaint, "invalid factor {factor:?}");
        assert_eq!(
            state.scroll_delta_for(pinch_id()).zoom.get(),
            1.0,
            "invalid factor {factor:?}"
        );
        assert!(state.frame_target_deltas.is_empty());
        assert_eq!(
            state.signal_since_last_frame,
            InputSignal::None,
            "invalid factor {factor:?}"
        );
    }
}

#[test]
fn pinch_gesture_accumulates_zoom_delta() {
    let mut state = pinch_state();
    state.feed(InputEvent::Zoom(1.1));
    state.feed(InputEvent::Zoom(1.05));
    // `combine` multiplies in f64 and rounds the product once, back to f32.
    let product = (f64::from(1.1f32) * f64::from(1.05f32)) as f32;
    assert_eq!(state.scroll_delta_for(pinch_id()).zoom.get(), product);
}

#[test]
fn long_valid_pinch_and_wheel_sequences_remain_positive_and_finite() {
    for factor in [1.1, 0.9] {
        let mut state = pinch_state();
        for _ in 0..10_000 {
            state.feed(InputEvent::Zoom(factor));
            assert!(ZoomFactor::new(state.scroll_delta_for(pinch_id()).zoom.get()).is_some());
        }
        let expected = if factor > 1.0 {
            f32::MAX
        } else {
            f32::MIN_POSITIVE
        };
        assert_eq!(state.scroll_delta_for(pinch_id()).zoom.get(), expected);
    }

    for direction in [-1.0, 1.0] {
        let mut notches = 0.0;
        let mut factor = 1.0;
        for _ in 0..10_000 {
            notches += direction;
            factor = ZoomFactor::from_wheel(1.03, notches).get();
            assert!(ZoomFactor::new(factor).is_some());
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
    let cascade = Cascade::default();
    state.feed(InputEvent::Zoom(1.2));
    assert_eq!(state.scroll_delta_for(pinch_id()).zoom.get(), 1.2);
    state.end_frame(&cascade);
    assert_eq!(state.scroll_delta_for(pinch_id()).zoom.get(), 1.0);
    assert!(state.frame_target_deltas.is_empty());
}
