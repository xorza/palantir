use crate::Ui;
use crate::input::capture::{Press, PressDrag, Release, ReleaseKind};
use crate::input::input_state::InputState;
use crate::input::pointer::PointerButton;
use crate::input::response::button_phase::ButtonPhase;
use crate::input::response::button_state::ButtonState;
use crate::input::response::drag::Drag;
use crate::input::response::response_state::ResponseState;
use crate::input::response::scroll_delta::ScrollDelta;
use crate::input::target_scroll_delta::TargetScrollDelta;
use crate::input::zoom_factor::ZoomFactor;
use crate::layout::types::sizing::Sizing;
use crate::primitives::rect::Rect;
use crate::primitives::translate_scale::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use crate::scene::cascade::Cascade;
use crate::ui::harness::UiHarness;
use crate::widgets::button::Button;
use crate::widgets::configure::Configure;
use crate::widgets::frame::Frame;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

fn focusable_id() -> WidgetId {
    WidgetId::from_hash("focusable")
}

fn build_focusable_leaf(ui: &mut Ui) {
    Frame::new()
        .id(WidgetId::from_hash("focusable"))
        .focusable(true)
        .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
        .show(ui);
}

#[test]
fn focused_reflects_focused_id_synchronously() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(build_focusable_leaf);
    assert!(!h.ui.response_for(focusable_id()).focused);

    h.request_focus(Some(focusable_id()));
    assert!(
        h.ui.response_for(focusable_id()).focused,
        "focused must be true the same frame as request_focus",
    );

    h.request_focus(None);
    assert!(!h.ui.response_for(focusable_id()).focused);
}

#[test]
fn disabled_reflects_cascaded_ancestor_flag() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    let build = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("parent"))
            .disabled(true)
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("child"))
                    .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
                    .show(ui);
            });
    };
    h.frame(build);
    h.frame(build);

    let parent_state = h.ui.response_for(WidgetId::from_hash("parent"));
    let child_state = h.ui.response_for(WidgetId::from_hash("child"));
    assert!(parent_state.disabled);
    assert!(
        child_state.disabled,
        "child inherits cascaded disabled from parent (no self flag)",
    );
}

/// Folding a disabled bit in takes the interaction half with it, and
/// every fold does — not only the last.
///
/// Three sources reach a widget's state at three different times, and
/// only `Widget::response` can see the third (the node's own flag). A
/// reset that ran between the second and the third left a widget
/// disabled *this* frame reporting `disabled: true` beside `hovered` and
/// `left.clicked()` — a pair the steady-state hit index can never
/// produce, because a disabled entry carries `Sense::NONE` and leaves the
/// index entirely.
///
/// Geometry stays: `pointer_local` is where the cursor is relative to the
/// widget, which `Ui::peek_pointer_local` answers whatever the widget is
/// allowed to do about it.
#[test]
fn folding_disabled_in_drops_the_interaction_half_at_every_fold() {
    let busy = ResponseState {
        rect: Some(Rect::new(1.0, 2.0, 3.0, 4.0)),
        layout_rect: Some(Rect::new(5.0, 6.0, 7.0, 8.0)),
        pointer_local: Some(Vec2::new(9.0, 10.0)),
        hovered: true,
        focused: true,
        left: ButtonState::new(ButtonPhase::Up { click: Some(1) }, Drag::None),
        scroll: ScrollDelta {
            pixels: Vec2::new(11.0, 12.0),
            ..ScrollDelta::default()
        },
        ..ResponseState::default()
    };

    // A `false` fold changes nothing at all.
    let mut kept = busy;
    kept.merge_disabled(false);
    assert!(!kept.disabled);
    assert!(kept.hovered && kept.left.clicked());

    // The cascade's fold clears, and the node's later fold finds nothing
    // left to clear — which is the point: the order of the three sources
    // stops mattering.
    let mut off = busy;
    off.merge_disabled(true);
    off.merge_disabled(false);
    assert!(off.disabled, "a later `false` cannot re-enable it");
    assert!(!off.hovered, "the hover goes with it");
    assert!(!off.left.clicked(), "and the click");
    assert_eq!(off.left, ButtonState::default());
    assert_eq!(off.right, ButtonState::default());
    assert_eq!(off.middle, ButtonState::default());
    assert_eq!(off.scroll, ScrollDelta::default(), "and the wheel");

    // Geometry is untouched, `focused` included — a disabled widget still
    // has a rect, and still knows where the cursor is over it.
    assert_eq!(off.rect, busy.rect);
    assert_eq!(off.layout_rect, busy.layout_rect);
    assert_eq!(off.pointer_local, busy.pointer_local);
    assert!(off.focused);
}

#[test]
fn disabled_false_when_chain_clean() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    let build = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("parent"))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("child"))
                    .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
                    .show(ui);
            });
    };
    h.frame(build);
    h.frame(build);
    assert!(!h.ui.response_for(WidgetId::from_hash("child")).disabled);
}

