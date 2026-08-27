use crate::input::input_event::InputEvent;
use crate::input::input_state::InputState;
use crate::primitives::widget_id::WidgetId;
use crate::scene::cascade::Cascade;
use glam::Vec2;

#[test]
fn scroll_delta_for_preserves_raw_pixels_and_lines() {
    let mut state = InputState::default();
    let cascade = Cascade::default();
    let id = WidgetId::from_hash("scroll");
    state.scroll_target = Some(id);
    state.on_input(InputEvent::ScrollPixels(Vec2::new(0.0, 5.0)), &cascade);
    state.on_input(InputEvent::ScrollLines(Vec2::new(0.0, 2.0)), &cascade);
    let delta = state.scroll_delta_for(id);
    assert_eq!(delta.pixels, Vec2::new(0.0, 5.0));
    assert_eq!(delta.lines, Vec2::new(0.0, 2.0));
    assert_eq!(delta.zoom, 1.0);
}

#[test]
fn on_input_accumulates_scroll_delta() {
    let mut state = InputState::default();
    let cascade = Cascade::default();
    let id = WidgetId::from_hash("scroll");
    state.scroll_target = Some(id);
    state.on_input(InputEvent::ScrollPixels(Vec2::new(0.0, 40.0)), &cascade);
    state.on_input(InputEvent::ScrollPixels(Vec2::new(5.0, -10.0)), &cascade);
    assert_eq!(state.scroll_delta_for(id).pixels, Vec2::new(5.0, 30.0));
}

#[test]
fn end_frame_clears_target_deltas_without_releasing_capacity() {
    let mut state = InputState::default();
    let cascade = Cascade::default();
    for index in 0..8 {
        state.scroll_target = Some(WidgetId::from_hash(("scroll", index)));
        state.on_input(InputEvent::ScrollPixels(Vec2::ONE), &cascade);
    }
    assert_eq!(state.frame_target_deltas.len(), 8);
    let capacity = state.frame_target_deltas.capacity();

    state.end_frame(&cascade);
    assert!(state.frame_target_deltas.is_empty());
    assert_eq!(state.frame_target_deltas.capacity(), capacity);

    for index in 0..8 {
        state.scroll_target = Some(WidgetId::from_hash(("next", index)));
        state.on_input(InputEvent::ScrollLines(Vec2::new(0.0, 1.0)), &cascade);
    }
    assert_eq!(state.frame_target_deltas.len(), 8);
    assert_eq!(state.frame_target_deltas.capacity(), capacity);
}

/// A non-finite payload is refused at the door, so it never reaches
/// retained state.
///
/// It used to reach it: `pan_delta.x != 0.0` is true for NaN,
/// `f32::clamp` hands a NaN input straight back, and the poisoned scroll
/// offset then failed `TranslateScale::new`'s finite-translation assert a
/// pass later — a release panic, from whatever the platform put on the
/// wheel. The accumulated delta and the pointer position both have to
/// survive the refusal untouched, since a rejected event mutates nothing.
#[test]
fn non_finite_payloads_are_refused_before_they_reach_retained_state() {
    let mut state = InputState::default();
    let cascade = Cascade::default();
    let id = WidgetId::from_hash("scroll");
    // The pointer move first: it re-resolves the targets against the
    // cascade, and this one is empty. Routing is stamped in after it, the
    // way the other cases here do.
    state.on_input(InputEvent::PointerMoved(Vec2::new(7.0, 11.0)), &cascade);
    state.scroll_target = Some(id);
    state.on_input(InputEvent::ScrollPixels(Vec2::new(0.0, 5.0)), &cascade);
    state.on_input(InputEvent::ScrollLines(Vec2::new(1.0, 0.0)), &cascade);

    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for axis in [Vec2::new(bad, 0.0), Vec2::new(0.0, bad)] {
            for event in [
                InputEvent::ScrollPixels(axis),
                InputEvent::ScrollLines(axis),
                InputEvent::PointerMoved(axis),
            ] {
                assert!(
                    !state.on_input(event, &cascade).requests_repaint,
                    "{event:?} must be refused",
                );
            }
        }
    }

    let delta = state.scroll_delta_for(id);
    assert_eq!(delta.pixels, Vec2::new(0.0, 5.0), "good pixels stand");
    assert_eq!(delta.lines, Vec2::new(1.0, 0.0), "good lines stand");
    assert_eq!(state.pointer_pos, Some(Vec2::new(7.0, 11.0)));
    // A refused pointer move re-resolves nothing either, so the routing
    // the good events accumulated against is still the routing in force.
    assert_eq!(state.scroll_target, Some(id));
}
