use crate::TextStyle;
use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::tree::Layer;
use crate::forest::widget_id::WidgetId;
use crate::layout::types::display::Display;
use crate::primitives::{color::Color, rect::Rect};
use crate::support::testing::{begin, new_ui_text, ui_at};
use crate::ui::damage::DamagePaint;
use crate::widgets::theme::Background;
use crate::widgets::{button::Button, frame::Frame, panel::Panel};
use glam::UVec2;

#[test]
#[should_panic(expected = "WidgetId collision")]
fn duplicate_widget_id_panics() {
    // Two `Button::new().id_salt("dup")` calls in one frame produce the same
    // `WidgetId`, which would silently corrupt every per-id store (focus,
    // scroll, click capture, hit-testing). `Ui::node` enforces uniqueness
    // with a release `assert!`.
    let mut ui = Ui::new();
    ui.begin_frame(Display::default());
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        Button::new().id_salt("dup").show(ui);
        Button::new().id_salt("dup").show(ui);
    });
}

/// Auto-generated ids (call-site hash) silently disambiguate when the same
/// site fires more than once per frame — that's the "loop / closure helper"
/// case where `Foo::new()` would otherwise collide. Each occurrence must
/// produce a distinct `WidgetId` so per-id state stays separate.
#[test]
fn auto_id_collisions_disambiguate() {
    fn chip(ui: &mut crate::Ui) {
        Frame::new().auto_id().show(ui);
    }
    let mut ui = Ui::new();
    ui.begin_frame(Display::default());
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        chip(ui);
        chip(ui);
        chip(ui);
    });
    // 1 panel + 3 chips = 4 distinct ids, no panic.
    assert_eq!(ui.forest.tree(Layer::Main).records.len(), 4);
}

/// Helper: drive one full frame with an empty root so we can inspect
/// the post-`end_frame` state of the repaint gate.
fn drain_one_frame(ui: &mut Ui) {
    begin(ui, UVec2::new(100, 100));
    Panel::hstack().auto_id().show(ui, |_| {});
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

    assert_eq!(ui.forest.tree(Layer::Main).records.len(), 0);
    assert!(ui.damage.prev.is_empty());
    assert!(ui.damage.dirty.is_empty());
    assert!(ui.damage.region.is_empty());
    assert_eq!(
        ui.damage.filter(ui.display.logical_rect()),
        DamagePaint::Skip
    );
}

/// Pin: an empty frame followed by a populated frame works (the
/// recorder retains no per-frame state across `begin_frame`).
#[test]
fn empty_then_populated_frame() {
    let mut ui = ui_at(UVec2::new(100, 100));
    ui.end_frame();

    drain_one_frame(&mut ui);
    assert_eq!(ui.forest.tree(Layer::Main).records.len(), 1);
    // Root Panel is non-painting (no chrome, no shapes) so prev stays
    // empty — only painting widgets are tracked.
    assert!(ui.damage.prev.is_empty());
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
    Panel::hstack().id_salt("root").show(&mut ui, |ui| {
        Frame::new()
            .id_salt("a")
            .size(50.0)
            .background(Background {
                fill: Color::rgb(0.2, 0.4, 0.8),
                ..Default::default()
            })
            .show(ui);
    });
    ui.end_frame();

    let prev = &ui.damage.prev;
    let root_id = WidgetId::from_hash("root");
    let frame_id = WidgetId::from_hash("a");
    // Root Panel has no chrome and no direct shapes — non-painting,
    // so it's not snapshotted. The Frame paints (background) and is.
    assert!(!prev.contains_key(&root_id));
    assert!(prev.contains_key(&frame_id));
}

#[test]
fn prev_frame_captures_arranged_rect() {
    let mut ui = ui_at(UVec2::new(200, 200));
    let frame_node = Frame::new()
        .id_salt("a")
        .size(50.0)
        .background(Background {
            fill: Color::rgb(0.2, 0.4, 0.8),
            ..Default::default()
        })
        .show(&mut ui)
        .node;
    ui.end_frame();
    let arranged = ui.layout.result[Layer::Main].rect[frame_node.index()];

    let snap = ui.damage.prev[&WidgetId::from_hash("a")];
    assert_eq!(snap.rect, arranged);
}

#[test]
fn prev_frame_captures_authoring_hash() {
    let mut ui = ui_at(UVec2::new(200, 200));
    let frame_node = Frame::new()
        .id_salt("a")
        .size(50.0)
        .background(Background {
            fill: Color::rgb(0.2, 0.4, 0.8),
            ..Default::default()
        })
        .show(&mut ui)
        .node;
    ui.end_frame();

    let snap = ui.damage.prev[&WidgetId::from_hash("a")];
    assert_eq!(
        snap.hash,
        ui.forest.tree(Layer::Main).rollups.node[frame_node.index()]
    );
}