/// A pointer that left takes every routed target with it:
/// `refresh_pointer_targets` is the only writer of the three, and it
/// clears them all when `pointer_pos` is `None`.
///
/// This is what lets `snapshot_frame_quiescent` read one pointer test
/// where four would otherwise be needed — see there. The setting half of
/// the same method, where a hit test fills the three in, is covered by
/// the routing tests that drive a real tree.
#[test]
fn a_departed_pointer_clears_every_routed_target() {
    let id = WidgetId::from_hash("w");
    let mut s = InputState {
        pointer_pos: None,
        hovered: Some(id),
        scroll_target: Some(id),
        pinch_target: Some(id),
        ..Default::default()
    };

    s.refresh_pointer_targets(&Cascade::default());
    assert_eq!(
        (s.hovered, s.scroll_target, s.pinch_target),
        (None, None, None),
        "a pointer that left takes every routed target with it",
    );
}

/// The once-per-frame quiescence predicate that gates `response_for`'s
/// fast path: every pointer/capture-derived signal flips it false, but
/// `focused` deliberately does not (it can be set mid-record).
///
/// `hovered` / `scroll_target` / `pinch_target` are not among the signals
/// tested, and cannot be: `refresh_pointer_targets` clears all three
/// whenever the pointer leaves, so a routed target without a pointer is
/// a state nothing can reach. The invariant is asserted below, and
/// `a_departed_pointer_clears_every_routed_target` pins its source.
#[test]
fn frame_quiescent_predicate() {
    // Fresh state, one mutation, snapshot — returns the sealed flag.
    let quiescent = |mutate: &dyn Fn(&mut InputState)| {
        let mut s = InputState::default();
        mutate(&mut s);
        s.snapshot_frame_quiescent();
        s.frame_quiescent
    };
    assert!(
        quiescent(&|_| {}),
        "a fresh input state (no pointer, no captures) is quiescent",
    );

    let id = WidgetId::from_hash("w");
    // Each pointer / routing / capture signal independently breaks
    // quiescence.
    let broken = |label: &str, mutate: &dyn Fn(&mut InputState)| {
        assert!(!quiescent(mutate), "{label} must break quiescence");
    };
    broken("pointer_pos", &|s| s.pointer_pos = Some(Vec2::ZERO));
    broken("frame_target_deltas", &|s| {
        s.frame_target_deltas.push(TargetScrollDelta::new(id))
    });
    broken("capture.press", &|s| {
        s.captures[PointerButton::Left.idx()].press = Some(Press {
            target: id,
            origin: Vec2::ZERO,
            travel: Vec2::ZERO,
            count: 1,
            fresh: true,
            drag: PressDrag::None,
        })
    });
    broken("capture.release (click)", &|s| {
        s.captures[PointerButton::Right.idx()].release = Some(Release {
            target: id,
            kind: ReleaseKind::Click { count: 1 },
        })
    });
    broken("capture.release (miss)", &|s| {
        s.captures[PointerButton::Middle.idx()].release = Some(Release {
            target: id,
            kind: ReleaseKind::Miss,
        })
    });

    // `focused` is excluded: a focused widget on an otherwise idle frame
    // stays quiescent so the fast path still applies.
    assert!(
        quiescent(&|s| s.focused = Some(id)),
        "focus alone must NOT break quiescence (read live on the fast path)",
    );
}

fn button_surface() -> UVec2 {
    UVec2::new(200, 80)
}

fn build_button(id: WidgetId) -> impl FnMut(&mut Ui) {
    move |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Button::new()
                .id(id)
                .label("hi")
                .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                .show(ui);
        });
    }
}

/// On a quiescent frame (no pointer ever fed) `response_for` takes the
/// geometry-only fast path: the arranged rect survives but every
/// interaction field reads its default.
#[test]
fn quiescent_frame_keeps_geometry_defaults_interaction() {
    let mut h = UiHarness::new(button_surface());
    let id = WidgetId::from_hash("btn");
    // No pointer is ever fed → the frame is quiescent, so the snapshot
    // taken at record-pass start stays valid for this post-frame read.
    h.frame(build_button(id));

    let r = h.ui.response_for(id);
    let rect = r
        .rect
        .expect("arranged rect present on the quiescent fast path");
    assert_eq!(rect.size.w, 100.0);
    assert_eq!(rect.size.h, 40.0);
    assert!(r.layout_rect.is_some());

    assert!(!r.hovered);
    assert!(!r.pressed());
    assert!(!r.left.clicked());
    assert!(!r.right.clicked());
    assert!(!r.focused);
    assert!(!r.left.drag.dragging());
    assert_eq!(r.left.click_count(), 0);
    assert_eq!(r.scroll.pixels, Vec2::ZERO);
    assert_eq!(r.scroll.lines, Vec2::ZERO);
    assert_eq!(r.scroll.zoom, ZoomFactor::ONE);
    assert_eq!(r.pointer_local, None);
}

