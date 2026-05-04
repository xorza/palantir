use crate::Ui;
use crate::element::Configure;
use crate::primitives::{color::Color, display::Display, rect::Rect, widget_id::WidgetId};
use crate::test_support::{begin, new_ui_text, ui_at};
use crate::widgets::{button::Button, frame::Frame, panel::Panel, styled::Styled};
use glam::UVec2;

#[test]
#[should_panic(expected = "WidgetId collision")]
fn duplicate_widget_id_panics() {
    // Two `Button::with_id("dup")` calls in one frame produce the same
    // `WidgetId`, which would silently corrupt every per-id store (focus,
    // scroll, click capture, hit-testing). `Ui::node` enforces uniqueness
    // with a release `assert!`.
    let mut ui = Ui::new();
    ui.begin_frame(Display::default());
    Panel::hstack().show(&mut ui, |ui| {
        Button::with_id("dup").show(ui);
        Button::with_id("dup").show(ui);
    });
}

/// Helper: drive one full frame with an empty root so we can inspect
/// the post-`end_frame` state of the repaint gate.
fn drain_one_frame(ui: &mut Ui) {
    begin(ui, UVec2::new(100, 100));
    Panel::hstack().show(ui, |_| {});
    ui.end_frame();
}

/// Pin: an empty frame (no widgets recorded) drives `begin → layout →
/// end_frame` without panicking, leaves every per-frame table empty,
/// and produces an empty `Frame` with no quads/texts/groups. Empty UI
/// is a real case (initial state, debug toggle, conditional UI all
/// empty), and the full CPU pipeline must survive it.
#[test]
fn empty_ui_drives_a_frame_safely() {
    let mut ui = ui_at(UVec2::new(200, 200));
    {
        // FrameOutput borrows ui.buffer; check pipeline output first,
        // then drop the borrow so we can read other ui state.
        let frame = ui.end_frame();
        assert!(frame.buffer.quads.is_empty());
        assert!(frame.buffer.texts.is_empty());
        assert!(frame.buffer.groups.is_empty());
    }

    assert_eq!(ui.tree.node_count(), 0);
    assert!(ui.damage.prev.is_empty());
    assert!(ui.damage.dirty.is_empty());
    assert!(ui.damage.rect.is_none());
    assert!(ui.damage.filter(ui.display.logical_rect()).is_none());
    // Repaint gate clears even on empty frames so an idle empty host
    // doesn't burn cycles.
    assert!(!ui.should_repaint());
}

/// Pin: an empty frame followed by a populated frame works (the
/// recorder retains no per-frame state across `begin_frame`).
#[test]
fn empty_then_populated_frame() {
    let mut ui = ui_at(UVec2::new(100, 100));
    ui.end_frame();

    drain_one_frame(&mut ui);
    assert_eq!(ui.tree.node_count(), 1);
    assert!(!ui.damage.prev.is_empty());
}

/// Pin: initial gate state is `true` (very first frame must run, host
/// has nothing to present otherwise) and `end_frame()` clears it (idle
/// host can skip the next tick).
#[test]
fn repaint_gate_starts_true_clears_after_end_frame() {
    let mut ui = Ui::new();
    assert!(ui.should_repaint());
    drain_one_frame(&mut ui);
    assert!(!ui.should_repaint());
}

/// Pin: `request_repaint()` flips the gate and is idempotent — N
/// calls in one frame don't accumulate; one `end_frame()` clears
/// them all. Animations and async state landing use this path.
#[test]
fn request_repaint_flips_gate_idempotently() {
    let mut ui = Ui::new();
    drain_one_frame(&mut ui);
    assert!(!ui.should_repaint());

    ui.request_repaint();
    ui.request_repaint();
    ui.request_repaint();
    assert!(ui.should_repaint());

    drain_one_frame(&mut ui);
    assert!(!ui.should_repaint());
}

/// Pin: input flips the gate back on. Conservative — even a pointer
/// move that doesn't change hover index sets it.
#[test]
fn repaint_gate_flips_on_input() {
    use crate::input::InputEvent;
    use glam::Vec2;
    let mut ui = Ui::new();
    drain_one_frame(&mut ui);
    assert!(!ui.should_repaint());

    ui.on_input(InputEvent::PointerMoved(Vec2::new(10.0, 10.0)));
    assert!(ui.should_repaint());
}

/// Pin: `begin_frame` panics if `display.scale_factor` is below
/// `f32::EPSILON`.
#[test]
#[should_panic(expected = "Display::scale_factor must be ≥ f32::EPSILON")]
fn begin_frame_rejects_zero_scale_factor() {
    let mut ui = Ui::new();
    ui.begin_frame(Display::from_physical(UVec2::new(800, 600), 0.0));
}

/// Pin: `Display::logical_rect` divides physical by scale_factor.
#[test]
fn display_logical_rect_scales() {
    let d = Display::from_physical(UVec2::new(800, 600), 2.0);
    assert_eq!(d.logical_rect(), Rect::new(0.0, 0.0, 400.0, 300.0));
}

