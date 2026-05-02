use crate::Ui;
use crate::primitives::Rect;
use crate::widgets::{Button, Panel};

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
/// changes with scale factor.
#[test]
fn set_scale_factor_requests_repaint() {
    let mut ui = Ui::new();
    drain_one_frame(&mut ui);
    assert!(!ui.should_repaint());

    ui.set_scale_factor(2.0);
    assert!(ui.should_repaint());
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