/// With the pointer resting over a widget the frame is non-quiescent, so
/// `response_for` runs the full interaction path and computes the
/// pre-transform widget-local pointer.
#[test]
fn non_quiescent_frame_computes_interaction() {
    let mut h = UiHarness::new(button_surface());
    let id = WidgetId::from_hash("btn");
    h.frame(build_button(id));

    let pointer = Vec2::new(50.0, 20.0);
    h.move_to(pointer);
    // Run a frame *after* the pointer event so the snapshot reflects it,
    // then read — the pointer makes the frame non-quiescent (full path).
    h.frame(build_button(id));

    let r = h.ui.response_for(id);
    let layout_rect = r.layout_rect.expect("arranged layout rect present");
    assert!(
        r.hovered,
        "pointer resting inside the button rect hovers it"
    );
    assert_eq!(
        r.pointer_local,
        Some(pointer - layout_rect.min),
        "pointer_local is the cursor offset from layout_rect.min",
    );
}

#[test]
fn pointer_and_drag_vectors_are_scale_invariant() {
    let id = WidgetId::from_hash("scaled-button");
    let local_pointer = Vec2::new(25.0, 10.0);
    let local_delta = Vec2::new(12.0, -8.0);

    for scale in [0.5, 1.0, 2.0] {
        let mut h = UiHarness::new(UVec2::new(300, 200));
        let build = |ui: &mut Ui| {
            Panel::zstack()
                .id(WidgetId::from_hash("scaled-parent"))
                .transform(TranslateScale::from_scale(scale))
                .size((Sizing::fixed(120.0), Sizing::fixed(60.0)))
                .show(ui, |ui| {
                    Button::new()
                        .id(id)
                        .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                        .show(ui);
                });
        };
        h.frame(build);

        let arranged = h.ui.response_for(id);
        let layout = arranged.layout_rect.expect("button arranged");
        let press = arranged.transform.apply_point(layout.min + local_pointer);
        let pointer = arranged
            .transform
            .apply_point(layout.min + local_pointer + local_delta);
        h.press_at(press);
        h.move_to(pointer);
        h.frame(build);

        let response = h.ui.response_for(id);
        assert_eq!(
            response.pointer_local,
            Some(local_pointer + local_delta),
            "pointer position at {scale}×",
        );
        assert_eq!(
            response.left.drag.delta(),
            Some(local_delta),
            "drag vector at {scale}×",
        );
    }
}

#[test]
fn pointer_local_uses_unclipped_widget_origin() {
    let mut h = UiHarness::new(button_surface());
    let id = WidgetId::from_hash("clipped-child");
    let build = |ui: &mut Ui| {
        Panel::canvas()
            .id(WidgetId::from_hash("clipper"))
            .clip_rect()
            .size((Sizing::fixed(50.0), Sizing::fixed(40.0)))
            .show(ui, |ui| {
                Frame::new()
                    .id(id)
                    .position(Vec2::new(-20.0, 0.0))
                    .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                    .show(ui);
            });
    };
    h.frame(build);

    let arranged = h.ui.response_for(id);
    let visible = arranged.rect.expect("child visible through clip");
    let layout = arranged.layout_rect.expect("child arranged");
    let surface_origin = arranged.transform.apply_point(layout.min);
    assert_ne!(
        visible.min, surface_origin,
        "the clip must trim the widget's leading edge",
    );

    let pointer = visible.min + Vec2::new(10.0, 10.0);
    h.move_to(pointer);
    h.frame(build);
    let response = h.ui.response_for(id);
    assert_eq!(
        response.pointer_local,
        Some(response.transform.inverse_vector(pointer - surface_origin)),
    );
}

/// The quiescent and non-quiescent paths must agree on every field they
/// both own — the geometry half plus `focused`. They used to be two
/// separate `ResponseState` constructions, so a field filled on one path
/// could be silently defaulted on the other; this pins that they don't
/// diverge.
///
/// Driven by toggling *only* `frame_quiescent`, with the same widget and
/// the same cascade underneath, so any difference is attributable to the
/// path taken rather than to the state it read.
#[test]
fn quiescent_and_full_paths_agree_on_geometry() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(build_focusable_leaf);

    // Drive `InputState` directly against the harness's frozen cascade +
    // layout: `Ui::response_for` owns the quiescent snapshot, and the
    // point here is to flip that one bit with everything else held equal.
    let mut s = InputState {
        focused: Some(focusable_id()),
        ..InputState::default()
    };

    s.frame_quiescent = true;
    let quiet = s.response_for(focusable_id(), h.ui.cascade(), h.ui.layout_tables());
    s.frame_quiescent = false;
    let full = s.response_for(focusable_id(), h.ui.cascade(), h.ui.layout_tables());

    assert_eq!(quiet.rect, full.rect);
    assert_eq!(quiet.layout_rect, full.layout_rect);
    assert_eq!(quiet.transform, full.transform);
    assert_eq!(quiet.disabled, full.disabled);
    assert_eq!(quiet.focused, full.focused);
    assert!(
        quiet.rect.is_some(),
        "fixture must actually arrange, or the comparison is vacuous",
    );
    assert!(quiet.focused, "focus must survive the quiescent path");
}
