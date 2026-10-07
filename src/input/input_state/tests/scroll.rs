use crate::cascade::Cascade;
use crate::input::input_event::InputEvent;
use crate::input::input_state::InputState;
use crate::input::scroll_targets::ScrollTargets;
use crate::input::zoom_factor::ZoomFactor;
use crate::primitives::identity::widget_id::WidgetId;
use glam::Vec2;

#[test]
fn scroll_delta_for_preserves_raw_pixels_and_lines() {
    let mut state = InputState::default();
    let id = WidgetId::from_hash("scroll");
    state.scroll_targets = ScrollTargets::both(id);
    state.feed(InputEvent::ScrollPixels(Vec2::new(0.0, 5.0)));
    state.feed(InputEvent::ScrollLines(Vec2::new(0.0, 2.0)));
    let delta = state.scroll_delta_for(id);
    assert_eq!(delta.pixels, Vec2::new(0.0, 5.0));
    assert_eq!(delta.lines, Vec2::new(0.0, 2.0));
    assert_eq!(delta.zoom, ZoomFactor::ONE);
}

#[test]
fn on_input_accumulates_scroll_delta() {
    let mut state = InputState::default();
    let id = WidgetId::from_hash("scroll");
    state.scroll_targets = ScrollTargets::both(id);
    state.feed(InputEvent::ScrollPixels(Vec2::new(0.0, 40.0)));
    state.feed(InputEvent::ScrollPixels(Vec2::new(5.0, -10.0)));
    assert_eq!(state.scroll_delta_for(id).pixels, Vec2::new(5.0, 30.0));
}

#[test]
fn end_frame_clears_target_deltas_without_releasing_capacity() {
    let mut state = InputState::default();
    let cascade = Cascade::default();
    for index in 0..8 {
        state.scroll_targets = ScrollTargets::both(WidgetId::from_hash(("scroll", index)));
        state.feed(InputEvent::ScrollPixels(Vec2::ONE));
    }
    assert_eq!(state.frame_target_deltas.len(), 8);
    let capacity = state.frame_target_deltas.capacity();

    state.end_frame(&cascade);
    assert!(state.frame_target_deltas.is_empty());
    assert_eq!(state.frame_target_deltas.capacity(), capacity);

    for index in 0..8 {
        state.scroll_targets = ScrollTargets::both(WidgetId::from_hash(("next", index)));
        state.feed(InputEvent::ScrollLines(Vec2::new(0.0, 1.0)));
    }
    assert_eq!(state.frame_target_deltas.len(), 8);
    assert_eq!(state.frame_target_deltas.capacity(), capacity);
}

/// A non-finite payload is refused at the door and never reaches retained state: NaN passes `pan_delta.x != 0.0` and `f32::clamp`, and the poisoned offset later trips `TranslateScale::new`'s finite assert. A refused event mutates nothing.
#[test]
fn non_finite_payloads_are_refused_before_they_reach_retained_state() {
    let mut state = InputState::default();
    let id = WidgetId::from_hash("scroll");
    state.feed(InputEvent::PointerMoved(Vec2::new(7.0, 11.0)));
    state.scroll_targets = ScrollTargets::both(id);
    state.feed(InputEvent::ScrollPixels(Vec2::new(0.0, 5.0)));
    state.feed(InputEvent::ScrollLines(Vec2::new(1.0, 0.0)));

    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for axis in [Vec2::new(bad, 0.0), Vec2::new(0.0, bad)] {
            for event in [
                InputEvent::ScrollPixels(axis),
                InputEvent::ScrollLines(axis),
                InputEvent::PointerMoved(axis),
            ] {
                assert!(
                    !state.feed(event).repaint_requested,
                    "{event:?} must be refused",
                );
            }
        }
    }

    let delta = state.scroll_delta_for(id);
    assert_eq!(delta.pixels, Vec2::new(0.0, 5.0), "good pixels stand");
    assert_eq!(delta.lines, Vec2::new(1.0, 0.0), "good lines stand");
    assert_eq!(state.pointer_pos, Some(Vec2::new(7.0, 11.0)));
    assert_eq!(state.scroll_targets, ScrollTargets::both(id));
}