#[test]
fn prev_frame_drops_disappeared_widgets() {
    let mut ui = ui_at(UVec2::new(200, 200));
    Panel::hstack().id_salt("root").show(&mut ui, |ui| {
        Button::new().id_salt("gone").label("X").show(ui);
    });
    ui.end_frame();
    assert!(ui.damage.prev.contains_key(&WidgetId::from_hash("gone")));

    ui.begin_frame(Display::default());
    Panel::hstack().id_salt("root").show(&mut ui, |_| {});
    ui.end_frame();
    assert!(!ui.damage.prev.contains_key(&WidgetId::from_hash("gone")));
    // Root Panel is non-painting so it never enters prev — the
    // remaining-after-eviction check is just that "gone" is gone.
    assert!(!ui.damage.prev.contains_key(&WidgetId::from_hash("root")));
}

#[test]
fn prev_frame_updates_on_authoring_change() {
    let mut ui = ui_at(UVec2::new(200, 200));
    Frame::new()
        .id_salt("a")
        .size(50.0)
        .background(Background {
            fill: Color::rgb(0.2, 0.4, 0.8),
            ..Default::default()
        })
        .show(&mut ui);
    ui.end_frame();
    let h1 = ui.damage.prev[&WidgetId::from_hash("a")].hash;

    ui.begin_frame(Display::default());
    Frame::new()
        .id_salt("a")
        .size(50.0)
        .background(Background {
            fill: Color::rgb(0.9, 0.4, 0.8),
            ..Default::default()
        })
        .show(&mut ui);
    ui.end_frame();
    let h2 = ui.damage.prev[&WidgetId::from_hash("a")].hash;

    assert_ne!(h1, h2);
}

/// Pin: a Text widget whose authoring inputs don't change across
/// frames hits the per-`WidgetId` reuse cache in `LayoutEngine` and
/// does *not* dispatch through `TextShaper::measure` again.
#[test]
fn text_reshape_skipped_when_unchanged_across_frames() {
    use crate::widgets::text::Text;

    let mut ui = new_ui_text();

    let render = |ui: &mut Ui| {
        begin(ui, UVec2::new(400, 200));
        Panel::vstack().auto_id().show(ui, |ui| {
            Text::new("the quick brown fox").id_salt("hello").show(ui);
        });
        ui.end_frame();
    };

    render(&mut ui);
    let after_first = crate::support::internals::text_shaper_measure_calls(&ui.text);
    assert!(
        after_first > 0,
        "first frame should drive at least one measure call",
    );

    render(&mut ui);
    let after_second = crate::support::internals::text_shaper_measure_calls(&ui.text);
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
    Panel::vstack().auto_id().show(&mut ui, |ui| {
        Text::new("first").id_salt("changing").show(ui);
    });
    ui.end_frame();
    let before = crate::support::internals::text_shaper_measure_calls(&ui.text);

    begin(&mut ui, UVec2::new(400, 200));
    Panel::vstack().auto_id().show(&mut ui, |ui| {
        Text::new("second").id_salt("changing").show(ui);
    });
    ui.end_frame();
    let after = crate::support::internals::text_shaper_measure_calls(&ui.text);

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
    use crate::layout::types::sizing::Sizing;
    use crate::widgets::text::Text;

    let mut ui = new_ui_text();

    let render = |ui: &mut Ui| {
        begin(ui, UVec2::new(400, 200));
        Panel::vstack()
            .auto_id()
            .size((Sizing::Fixed(60.0), Sizing::Hug))
            .show(ui, |ui| {
                Text::new("the quick brown fox jumps over the lazy dog")
                    .id_salt("wrapped")
                    .style(TextStyle::default().with_font_size(16.0))
                    .wrapping()
                    .show(ui);
            });
        ui.end_frame();
    };

    render(&mut ui);
    let after_first = crate::support::internals::text_shaper_measure_calls(&ui.text);
    render(&mut ui);
    let after_second = crate::support::internals::text_shaper_measure_calls(&ui.text);
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
    use crate::layout::types::{sizing::Sizing, track::Track};
    use crate::widgets::{grid::Grid, text::Text};

    let mut ui = new_ui_text();

    let render = |ui: &mut Ui| {
        begin(ui, UVec2::new(400, 200));
        Grid::new()
            .id_salt("g")
            .size((Sizing::Fixed(200.0), Sizing::Hug))
            .cols(std::rc::Rc::from([Track::hug(), Track::fill()]))
            .show(ui, |ui| {
                Text::new("label")
                    .id_salt("hug-col-text")
                    .grid_cell((0, 0))
                    .show(ui);
                Text::new("the quick brown fox jumps over the lazy dog")
                    .id_salt("fill-col-text")
                    .wrapping()
                    .grid_cell((0, 1))
                    .show(ui);
            });
        ui.end_frame();
    };

    render(&mut ui);
    let after_first = crate::support::internals::text_shaper_measure_calls(&ui.text);
    render(&mut ui);
    let after_second = crate::support::internals::text_shaper_measure_calls(&ui.text);
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
    Panel::vstack().auto_id().show(&mut ui, |ui| {
        Text::new("hello").id_salt("transient").show(ui);
    });
    ui.end_frame();
    let wid = WidgetId::from_hash("transient");
    assert!(
        crate::support::internals::text_shaper_has_reuse_entry(&ui.text, wid, 0),
        "text widget should populate text_reuse on first render",
    );

    begin(&mut ui, UVec2::new(400, 200));
    Panel::vstack().auto_id().show(&mut ui, |_ui| {});
    ui.end_frame();
    assert!(
        !crate::support::internals::text_shaper_has_reuse_entry(&ui.text, wid, 0),
        "removed widget's reuse entry must be swept",
    );
}

