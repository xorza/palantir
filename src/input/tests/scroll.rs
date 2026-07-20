use crate::input::{InputEvent, InputState};
use crate::primitives::widget_id::WidgetId;
use crate::ui::cascade::Cascades;
use glam::Vec2;

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