// --- prev_frame snapshot tests ----------------------------------------------
// Stage 3 / Step 2 of the damage-rendering plan. `Ui::prev_frame` holds
// the previous frame's `(rect, authoring-hash)` per `WidgetId`, rebuilt
// at the tail of `end_frame()`.

#[test]
fn prev_frame_empty_before_first_end_frame() {
    let ui = Ui::new();
    assert!(ui.damage.prev.is_empty());
}

#[test]
fn prev_frame_populated_after_end_frame() {
    let mut ui = ui_at(UVec2::new(200, 200));
    Panel::hstack_with_id("root").show(&mut ui, |ui| {
        Frame::with_id("a")
            .size(50.0)
            .fill(Color::rgb(0.2, 0.4, 0.8))
            .show(ui);
    });
    ui.end_frame();

    let prev = &ui.damage.prev;
    let root_id = WidgetId::from_hash("root");
    let frame_id = WidgetId::from_hash("a");
    assert!(prev.contains_key(&root_id));
    assert!(prev.contains_key(&frame_id));
}

#[test]
fn prev_frame_captures_arranged_rect() {
    let mut ui = ui_at(UVec2::new(200, 200));
    let frame_node = Frame::with_id("a")
        .size(50.0)
        .fill(Color::rgb(0.2, 0.4, 0.8))
        .show(&mut ui)
        .node;
    ui.end_frame();
    let arranged = ui.layout_engine.rect(frame_node);

    let snap = ui.damage.prev[&WidgetId::from_hash("a")];
    assert_eq!(snap.rect, arranged);
}

#[test]
fn prev_frame_captures_authoring_hash() {
    let mut ui = ui_at(UVec2::new(200, 200));
    let frame_node = Frame::with_id("a")
        .size(50.0)
        .fill(Color::rgb(0.2, 0.4, 0.8))
        .show(&mut ui)
        .node;
    ui.end_frame();

    let snap = ui.damage.prev[&WidgetId::from_hash("a")];
    assert_eq!(snap.hash, ui.tree.node_hash(frame_node));
}

#[test]
fn prev_frame_drops_disappeared_widgets() {
    let mut ui = ui_at(UVec2::new(200, 200));
    Panel::hstack_with_id("root").show(&mut ui, |ui| {
        Button::with_id("gone").label("X").show(ui);
    });
    ui.end_frame();
    assert!(ui.damage.prev.contains_key(&WidgetId::from_hash("gone")));

    ui.begin_frame(Display::default());
    Panel::hstack_with_id("root").show(&mut ui, |_| {});
    ui.end_frame();
    assert!(!ui.damage.prev.contains_key(&WidgetId::from_hash("gone")));
    assert!(ui.damage.prev.contains_key(&WidgetId::from_hash("root")));
}

#[test]
fn prev_frame_updates_on_authoring_change() {
    let mut ui = ui_at(UVec2::new(200, 200));
    Frame::with_id("a")
        .size(50.0)
        .fill(Color::rgb(0.2, 0.4, 0.8))
        .show(&mut ui);
    ui.end_frame();
    let h1 = ui.damage.prev[&WidgetId::from_hash("a")].hash;

    ui.begin_frame(Display::default());
    Frame::with_id("a")
        .size(50.0)
        .fill(Color::rgb(0.9, 0.4, 0.8))
        .show(&mut ui);
    ui.end_frame();
    let h2 = ui.damage.prev[&WidgetId::from_hash("a")].hash;

    assert_ne!(h1, h2);
}

/// Pin: a Text widget whose authoring inputs don't change across
/// frames hits the per-`WidgetId` reuse cache in `LayoutEngine` and
/// does *not* dispatch through `TextMeasurer::measure` again.
#[test]
fn text_reshape_skipped_when_unchanged_across_frames() {
    use crate::widgets::text::Text;

    let mut ui = new_ui_text();

    let render = |ui: &mut Ui| {
        begin(ui, UVec2::new(400, 200));
        Panel::vstack().show(ui, |ui| {
            Text::with_id("hello", "the quick brown fox").show(ui);
        });
        ui.end_frame();
    };

    render(&mut ui);
    let after_first = ui.text.measure_calls;
    assert!(
        after_first > 0,
        "first frame should drive at least one measure call",
    );

    render(&mut ui);
    let after_second = ui.text.measure_calls;
    assert_eq!(
        after_second,
        after_first,
        "second identical frame must reuse cached MeasureResult — \
         got {} extra measure call(s)",
        after_second - after_first,
    );
}