/// Pin: when the authoring hash is unchanged but the wrap target
/// (parent's available width) shifts between frames, the cached
/// *unbounded* shape is preserved — only the *wrap* reshape runs
/// again.
#[test]
fn wrap_target_change_preserves_unbounded_cache() {
    use crate::layout::types::sizing::Sizing;
    use crate::widgets::text::Text;

    let mut ui = new_ui_text();

    let render = |ui: &mut Ui, slot_w: f32| {
        begin(ui, UVec2::new(400, 200));
        Panel::vstack()
            .auto_id()
            .size((Sizing::Fixed(slot_w), Sizing::Hug))
            .show(ui, |ui| {
                Text::new("the quick brown fox jumps over the lazy dog")
                    .id_salt("p")
                    .style(TextStyle::default().with_font_size(16.0))
                    .wrapping()
                    .show(ui);
            });
        ui.end_frame();
    };

    render(&mut ui, 60.0);
    let after_first = crate::support::internals::text_shaper_measure_calls(&ui.text);
    assert!(
        after_first >= 2,
        "first frame should measure both unbounded and wrap (got {after_first})",
    );

    render(&mut ui, 80.0);
    let after_second = crate::support::internals::text_shaper_measure_calls(&ui.text);
    let delta = after_second - after_first;
    assert_eq!(
        delta, 1,
        "wrap-target change must reshape only the wrap path, not unbounded \
         (extra calls: {delta})",
    );
}

#[test]
fn state_map_persists_and_evicts_with_recorded_ids() {
    let mut ui = ui_at(UVec2::new(100, 100));
    let id_a = WidgetId::from_hash("a");
    let id_b = WidgetId::from_hash("b");

    begin(&mut ui, UVec2::new(100, 100));
    Frame::new().id_salt("a").show(&mut ui);
    Frame::new().id_salt("b").show(&mut ui);
    *ui.state_mut::<u32>(id_a) = 11;
    *ui.state_mut::<u32>(id_b) = 22;
    ui.end_frame();

    begin(&mut ui, UVec2::new(100, 100));
    Frame::new().id_salt("a").show(&mut ui);
    assert_eq!(*ui.state_mut::<u32>(id_a), 11);
    ui.end_frame();

    begin(&mut ui, UVec2::new(100, 100));
    Frame::new().id_salt("b").show(&mut ui);
    assert_eq!(
        *ui.state_mut::<u32>(id_b),
        0,
        "B was unrecorded in frame 2; its row should have been swept",
    );
    ui.end_frame();
}

