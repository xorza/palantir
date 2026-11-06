use crate::input::Input;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::input_event::InputEvent;
use crate::primitives::input::zoom_factor::ZoomFactor;

fn pinch_state() -> Input {
    let mut state = Input::new();
    state.pointer.pinch_target = Some(pinch_id());
    state
}

fn pinch_id() -> WidgetId {
    WidgetId::from_hash("pinch")
}

#[test]
fn pinch_gesture_accumulates_zoom_delta() {
    let mut state = pinch_state();
    state.feed(InputEvent::Pinch(ZoomFactor::new(1.1).unwrap()));
    state.feed(InputEvent::Pinch(ZoomFactor::new(1.05).unwrap()));
    // `combine` multiplies in f64 and rounds the product once, back to f32.
    let product = (f64::from(1.1f32) * f64::from(1.05f32)) as f32;
    assert_eq!(state.scroll_delta_for(pinch_id()).zoom.get(), product);
}

#[test]
fn post_record_resets_zoom_delta_to_identity() {
    let mut state = pinch_state();
    state.feed(InputEvent::Pinch(ZoomFactor::new(1.2).unwrap()));
    assert_eq!(state.scroll_delta_for(pinch_id()).zoom.get(), 1.2);
    state.end_frame();
    assert_eq!(state.scroll_delta_for(pinch_id()).zoom.get(), 1.0);
    assert!(state.pointer.target_deltas.is_empty());
}
