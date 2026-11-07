use crate::input::Input;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::input_event::InputEvent;
use crate::primitives::input::policy::InputSignal;
use crate::primitives::input::scroll_targets::ScrollTargets;
use crate::primitives::input::zoom_factor::ZoomFactor;
use glam::Vec2;

#[test]
fn scroll_delta_for_preserves_raw_pixels_and_lines() {
    let mut state = Input::new();
    let id = WidgetId::from_hash("scroll");
    state.pointer.scroll_targets = ScrollTargets::both(id);
    state.feed(InputEvent::ScrollPixels(Vec2::new(0.0, 5.0)));
    state.feed(InputEvent::ScrollLines {
        notches: Vec2::new(0.0, 2.0),
        lines: Vec2::new(0.0, 6.0),
        pages: Vec2::new(0.0, 1.0),
    });
    let delta = state.scroll_delta_for(id);
    assert_eq!(delta.pixels, Vec2::new(0.0, 5.0));
    assert_eq!(delta.lines, Vec2::new(0.0, 6.0));
    assert_eq!(delta.notches, Vec2::new(0.0, 2.0));
    assert_eq!(delta.pages, Vec2::new(0.0, 1.0));
    assert_eq!(delta.zoom, ZoomFactor::ONE);
}

#[test]
fn on_input_accumulates_scroll_delta() {
    let mut state = Input::new();
    let id = WidgetId::from_hash("scroll");
    state.pointer.scroll_targets = ScrollTargets::both(id);
    state.feed(InputEvent::ScrollPixels(Vec2::new(0.0, 40.0)));
    state.feed(InputEvent::ScrollPixels(Vec2::new(5.0, -10.0)));
    assert_eq!(state.scroll_delta_for(id).pixels, Vec2::new(5.0, 30.0));
}

#[test]
fn end_frame_clears_target_deltas_without_releasing_capacity() {
    let mut state = Input::new();
    for idx in 0..8 {
        state.pointer.scroll_targets = ScrollTargets::both(WidgetId::from_hash(("scroll", idx)));
        state.feed(InputEvent::ScrollPixels(Vec2::ONE));
    }
    assert_eq!(state.pointer.target_deltas.len(), 8);
    let capacity = state.pointer.target_deltas.capacity();

    state.end_frame();
    assert!(state.pointer.target_deltas.is_empty());
    assert_eq!(state.pointer.target_deltas.capacity(), capacity);

    for idx in 0..8 {
        state.pointer.scroll_targets = ScrollTargets::both(WidgetId::from_hash(("next", idx)));
        state.feed(InputEvent::scroll_lines(Vec2::new(0.0, 1.0)));
    }
    assert_eq!(state.pointer.target_deltas.len(), 8);
    assert_eq!(state.pointer.target_deltas.capacity(), capacity);
}

/// Every invalid scalar payload is refused at the door and never reaches retained state: NaN passes `pan_delta.x != 0.0` and `f32::clamp`, and the poisoned offset later trips `TranslateScale::new`'s finite assert; a pinch's factor is a `ZoomFactor`, valid by construction. A refused event mutates nothing and asks for no frame.
#[test]
fn an_invalid_payload_is_refused_and_mutates_nothing() {
    let id = WidgetId::from_hash("scroll");
    let mut state = Input::new();
    state.feed(InputEvent::PointerMoved(Vec2::new(7.0, 11.0)));
    state.pointer.scroll_targets = ScrollTargets::both(id);
    state.pointer.pinch_target = Some(id);
    state.feed(InputEvent::ScrollPixels(Vec2::new(0.0, 5.0)));
    state.feed(InputEvent::scroll_lines(Vec2::new(1.0, 0.0)));
    state.feed(InputEvent::Pinch(ZoomFactor::new(1.5).unwrap()));
    state.signal_since_last_frame = InputSignal::None;

    let mut refused = Vec::new();
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for axis in [Vec2::new(bad, 0.0), Vec2::new(0.0, bad)] {
            refused.extend([
                InputEvent::ScrollPixels(axis),
                InputEvent::scroll_lines(axis),
                InputEvent::PointerMoved(axis),
            ]);
        }
    }
    for event in refused {
        assert!(!state.feed(event).repaint_requested, "{event:?}");
        assert_eq!(
            state.signal_since_last_frame,
            InputSignal::None,
            "{event:?}"
        );
    }

    let delta = state.scroll_delta_for(id);
    assert_eq!(delta.pixels, Vec2::new(0.0, 5.0), "good pixels stand");
    assert_eq!(delta.lines, Vec2::new(1.0, 0.0), "good lines stand");
    assert_eq!(delta.zoom.get(), 1.5, "the good factor stands");
    assert_eq!(state.pointer.pos, Some(Vec2::new(7.0, 11.0)));
    assert_eq!(state.pointer.scroll_targets, ScrollTargets::both(id));
}
