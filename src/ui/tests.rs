use crate::Ui;
use crate::element::Configure;
use crate::primitives::{Color, Rect, WidgetId};
use crate::widgets::{Button, Frame, Panel, Styled};

#[test]
#[should_panic(expected = "WidgetId collision")]
fn duplicate_widget_id_panics() {
    // Two `Button::with_id("dup")` calls in one frame produce the same
    // `WidgetId`, which would silently corrupt every per-id store (focus,
    // scroll, click capture, hit-testing). `Ui::node` enforces uniqueness
    // with a release `assert!`.
    let mut ui = Ui::new();
    ui.begin_frame();
    Panel::hstack().show(&mut ui, |ui| {
        Button::with_id("dup").show(ui);
        Button::with_id("dup").show(ui);
    });
}

/// Helper: drive one full frame with an empty root so we can inspect
/// the post-`end_frame` state of the repaint gate.
fn drain_one_frame(ui: &mut Ui) {
    ui.begin_frame();
    Panel::hstack().show(ui, |_| {});
    ui.layout(Rect::new(0.0, 0.0, 100.0, 100.0));
    ui.end_frame();
}

/// Pin: initial state has `should_repaint = true` so the very first
/// frame always runs. Host has nothing to present otherwise.
#[test]
fn should_repaint_starts_true() {
    let ui = Ui::new();
    assert!(ui.should_repaint());
}

/// Pin: a frame that records no widgets (empty conditional UI,
/// initial state, debug toggle) drives `begin → layout → end_frame`
/// without panicking and leaves every per-frame table in a consistent
/// empty state. `Ui::layout` no-ops when the tree has no root rather
/// than panicking — empty UI is a real case.
#[test]
fn empty_ui_drives_a_frame_without_panicking() {
    let mut ui = Ui::new();
    ui.begin_frame();
    ui.layout(Rect::new(0.0, 0.0, 200.0, 200.0));
    ui.end_frame();

    assert_eq!(ui.tree().node_count(), 0);
    assert!(ui.damage.prev.is_empty());
    assert!(ui.damage.dirty.is_empty());
    assert!(ui.damage.rect.is_none());
    assert!(ui.damage_filter().is_none());
    // Repaint gate clears even on empty frames so an idle empty host
    // doesn't burn cycles.
    assert!(!ui.should_repaint());
}

/// Pin: an empty frame followed by a populated frame works (the
/// recorder retains no per-frame state across `begin_frame`).
#[test]
fn empty_then_populated_frame() {
    let mut ui = Ui::new();
    ui.begin_frame();
    ui.layout(Rect::new(0.0, 0.0, 100.0, 100.0));
    ui.end_frame();

    drain_one_frame(&mut ui);
    assert_eq!(ui.tree().node_count(), 1);
    assert!(!ui.damage.prev.is_empty());
}

/// Pin: the full CPU render pipeline (encode + compose) survives an
/// empty UI. Backend `submit` is GPU-bound and not exercised here,
/// but every CPU stage `end_frame` runs (cascade, damage, painter)
/// must be safe on empty input.
#[test]
fn empty_ui_runs_through_pipeline() {
    use crate::primitives::Display;
    use glam::UVec2;
    let mut ui = Ui::new();
    ui.set_display(Display::from_physical(UVec2::new(200, 200), 1.0));
    ui.begin_frame();
    ui.layout(Rect::new(0.0, 0.0, 200.0, 200.0));
    ui.end_frame();

    let frame = ui.frame();
    assert!(frame.buffer.quads.is_empty());
    assert!(frame.buffer.texts.is_empty());
    assert!(frame.buffer.groups.is_empty());
}

/// Pin: a successful `end_frame()` clears the gate. After one frame
/// with no new events, the host can skip the next tick.
#[test]
fn should_repaint_clears_after_end_frame() {
    let mut ui = Ui::new();
    drain_one_frame(&mut ui);
    assert!(!ui.should_repaint());
}

/// Pin: any input flips the gate back on. Conservative — even a
/// pointer move that doesn't change hover index sets it (refining
/// is Stage 3 territory).
#[test]
fn should_repaint_after_input() {
    use crate::input::InputEvent;
    use glam::Vec2;
    let mut ui = Ui::new();
    drain_one_frame(&mut ui);
    assert!(!ui.should_repaint());

    ui.on_input(InputEvent::PointerMoved(Vec2::new(10.0, 10.0)));
    assert!(ui.should_repaint());
}

/// Pin: explicit `request_repaint()` flips the gate. Animations and
/// async state landing use this path.
#[test]
fn request_repaint_flips_gate() {
    let mut ui = Ui::new();
    drain_one_frame(&mut ui);
    assert!(!ui.should_repaint());

    ui.request_repaint();
    assert!(ui.should_repaint());
}

/// Pin: DPI change requests a repaint — physical-pixel rasterization
/// changes with scale factor. A no-op `set_display` call (same value)
/// does *not* request a repaint; the gate is change-driven.
#[test]
fn set_display_requests_repaint_only_on_change() {
    use crate::primitives::Display;
    use glam::UVec2;

    let mut ui = Ui::new();
    drain_one_frame(&mut ui);
    assert!(!ui.should_repaint());

    ui.set_display(Display::from_physical(UVec2::new(800, 600), 2.0));
    assert!(ui.should_repaint());

    drain_one_frame(&mut ui);
    assert!(!ui.should_repaint());

    // Re-setting the same value is a no-op.
    ui.set_display(Display::from_physical(UVec2::new(800, 600), 2.0));
    assert!(!ui.should_repaint());
}