/// `Ui::run_frame` re-records when the frame contained input that
/// could plausibly drive a state mutation (action input), and runs
/// the build closure exactly once otherwise. Action-event coverage
/// has to be exact: false positives waste CPU silently, false
/// negatives leave the popup-dismissal class of bugs unfixed.
#[test]
fn run_frame_pass_count_matches_action_trigger() {
    use crate::input::keyboard::Key;
    use crate::input::{InputEvent, PointerButton};
    use crate::layout::types::display::Display;
    use glam::Vec2;
    use std::cell::Cell;

    const SURFACE: UVec2 = UVec2::new(100, 100);
    let display = Display::from_physical(SURFACE, 1.0);
    type Prime = fn(&mut Ui);
    // (case label, what to fire between frames, expected build calls)
    let cases: &[(&str, Prime, usize)] = &[
        ("idle", |_ui| {}, 1),
        (
            "hover only",
            |ui| ui.on_input(InputEvent::PointerMoved(Vec2::new(10.0, 10.0))),
            1,
        ),
        (
            "modifiers only",
            |ui| {
                ui.on_input(InputEvent::ModifiersChanged(
                    crate::input::keyboard::Modifiers::NONE,
                ))
            },
            1,
        ),
        (
            "click",
            |ui| {
                ui.on_input(InputEvent::PointerMoved(Vec2::new(10.0, 10.0)));
                ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
                ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
            },
            2,
        ),
        (
            "keydown",
            |ui| {
                ui.on_input(InputEvent::KeyDown {
                    key: Key::Enter,
                    repeat: false,
                })
            },
            2,
        ),
        (
            "scroll",
            |ui| ui.on_input(InputEvent::Scroll(Vec2::new(0.0, 10.0))),
            2,
        ),
    ];

    for (label, prime, expected) in cases {
        let mut ui = ui_at(SURFACE);
        // Establish a baseline frame so the under-test `run_frame`
        // diffs against a real prior recording, not the
        // never-painted initial state.
        Panel::vstack().id_salt("root").show(&mut ui, |_| {});
        ui.end_frame();

        prime(&mut ui);

        let count = Cell::new(0u32);
        let _ = ui.run_frame(display, std::time::Duration::ZERO, |ui| {
            count.set(count.get() + 1);
            Panel::vstack().id_salt("root").show(ui, |_| {});
        });
        assert_eq!(
            count.get() as usize,
            *expected,
            "{label}: expected {expected} build invocation(s), got {}",
            count.get(),
        );
    }
}

/// `Ui::run_frame` plumbs `now`, `dt`, and the repaint-requested
/// flag end-to-end: per-call `now` lands in `Ui::time`, the derived
/// `dt` clamps to `MAX_DT`, `repaint_requested` resets at the top
/// of every call, and a flag set during recording surfaces on
/// `FrameOutput`.
#[test]
fn run_frame_plumbs_now_dt_and_repaint_request() {
    use crate::ui::MAX_DT;
    use std::time::Duration;

    const SURFACE: UVec2 = UVec2::new(100, 100);
    let display = Display::from_physical(SURFACE, 1.0);

    let mut ui = ui_at(SURFACE);
    Panel::vstack().id_salt("root").show(&mut ui, |_| {});
    ui.end_frame();

    // Frame A: idle, no repaint request, now = 16ms.
    {
        let frame = ui.run_frame(display, Duration::from_millis(16), |ui| {
            Panel::vstack().id_salt("root").show(ui, |_| {});
        });
        assert!(
            !frame.repaint_requested(),
            "no animate-not-settled flag set — must stay false",
        );
    }
    assert_eq!(ui.time, Duration::from_millis(16));
    assert!(
        (ui.dt - 0.016).abs() < 1e-6,
        "Ui::dt should be (now - prev) in seconds; got {}",
        ui.dt,
    );

    // Frame B: simulate an unsettled animation tick by setting the
    // internal flag during recording (production code does this via
    // `Ui::animate`). The flag must survive end_frame and reach
    // `FrameOutput`.
    {
        let frame = ui.run_frame(display, Duration::from_millis(32), |ui| {
            Panel::vstack().id_salt("root").show(ui, |_| {});
            ui.repaint_requested = true;
        });
        assert!(
            frame.repaint_requested(),
            "repaint_requested set during recording must surface on FrameOutput",
        );
    }
    assert_eq!(ui.time, Duration::from_millis(32));
    assert!(
        (ui.dt - 0.016).abs() < 1e-6,
        "Ui::dt should be next-frame delta; got {}",
        ui.dt,
    );

    // Frame C: oversized gap (5s pause) clamps dt to MAX_DT, but
    // `time` still tracks the host's true clock — only `dt` clamps so
    // animation math doesn't teleport.
    {
        let _ = ui.run_frame(display, Duration::from_millis(5_032), |ui| {
            Panel::vstack().id_salt("root").show(ui, |_| {});
        });
    }
    assert_eq!(ui.time, Duration::from_millis(5_032));
    assert!(
        (ui.dt - MAX_DT).abs() < 1e-6,
        "Ui::dt should clamp at MAX_DT; got {}",
        ui.dt,
    );

    // Frame D: prior frame's repaint_requested must NOT leak — the flag
    // resets at the top of every run_frame regardless of pass count.
    {
        let frame = ui.run_frame(display, Duration::from_millis(5_048), |ui| {
            Panel::vstack().id_salt("root").show(ui, |_| {});
        });
        assert!(
            !frame.repaint_requested(),
            "repaint_requested must reset at the top of run_frame",
        );
    }
}