/// Pin: changing the Text's content invalidates the reuse entry and
/// drives a fresh measure.
#[test]
fn text_reshape_runs_when_content_changes() {
    use crate::widgets::text::Text;

    let mut ui = new_ui_text();

    begin(&mut ui, UVec2::new(400, 200));
    Panel::vstack().show(&mut ui, |ui| {
        Text::with_id("changing", "first").show(ui);
    });
    ui.end_frame();
    let before = ui.text.measure_calls;

    begin(&mut ui, UVec2::new(400, 200));
    Panel::vstack().show(&mut ui, |ui| {
        Text::with_id("changing", "second").show(ui);
    });
    ui.end_frame();
    let after = ui.text.measure_calls;

    assert!(
        after > before,
        "content change must trigger fresh measure (before={before}, after={after})",
    );
}

/// Pin: reuse survives a wrap reshape — same widget, same content, same
/// constrained width across two frames runs measure once on frame 1 and
/// not at all on frame 2.
#[test]
fn wrapping_text_reshape_skipped_when_unchanged() {
    use crate::primitives::sizing::Sizing;
    use crate::widgets::text::Text;

    let mut ui = new_ui_text();

    let render = |ui: &mut Ui| {
        begin(ui, UVec2::new(400, 200));
        Panel::vstack()
            .size((Sizing::Fixed(60.0), Sizing::Hug))
            .show(ui, |ui| {
                Text::with_id("wrapped", "the quick brown fox jumps over the lazy dog")
                    .size_px(16.0)
                    .wrapping()
                    .show(ui);
            });
        ui.end_frame();
    };

    render(&mut ui);
    let after_first = ui.text.measure_calls;
    render(&mut ui);
    let after_second = ui.text.measure_calls;
    assert_eq!(
        after_second,
        after_first,
        "second wrapped frame must reuse — got {} extra measure call(s)",
        after_second - after_first,
    );
}

/// Pin: intrinsic-query path also reuses the per-widget cache.
#[test]
fn intrinsic_query_reuses_cached_text_measure() {
    use crate::primitives::{sizing::Sizing, track::Track};
    use crate::widgets::{grid::Grid, text::Text};

    let mut ui = new_ui_text();

    let render = |ui: &mut Ui| {
        begin(ui, UVec2::new(400, 200));
        Grid::with_id("g")
            .size((Sizing::Fixed(200.0), Sizing::Hug))
            .cols(std::rc::Rc::from([Track::hug(), Track::fill()]))
            .show(ui, |ui| {
                Text::with_id("hug-col-text", "label")
                    .grid_cell((0, 0))
                    .show(ui);
                Text::with_id(
                    "fill-col-text",
                    "the quick brown fox jumps over the lazy dog",
                )
                .wrapping()
                .grid_cell((0, 1))
                .show(ui);
            });
        ui.end_frame();
    };

    render(&mut ui);
    let after_first = ui.text.measure_calls;
    render(&mut ui);
    let after_second = ui.text.measure_calls;
    assert_eq!(
        after_second,
        after_first,
        "intrinsic queries on unchanged Text widgets must reuse — got {} extra measure call(s)",
        after_second - after_first,
    );
}

/// Pin: when a Text widget disappears from the tree, its `text_reuse`
/// entry is evicted on the same frame.
#[test]
fn text_reuse_evicts_disappeared_widgets() {
    use crate::widgets::text::Text;

    let mut ui = new_ui_text();

    begin(&mut ui, UVec2::new(400, 200));
    Panel::vstack().show(&mut ui, |ui| {
        Text::with_id("transient", "hello").show(ui);
    });
    ui.end_frame();
    let wid = WidgetId::from_hash("transient");
    assert!(
        ui.text.reuse.contains_key(&wid),
        "text widget should populate text_reuse on first render",
    );

    begin(&mut ui, UVec2::new(400, 200));
    Panel::vstack().show(&mut ui, |_ui| {});
    ui.end_frame();
    assert!(
        !ui.text.reuse.contains_key(&wid),
        "removed widget's reuse entry must be swept",
    );
}

/// Pin: when the authoring hash is unchanged but the wrap target
/// (parent's available width) shifts between frames, the cached
/// *unbounded* shape is preserved — only the *wrap* reshape runs
/// again.
#[test]
fn wrap_target_change_preserves_unbounded_cache() {
    use crate::primitives::sizing::Sizing;
    use crate::widgets::text::Text;

    let mut ui = new_ui_text();

    let render = |ui: &mut Ui, slot_w: f32| {
        begin(ui, UVec2::new(400, 200));
        Panel::vstack()
            .size((Sizing::Fixed(slot_w), Sizing::Hug))
            .show(ui, |ui| {
                Text::with_id("p", "the quick brown fox jumps over the lazy dog")
                    .size_px(16.0)
                    .wrapping()
                    .show(ui);
            });
        ui.end_frame();
    };

    render(&mut ui, 60.0);
    let after_first = ui.text.measure_calls;
    assert!(
        after_first >= 2,
        "first frame should measure both unbounded and wrap (got {after_first})",
    );

    render(&mut ui, 80.0);
    let after_second = ui.text.measure_calls;
    let delta = after_second - after_first;
    assert_eq!(
        delta, 1,
        "wrap-target change must reshape only the wrap path, not unbounded \
         (extra calls: {delta})",
    );
}
