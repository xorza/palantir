use crate::host::harness::Harness;
use crate::input::Input;
use crate::input::capture::{Press, PressDrag, Release, ReleaseKind};
use crate::input::target_scroll_delta::TargetScrollDelta;
use crate::input::tests::{BUTTON_SURFACE, build_button, fixed_button};
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::interaction::button_phase::ButtonPhase;
use crate::primitives::input::interaction::button_state::ButtonState;
use crate::primitives::input::interaction::drag::Drag;
use crate::primitives::input::interaction::response_state::ResponseState;
use crate::primitives::input::interaction::scroll_delta::ScrollDelta;
use crate::primitives::input::pointer::PointerButton;
use crate::primitives::input::scroll_targets::ScrollTargets;
use crate::primitives::input::zoom_factor::ZoomFactor;
use crate::primitives::layout::sizing::Sizing;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

fn focusable_id() -> WidgetId {
    WidgetId::from_hash("focusable")
}

fn build_focusable_leaf(ui: &mut Ui) {
    Block::new()
        .id(WidgetId::from_hash("focusable"))
        .focusable(true)
        .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
        .show(ui);
}

#[test]
fn focused_reflects_focused_id_synchronously() {
    let mut h = Harness::mono(UVec2::new(200, 200));
    h.frame(build_focusable_leaf);
    assert!(!h.ui().response_for(focusable_id()).focused);

    h.ui_mut().set_focus(focusable_id());
    assert!(
        h.ui().response_for(focusable_id()).focused,
        "focused must be true the same frame as set_focus",
    );

    h.ui_mut().clear_focus();
    assert!(!h.ui().response_for(focusable_id()).focused);
}

#[test]
fn disabled_reflects_cascaded_ancestor_flag() {
    let mut h = Harness::mono(UVec2::new(200, 200));
    let build = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("parent"))
            .disabled(true)
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("child"))
                    .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
                    .show(ui);
            });
    };
    h.prime(2, build);

    let parent_state = h.ui().response_for(WidgetId::from_hash("parent"));
    let child_state = h.ui().response_for(WidgetId::from_hash("child"));
    assert!(parent_state.disabled);
    assert!(
        child_state.disabled,
        "child inherits cascaded disabled from parent (no self flag)",
    );
}

/// Every disabled fold drops the interaction half, not only the last: a disabled widget must
/// not report `hovered` or `clicked`, which the hit index cannot produce. Geometry stays.
#[test]
fn folding_disabled_in_drops_the_interaction_half_at_every_fold() {
    let busy = ResponseState {
        rect: Some(Rect::new(1.0, 2.0, 3.0, 4.0)),
        layout_rect: Some(Rect::new(5.0, 6.0, 7.0, 8.0)),
        pointer_local: Some(Vec2::new(9.0, 10.0)),
        pointer_over: true,
        focused: true,
        left: ButtonState::new(ButtonPhase::Up { click: Some(1) }, Drag::None),
        scroll: ScrollDelta {
            pixels: Vec2::new(11.0, 12.0),
            ..ScrollDelta::default()
        },
        ..ResponseState::default()
    };

    let mut kept = busy;
    kept.merge_disabled(false);
    assert!(!kept.disabled);
    assert!(kept.hovered() && kept.left.clicked());

    let mut off = busy;
    off.merge_disabled(true);
    off.merge_disabled(false);
    assert!(off.disabled, "a later `false` cannot re-enable it");
    assert!(!off.hovered(), "the hover goes with it");
    assert!(!off.left.clicked(), "and the click");
    assert_eq!(off.left, ButtonState::default());
    assert_eq!(off.right, ButtonState::default());
    assert_eq!(off.middle, ButtonState::default());
    assert_eq!(off.scroll, ScrollDelta::default(), "and the wheel");

    assert_eq!(off.rect, busy.rect);
    assert_eq!(off.layout_rect, busy.layout_rect);
    assert_eq!(off.pointer_local, busy.pointer_local);
    assert!(off.focused);
    assert!(
        off.pointer_over,
        "the observation is geometry too — a tooltip explaining the \
         disable needs it",
    );
}

#[test]
fn disabled_false_when_chain_clean() {
    let mut h = Harness::mono(UVec2::new(200, 200));
    let build = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("parent"))
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("child"))
                    .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
                    .show(ui);
            });
    };
    h.prime(2, build);
    assert!(!h.ui().response_for(WidgetId::from_hash("child")).disabled);
}

/// A departed pointer clears every routed target, which lets `snapshot_frame_quiescent` test one field instead of four.
#[test]
fn a_departed_pointer_clears_every_routed_target() {
    let id = WidgetId::from_hash("w");
    let mut s = Input::new();
    s.pointer.hovered = Some(id);
    s.pointer.scroll_targets = ScrollTargets::both(id);
    s.pointer.pinch_target = Some(id);

    s.refresh_pointer_targets();
    assert_eq!(
        (
            s.pointer.hovered,
            s.pointer.scroll_targets,
            s.pointer.pinch_target
        ),
        (None, ScrollTargets::default(), None),
        "a pointer that left takes every routed target with it",
    );
}

