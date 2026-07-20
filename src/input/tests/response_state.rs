use crate::Ui;
use crate::forest::element::Configure;
use crate::input::pointer::PointerButton;
use crate::input::{InputEvent, InputState, Press, PressDrag, Release, ReleaseKind, TargetDeltas};
use crate::layout::types::sizing::Sizing;
use crate::primitives::transform::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use crate::widgets::button::Button;
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
    let mut ui = Ui::for_test();
    ui.run_at(UVec2::new(200, 200), build_focusable_leaf);
    assert!(!ui.response_for(focusable_id()).focused);

    ui.request_focus(Some(focusable_id()));
    assert!(
        ui.response_for(focusable_id()).focused,
        "focused must be true the same frame as request_focus",
    );

    ui.request_focus(None);
    assert!(!ui.response_for(focusable_id()).focused);
}

#[test]
fn disabled_reflects_cascaded_ancestor_flag() {
    let mut ui = Ui::for_test();
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
    ui.run_at(UVec2::new(200, 200), build);
    ui.run_at(UVec2::new(200, 200), build);

    let parent_state = ui.response_for(WidgetId::from_hash("parent"));
    let child_state = ui.response_for(WidgetId::from_hash("child"));
    assert!(parent_state.disabled);
    assert!(
        child_state.disabled,
        "child inherits cascaded disabled from parent (no self flag)",
    );
}

#[test]
fn disabled_false_when_chain_clean() {
    let mut ui = Ui::for_test();
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
    ui.run_at(UVec2::new(200, 200), build);
    ui.run_at(UVec2::new(200, 200), build);
    assert!(!ui.response_for(WidgetId::from_hash("child")).disabled);
}

/// The once-per-frame quiescence predicate that gates `response_for`'s
/// fast path: every pointer/capture-derived signal flips it false, but
/// `focused` deliberately does not (it can be set mid-record).
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
    broken("hovered", &|s| s.hovered = Some(id));
    broken("scroll_target", &|s| s.scroll_target = Some(id));
    broken("pinch_target", &|s| s.pinch_target = Some(id));
    broken("frame_target_deltas", &|s| {
        s.frame_target_deltas.push(TargetDeltas::new(id))
    });
    broken("capture.press", &|s| {
        s.captures[PointerButton::Left.idx()].press = Some(Press {
            target: id,
            origin: Vec2::ZERO,
            seq: 1,
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
    let mut ui = Ui::for_test();
    let id = WidgetId::from_hash("btn");
    // No pointer is ever fed → the frame is quiescent, so the snapshot
    // taken at record-pass start stays valid for this post-frame read.
    ui.run_at(button_surface(), build_button(id));

    let r = ui.response_for(id);
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
    assert_eq!(r.scroll.zoom, 1.0);
    assert_eq!(r.pointer_local, None);
}

/// With the pointer resting over a widget the frame is non-quiescent, so
/// `response_for` runs the full interaction path and computes the
/// pre-transform widget-local pointer.
#[test]
fn non_quiescent_frame_computes_interaction() {
    let mut ui = Ui::for_test();
    let id = WidgetId::from_hash("btn");
    ui.run_at(button_surface(), build_button(id));

    let pointer = Vec2::new(50.0, 20.0);
    ui.on_input(InputEvent::PointerMoved(pointer));
    // Run a frame *after* the pointer event so the snapshot reflects it,
    // then read — the pointer makes the frame non-quiescent (full path).
    ui.run_at(button_surface(), build_button(id));

    let r = ui.response_for(id);
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
        let mut ui = Ui::for_test();
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
        ui.run_at(UVec2::new(300, 200), build);

        let arranged = ui.response_for(id);
        let layout = arranged.layout_rect.expect("button arranged");
        let press = arranged.transform.apply_point(layout.min + local_pointer);
        let pointer = arranged
            .transform
            .apply_point(layout.min + local_pointer + local_delta);
        ui.on_input(InputEvent::PointerMoved(press));
        ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
        ui.on_input(InputEvent::PointerMoved(pointer));
        ui.run_at(UVec2::new(300, 200), build);

        let response = ui.response_for(id);
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
    let mut ui = Ui::for_test();
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
    ui.run_at(button_surface(), build);

    let arranged = ui.response_for(id);
    let visible = arranged.rect.expect("child visible through clip");
    let layout = arranged.layout_rect.expect("child arranged");
    let surface_origin = arranged.transform.apply_point(layout.min);
    assert_ne!(
        visible.min, surface_origin,
        "the clip must trim the widget's leading edge",
    );

    let pointer = visible.min + Vec2::new(10.0, 10.0);
    ui.on_input(InputEvent::PointerMoved(pointer));
    ui.run_at(button_surface(), build);
    let response = ui.response_for(id);
    assert_eq!(
        response.pointer_local,
        Some(response.transform.inverse_vector(pointer - surface_origin)),
    );
}