/// Pin: `set_display` panics if scale_factor is below `f32::EPSILON`.
/// Catches stray `0.0` values from buggy hosts before they collapse
/// the UI to a single physical pixel.
#[test]
#[should_panic(expected = "Display::scale_factor must be ≥ f32::EPSILON")]
fn set_display_rejects_zero_scale_factor() {
    use crate::primitives::Display;
    use glam::UVec2;
    let mut ui = Ui::new();
    ui.set_display(Display::from_physical(UVec2::new(800, 600), 0.0));
}

/// Pin: `Display::logical_rect` divides physical by scale_factor.
#[test]
fn display_logical_rect_scales() {
    use crate::primitives::{Display, Rect};
    use glam::UVec2;
    let d = Display::from_physical(UVec2::new(800, 600), 2.0);
    assert_eq!(d.logical_rect(), Rect::new(0.0, 0.0, 400.0, 300.0));
}

/// Pin: the gate is idempotent — multiple `request_repaint()` calls
/// in one frame don't accumulate; one `end_frame()` clears them all.
#[test]
fn request_repaint_is_idempotent() {
    let mut ui = Ui::new();
    drain_one_frame(&mut ui);

    ui.request_repaint();
    ui.request_repaint();
    ui.request_repaint();
    assert!(ui.should_repaint());

    drain_one_frame(&mut ui);
    assert!(!ui.should_repaint());
}

// --- prev_frame snapshot tests ----------------------------------------------
// Stage 3 / Step 2 of the damage-rendering plan. `Ui::prev_frame` holds
// the previous frame's `(rect, authoring-hash)` per `WidgetId`, rebuilt
// at the tail of `end_frame()`. Tests below pin the contract: empty
// before any frame, populated after, captures both rect and hash, and
// drops widgets that disappeared.

#[test]
fn prev_frame_empty_before_first_end_frame() {
    let ui = Ui::new();
    assert!(ui.damage.prev.is_empty());
}

#[test]
fn prev_frame_populated_after_end_frame() {
    let mut ui = Ui::new();
    ui.begin_frame();
    Panel::hstack_with_id("root").show(&mut ui, |ui| {
        Frame::with_id("a")
            .size(50.0)
            .fill(Color::rgb(0.2, 0.4, 0.8))
            .show(ui);
    });
    ui.layout(Rect::new(0.0, 0.0, 200.0, 200.0));
    ui.end_frame();

    let prev = &ui.damage.prev;
    let root_id = WidgetId::from_hash("root");
    let frame_id = WidgetId::from_hash("a");
    assert!(prev.contains_key(&root_id));
    assert!(prev.contains_key(&frame_id));
}

#[test]
fn prev_frame_captures_arranged_rect() {
    let mut ui = Ui::new();
    ui.begin_frame();
    let frame_node = Frame::with_id("a")
        .size(50.0)
        .fill(Color::rgb(0.2, 0.4, 0.8))
        .show(&mut ui)
        .node;
    ui.layout(Rect::new(0.0, 0.0, 200.0, 200.0));
    let arranged = ui.rect(frame_node);
    ui.end_frame();

    let snap = ui.damage.prev[&WidgetId::from_hash("a")];
    assert_eq!(snap.rect, arranged);
}

#[test]
fn prev_frame_captures_authoring_hash() {
    let mut ui = Ui::new();
    ui.begin_frame();
    let frame_node = Frame::with_id("a")
        .size(50.0)
        .fill(Color::rgb(0.2, 0.4, 0.8))
        .show(&mut ui)
        .node;
    ui.layout(Rect::new(0.0, 0.0, 200.0, 200.0));
    ui.end_frame();

    let snap = ui.damage.prev[&WidgetId::from_hash("a")];
    assert_eq!(snap.hash, ui.tree().node_hash(frame_node));
}

#[test]
fn prev_frame_drops_disappeared_widgets() {
    let mut ui = Ui::new();
    ui.begin_frame();
    Panel::hstack_with_id("root").show(&mut ui, |ui| {
        Button::with_id("gone").label("X").show(ui);
    });
    ui.layout(Rect::new(0.0, 0.0, 200.0, 200.0));
    ui.end_frame();
    assert!(ui.damage.prev.contains_key(&WidgetId::from_hash("gone")));

    ui.begin_frame();
    Panel::hstack_with_id("root").show(&mut ui, |_| {});
    ui.layout(Rect::new(0.0, 0.0, 200.0, 200.0));
    ui.end_frame();
    assert!(!ui.damage.prev.contains_key(&WidgetId::from_hash("gone")));
    assert!(ui.damage.prev.contains_key(&WidgetId::from_hash("root")));
}

#[test]
fn prev_frame_updates_on_authoring_change() {
    let mut ui = Ui::new();
    ui.begin_frame();
    Frame::with_id("a")
        .size(50.0)
        .fill(Color::rgb(0.2, 0.4, 0.8))
        .show(&mut ui);
    ui.layout(Rect::new(0.0, 0.0, 200.0, 200.0));
    ui.end_frame();
    let h1 = ui.damage.prev[&WidgetId::from_hash("a")].hash;

    ui.begin_frame();
    Frame::with_id("a")
        .size(50.0)
        .fill(Color::rgb(0.9, 0.4, 0.8))
        .show(&mut ui);
    ui.layout(Rect::new(0.0, 0.0, 200.0, 200.0));
    ui.end_frame();
    let h2 = ui.damage.prev[&WidgetId::from_hash("a")].hash;

    assert_ne!(h1, h2);
}