/// The quiescence predicate gating `response_for`'s fast path: every pointer or
/// capture signal flips it false, `focused` deliberately does not. Routed
/// targets cannot be tested alone, since a departed pointer clears them.
#[test]
fn frame_quiescent_predicate() {
    let quiescent = |mutate: &dyn Fn(&mut Input)| {
        let mut s = Input::new();
        mutate(&mut s);
        s.snapshot_frame_quiescent();
        s.frame_quiescent
    };
    assert!(
        quiescent(&|_| {}),
        "a fresh input state (no pointer, no captures) is quiescent",
    );

    let id = WidgetId::from_hash("w");
    let broken = |label: &str, mutate: &dyn Fn(&mut Input)| {
        assert!(!quiescent(mutate), "{label} must break quiescence");
    };
    broken("pointer.pos", &|s| s.pointer.pos = Some(Vec2::ZERO));
    broken("pointer.target_deltas", &|s| {
        s.pointer.target_deltas.push(TargetScrollDelta::new(id));
    });
    broken("capture.press", &|s| {
        s.pointer.captures[PointerButton::Left.idx()].press = Some(Press {
            target: id,
            origin: Vec2::ZERO,
            travel: Vec2::ZERO,
            count: 1,
            fresh: true,
            drag: PressDrag::None,
        });
    });
    broken("capture.release (click)", &|s| {
        s.pointer.captures[PointerButton::Right.idx()].release = Some(Release {
            target: id,
            kind: ReleaseKind::Click { count: 1 },
        });
    });
    broken("capture.release (miss)", &|s| {
        s.pointer.captures[PointerButton::Middle.idx()].release = Some(Release {
            target: id,
            kind: ReleaseKind::Miss,
        });
    });

    assert!(
        quiescent(&|s| s.set_focus(id)),
        "focus alone must NOT break quiescence (read live on the fast path)",
    );
}

/// On a quiescent frame `response_for` keeps the rect and defaults the interaction fields.
#[test]
fn quiescent_frame_keeps_geometry_defaults_interaction() {
    let mut h = Harness::mono(BUTTON_SURFACE);
    let id = WidgetId::from_hash("btn");
    h.frame(build_button(id));

    let r = h.ui().response_for(id);
    let placed = Rect::new(0.0, 0.0, 100.0, 40.0);
    assert_eq!(
        r.rect,
        Some(placed),
        "the quiescent fast path keeps geometry"
    );
    assert_eq!(r.layout_rect, Some(placed));

    assert!(!r.hovered());
    assert!(!r.pressed());
    assert!(!r.left.clicked());
    assert!(!r.right.clicked());
    assert!(!r.focused);
    assert!(!r.left.drag.is_live());
    assert_eq!(r.left.click_count(), 0);
    assert_eq!(r.scroll.pixels, Vec2::ZERO);
    assert_eq!(r.scroll.lines, Vec2::ZERO);
    assert_eq!(r.scroll.zoom, ZoomFactor::ONE);
    assert_eq!(r.pointer_local, None);
}

/// With the pointer over a widget the frame is non-quiescent and the widget-local pointer is computed.
#[test]
fn non_quiescent_frame_computes_interaction() {
    let mut h = Harness::mono(BUTTON_SURFACE);
    let id = WidgetId::from_hash("btn");
    h.frame(build_button(id));

    let pointer = Vec2::new(50.0, 20.0);
    h.hover(pointer);
    h.frame(build_button(id));

    let r = h.ui().response_for(id);
    let layout_rect = r.layout_rect.expect("arranged layout rect present");
    assert!(
        r.hovered(),
        "pointer resting inside the button rect hovers it"
    );
    assert_eq!(
        r.pointer_local,
        Some(pointer - layout_rect.min),
        "pointer_local is the cursor offset from layout_rect.min",
    );
}

/// While a press of any button belongs to one widget, the pointer hovers no other: `hovered`
/// and `Ui::is_hover_within` read one capture-aware target, so the two agree, and the release
/// gives the hover back.
#[test]
fn a_held_press_of_any_button_keeps_the_hover_off_other_widgets() {
    let (a, b) = (WidgetId::from_hash("a"), WidgetId::from_hash("b"));
    let two = |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            fixed_button(a).show(ui);
            fixed_button(b).show(ui);
        });
    };
    let (on_a, on_b) = (Vec2::new(50.0, 20.0), Vec2::new(150.0, 20.0));
    for button in PointerButton::ALL {
        let mut h = Harness::mono(BUTTON_SURFACE);
        h.frame(two);
        h.press(on_a, button);
        h.hover(on_b);
        h.frame(two);
        assert!(
            !h.ui().response_for(b).hovered(),
            "{button:?}: A holds the press"
        );
        assert!(
            !h.ui().is_hover_within(b),
            "{button:?}: the subtree read agrees"
        );
        h.release(button);
        h.frame(two);
        assert!(
            h.ui().response_for(b).hovered(),
            "{button:?}: the release gives the hover back"
        );
        assert!(
            h.ui().is_hover_within(b),
            "{button:?}: and to the subtree read"
        );
    }
}

