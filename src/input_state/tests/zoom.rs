use crate::cascade::Cascade;
use crate::input::input_event::InputEvent;
use crate::input_state::InputState;
use crate::primitives::identity::widget_id::WidgetId;

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
fn pinch_gesture_accumulates_zoom_delta() {
    let mut state = pinch_state();
    state.feed(InputEvent::Zoom(1.1));
    state.feed(InputEvent::Zoom(1.05));
    // `combine` multiplies in f64 and rounds the product once, back to f32.
    let product = (f64::from(1.1f32) * f64::from(1.05f32)) as f32;
    assert_eq!(state.scroll_delta_for(pinch_id()).zoom.get(), product);
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