#[test]
fn pointer_and_drag_vectors_are_scale_invariant() {
    let id = WidgetId::from_hash("scaled-button");
    let local_pointer = Vec2::new(25.0, 10.0);
    let local_delta = Vec2::new(12.0, -8.0);

    for scale in [0.5, 1.0, 2.0] {
        let mut h = Harness::mono(UVec2::new(300, 200));
        let build = |ui: &mut Ui| {
            Panel::zstack()
                .id(WidgetId::from_hash("scaled-parent"))
                .transform(TranslateScale::from_scale(scale))
                .size((Sizing::fixed(120.0), Sizing::fixed(60.0)))
                .show(ui, |ui| {
                    fixed_button(id).show(ui);
                });
        };
        h.frame(build);

        let arranged = h.ui().response_for(id);
        let layout = arranged.layout_rect.expect("button arranged");
        let press = arranged.transform.apply_point(layout.min + local_pointer);
        let pointer = arranged
            .transform
            .apply_point(layout.min + local_pointer + local_delta);
        h.press(press, PointerButton::Left);
        h.hover(pointer);
        h.frame(build);

        let response = h.ui().response_for(id);
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
    let mut h = Harness::mono(BUTTON_SURFACE);
    let id = WidgetId::from_hash("clipped-child");
    let build = |ui: &mut Ui| {
        Panel::canvas()
            .id(WidgetId::from_hash("clipper"))
            .clip_rect()
            .size((Sizing::fixed(50.0), Sizing::fixed(40.0)))
            .show(ui, |ui| {
                Block::new()
                    .id(id)
                    .at(Vec2::new(-20.0, 0.0))
                    .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                    .show(ui);
            });
    };
    h.frame(build);

    let arranged = h.ui().response_for(id);
    let visible = arranged.rect.expect("child visible through clip");
    let layout = arranged.layout_rect.expect("child arranged");
    let surface_origin = arranged.transform.apply_point(layout.min);
    assert_ne!(
        visible.min, surface_origin,
        "the clip must trim the widget's leading edge",
    );

    let pointer = visible.min + Vec2::new(10.0, 10.0);
    h.hover(pointer);
    h.frame(build);
    let response = h.ui().response_for(id);
    assert_eq!(
        response.pointer_local,
        Some(response.transform.inverse_vector(pointer - surface_origin)),
    );
}

/// A warmup's scratch holds the window's one arranged frame while the pass fills it, and the
/// window's input takes it back: one left behind would hit-test the first frame against nothing.
#[test]
fn a_warmup_takes_the_arranged_frame_and_gives_it_back() {
    let mut h = Harness::mono(UVec2::new(200, 200));
    h.frame(build_focusable_leaf);
    let input = h.ui_mut().input_mut();
    let placed = input.arranged.cascade.endpoint(focusable_id());
    assert!(placed.is_some(), "the frame arranged the leaf");

    let scratch = input.warmup_scratch();
    assert_eq!(scratch.arranged.cascade.endpoint(focusable_id()), placed);
    assert_eq!(input.arranged.cascade.endpoint(focusable_id()), None);

    input.adopt_warmup(scratch);
    assert_eq!(input.arranged.cascade.endpoint(focusable_id()), placed);
}

/// The quiescent and non-quiescent paths must agree on every field they both own; toggles only `frame_quiescent`.
#[test]
fn quiescent_and_full_paths_agree_on_geometry() {
    let mut h = Harness::mono(UVec2::new(200, 200));
    h.frame(build_focusable_leaf);

    // Drives the window's `Input` against its frozen cascade, flipping only the quiescent bit.
    let s = h.ui_mut().input_mut();
    s.set_focus(focusable_id());

    let at = s.arranged.cascade.locate(focusable_id());
    s.frame_quiescent = true;
    let quiet = s.response_state(focusable_id(), at);
    s.frame_quiescent = false;
    let full = s.response_state(focusable_id(), at);

    assert_eq!(quiet.rect, full.rect);
    assert_eq!(quiet.layout_rect, full.layout_rect);
    assert_eq!(quiet.transform, full.transform);
    assert_eq!(quiet.disabled, full.disabled);
    assert_eq!(quiet.focused, full.focused);
    assert_eq!(
        quiet.rect,
        Some(Rect::new(0.0, 0.0, 50.0, 50.0)),
        "the fixed 50 × 50 root arranges at the origin, or the comparison \
         is vacuous",
    );
    assert!(quiet.focused, "focus must survive the quiescent path");
}
