//! What a frame reshapes, what it reuses, and what the shared caches keep.

use crate::InternedStr;
use crate::Ui;
use crate::internals::harness::UiHarness;
use crate::internals::panic_probe;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::align::Align;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::layout::track::Track;
use crate::primitives::paint::color::RgbaF32;
use crate::renderer::frontend::Frontend;
use crate::scene::layer::Layer;
use crate::scene::tree::paint_anims::curves;
use crate::scene::tree::paint_anims::paint_animation::PaintAnimation;
use crate::scene::tree::paint_anims::paint_animation::PaintRepeat;
use crate::shape::Shape;
use crate::shape::record::ShapeRecord;
use crate::text::RENDERED_RUN_KEEP_FRAMES;
use crate::text::font_family::FontFamily;
use crate::text::font_slant::FontSlant;
use crate::text::font_weight::FontWeight;
use crate::text::glyph_font::GlyphFont;
use crate::text::wrap::TextWrap;
use crate::ui::frame_report::FrameProcessing;
use crate::ui::resources::UiResources;
use crate::ui::tests::support::{SURFACE, ui_with_shared};
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;
use crate::widgets::grid::Grid;
use crate::widgets::{panel::Panel, text::Text};
use glam::UVec2;
use std::time::Duration;

/// An unchanged Text hits the per-`WidgetId` reuse cache and skips shaping
/// (single-line, wrapped, grid-intrinsic paths).
#[test]
fn text_reshape_skipped_when_unchanged() {
    type Build = fn(&mut Ui);

    // A run fitting its slot resolves once, unbounded; a wider one resolves
    // again, bounded at its wrap width. The grid label fits (1); its fill-column
    // sentence is wider than what the label leaves of 200 px (2).

    let single: Build = |ui| {
        Panel::vstack().auto_id().show(ui, |ui| {
            Text::new("the quick brown fox")
                .id(WidgetId::from_hash("hello"))
                .show(ui);
        });
    };
    let wrapped: Build = |ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::fixed(60.0), Sizing::HUG))
            .show(ui, |ui| {
                Text::new("the quick brown fox jumps over the lazy dog")
                    .id(WidgetId::from_hash("wrapped"))
                    .font_size(16.0)
                    .text_wrap(TextWrap::WrapWithOverflow)
                    .show(ui);
            });
    };
    let grid_intrinsic: Build = |ui| {
        Grid::new()
            .id(WidgetId::from_hash("g"))
            .size((Sizing::fixed(200.0), Sizing::HUG))
            .cols([Track::HUG, Track::FILL])
            .show(ui, |ui| {
                Text::new("label")
                    .id(WidgetId::from_hash("hug-col-text"))
                    .grid_cell((0, 0))
                    .show(ui);
                Text::new("the quick brown fox jumps over the lazy dog")
                    .id(WidgetId::from_hash("fill-col-text"))
                    .text_wrap(TextWrap::WrapWithOverflow)
                    .grid_cell((0, 1))
                    .show(ui);
            });
    };

    for (label, build, first_frame) in [
        ("single-line", single, 1),
        ("wrapped", wrapped, 2),
        ("grid-intrinsic", grid_intrinsic, 3),
    ] {
        let mut h = UiHarness::new(UVec2::new(400, 200));
        h.frame(build);
        let after_first = h.ui.shaper().measure_calls();
        assert_eq!(after_first, first_frame, "{label}: first-frame dispatches");
        h.frame(build);
        let after_second = h.ui.shaper().measure_calls();
        assert_eq!(
            after_second,
            after_first,
            "{label}: second identical frame must reuse cached TextMeasurement \
             (extra calls: {})",
            after_second - after_first,
        );
    }
}

/// Changing the Text's content invalidates the reuse entry and re-measures.
#[test]
fn text_reshape_runs_when_content_changes() {
    let render = |content: &'static str| {
        move |ui: &mut Ui| {
            Panel::vstack().auto_id().show(ui, |ui| {
                Text::new(content)
                    .id(WidgetId::from_hash("changing"))
                    .show(ui);
            });
        }
    };
    let mut h = UiHarness::new(UVec2::new(400, 200));
    h.frame(render("first"));
    let before = h.ui.shaper().measure_calls();
    h.frame(render("second"));
    let after = h.ui.shaper().measure_calls();
    assert_eq!(
        after - before,
        1,
        "content change must trigger exactly one fresh unbounded measure",
    );
}

/// A Text that leaves the tree has its `text_reuse` entry evicted that frame.
#[test]
fn text_reuse_evicts_disappeared_widgets() {
    let mut h = UiHarness::new(UVec2::new(400, 200));
    h.frame(|ui| {
        Panel::vstack().auto_id().show(ui, |ui| {
            Text::new("hello")
                .id(WidgetId::from_hash("transient"))
                .show(ui);
        });
    });
    let wid = WidgetId::from_hash("transient");
    assert!(
        h.engines.layout.text.has_entry(wid, 0),
        "text widget should populate text_reuse on first render",
    );

    h.frame(|ui| {
        Panel::vstack().auto_id().show(ui, |_| {});
    });
    assert!(
        !h.engines.layout.text.has_entry(wid, 0),
        "removed widget's reuse entry must be swept",
    );
}

/// A widget that records fewer runs than last frame loses the rows above its
/// new count on that measure pass; `end_frame` only sweeps whole widgets.
/// Driven through the harness because the count comes from
/// `LayoutPass::shape_text_runs`.
#[test]
fn a_widget_recording_fewer_runs_loses_the_rows_above_its_count() {
    let wid = WidgetId::from_hash("multi-run");
    let build = move |runs: usize| {
        move |ui: &mut Ui| {
            Panel::vstack().auto_id().show(ui, |ui| {
                Widget::leaf().id(wid).record(ui, None, |ui| {
                    for i in 0..runs {
                        let text = ui.intern(format!("run {i}"));
                        ui.add_shape(Shape::text(text, GlyphFont::new(14.0)));
                    }
                });
            });
        }
    };

    let mut h = UiHarness::new(UVec2::new(400, 200));
    h.frame(build(3));
    for ordinal in 0..3 {
        assert!(
            h.engines.layout.text.has_entry(wid, ordinal),
            "premise: run {ordinal} took a row",
        );
    }

    h.frame(build(1));
    assert!(h.engines.layout.text.has_entry(wid, 0), "the run it kept");
    assert!(!h.engines.layout.text.has_entry(wid, 1));
    assert!(!h.engines.layout.text.has_entry(wid, 2));
}

#[test]
fn text_reuse_is_window_local_while_cosmic_buffers_are_shared() {
    fn text_window(ui: &mut Ui, content: &'static str, width: f32) {
        Panel::vstack()
            .id(WidgetId::from_hash("shared-root"))
            .size((Sizing::fixed(width), Sizing::HUG))
            .show(ui, |ui| {
                Text::new(content)
                    .id(WidgetId::from_hash("shared-text"))
                    .show(ui);
            });
    }

    let shared = UiResources::isolated_text();
    let mut a = ui_with_shared(&shared);
    let mut b = ui_with_shared(&shared);
    let text_id = WidgetId::from_hash("shared-text");

    a.frame(|ui| text_window(ui, "window A", 120.0));
    let a_key = a.ui.layout[Layer::Main].text_shapes[0].buffer_key();
    b.frame(|ui| text_window(ui, "window B", 120.0));
    let b_key = b.ui.layout[Layer::Main].text_shapes[0].buffer_key();

    assert_ne!(a_key, b_key, "different window text needs distinct keys");
    for (label, shaper) in [("A", a.ui.resources.text()), ("B", b.ui.resources.text())] {
        assert!(
            shaper.has_cosmic_buffer(a_key),
            "window {label} shares the buffer cache, so it sees A's key",
        );
        assert!(
            shaper.has_cosmic_buffer(b_key),
            "window {label} shares the buffer cache, so it sees B's key",
        );
    }
    assert!(a.engines.layout.text.has_entry(text_id, 0));
    assert!(b.engines.layout.text.has_entry(text_id, 0));

    let after_b = a.ui.resources.text().measure_calls();
    a.frame(|ui| text_window(ui, "window A", 140.0));
    assert_eq!(
        a.ui.resources.text().measure_calls(),
        after_b,
        "window B must not overwrite window A's reuse row",
    );

    b.frame(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("shared-root"))
            .size((Sizing::fixed(120.0), Sizing::HUG))
            .show(ui, |_| {});
    });
    assert!(!b.engines.layout.text.has_entry(text_id, 0));
    assert!(a.engines.layout.text.has_entry(text_id, 0));

    let after_b_removal = a.ui.resources.text().measure_calls();
    a.frame(|ui| text_window(ui, "window A", 160.0));
    assert_eq!(
        a.ui.resources.text().measure_calls(),
        after_b_removal,
        "window B removal must not evict window A's reuse row",
    );
}

const HALF: Duration = Duration::from_millis(500);

/// A leaf of `text` that blinks on a square wave, one step per [`HALF`]; its
/// timer repaint yields paint-only frames.
fn blinking_text(ui: &mut Ui, text: &str) {
    let widget = Widget::leaf().size((Sizing::fixed(160.0), Sizing::fixed(30.0)));
    widget.record(ui, None, |ui| {
        let text = ui.intern(text);
        ui.add_shape_animated(
            Shape::text(
                text,
                GlyphFont {
                    line_height: 19.2,
                    ..GlyphFont::new(16.0)
                },
            )
            .color(RgbaF32::WHITE)
            .wrap(TextWrap::SingleLine)
            .align(Align::default())
            .family(FontFamily::SANS)
            .weight(FontWeight::REGULAR),
            PaintAnimation::alpha(0.0, 1.0)
                .with_started_at(HALF)
                .with_period(HALF * 2)
                .with_steps(2)
                .with_repeat(PaintRepeat::Settle(Duration::MAX))
                .with_curve(curves::square),
        );
    });
}

/// Every frame that reaches the screen advances the shared text clock,
/// `PaintOnly` ones included.
///
/// A stalled clock makes nothing atlas-evictable (`last_use < current_frame`),
/// so a full atlas starves inserts and can't recover. The clock normally
/// ticks in `TextSystem::end_frame` (`FullRecord` only); this pins the
/// separate `PaintOnly` tick.
#[test]
fn paint_only_frames_advance_the_shared_text_clock() {
    let shared = UiResources::isolated_text();
    let mut ui = UiHarness::from_resources(shared.clone(), SURFACE);
    let shaper = ui.ui.resources.text().clone();

    let first = ui.frame(|ui| blinking_text(ui, "paint-only clock"));
    assert_eq!(first.repaint_after, Some(HALF));
    let recorded = shaper.frame();

    // Several paint-only frames in a row.
    let mut at = HALF;
    for step in 1..=3u32 {
        let report = ui
            .at(at)
            .frame(|_| panic!("PaintOnly must not re-record the tree"));
        assert_eq!(
            report.processing,
            FrameProcessing::PaintOnly,
            "step {step}: fixture must produce a paint-only frame",
        );
        assert_eq!(
            shaper.frame(),
            recorded + u64::from(step),
            "step {step}: a painted frame must advance the shared text clock",
        );
        at += HALF;
    }

    // Ageing too: entries no longer asked for must expire on paint-only frames
    // alone; a stalled clock ticks nothing out.
    let before = shaper.cache_counts();
    for step in 0..=RENDERED_RUN_KEEP_FRAMES {
        let report = ui
            .at(at)
            .frame(|_| panic!("PaintOnly must not re-record the tree"));
        assert_eq!(
            report.processing,
            FrameProcessing::PaintOnly,
            "streak step {step}: fixture must keep producing paint-only frames",
        );
        at += HALF;
    }
    let over_the_streak = shaper.cache_counts() - before;
    assert_eq!(
        over_the_streak.expiries, 1,
        "a paint-only streak past the protected window must age the one \
         shaped buffer out; counts over the streak = {over_the_streak:?}",
    );
    assert_eq!(
        over_the_streak.shapes, 0,
        "and must do it without reshaping anything — paint-only records \
         nothing, so there is nothing to shape",
    );
}

#[test]
fn shared_cache_eviction_preserves_idle_windows_paint_only_text_source() {
    let shared = UiResources::isolated_text();
    let mut idle = UiHarness::from_resources(shared.clone(), SURFACE);
    let mut active = UiHarness::from_resources(shared.clone(), SURFACE);

    let idle_first = idle.frame(|ui| blinking_text(ui, "idle interned window text"));
    assert_eq!(idle_first.repaint_after, Some(HALF));
    let idle_key = idle.ui.layout[Layer::Main].text_shapes[0].buffer_key();

    active.frame(|ui| {
        Panel::vstack().auto_id().show(ui, |ui| {
            Text::new("active window one").auto_id().show(ui);
            Text::new("active window two").auto_id().show(ui);
        });
    });
    idle.ui.resources.text().drop_cosmic_buffers();
    assert!(
        !idle.ui.resources.text().has_cosmic_buffer(idle_key),
        "the idle window's shaped buffer must be gone before the paint",
    );

    let idle_paint = idle
        .at(HALF)
        .frame(|_| panic!("PaintOnly must retain the idle window's prior tree"));
    assert_eq!(idle_paint.processing, FrameProcessing::PaintOnly);
    let plan = idle_paint
        .plan
        .expect("the animated text boundary must produce a paint plan");
    assert!(!idle.ui.resources.text().has_cosmic_buffer(idle_key));

    let mut frontend = Frontend::for_test();
    frontend.build(idle.ui.frame_scene(), plan);
    let run = frontend
        .buffer
        .texts
        .iter()
        .find(|run| run.text.key == idle_key)
        .copied()
        .expect("PaintOnly must emit the retained text run");
    let scene = idle.ui.frame_scene();
    let interned_text = scene.forest.record_store.interned_text();
    assert_eq!(
        interned_text.resolve(run.text.span),
        "idle interned window text",
        "PaintOnly must retain the source needed for backend reconstruction",
    );
    assert!(
        !idle.ui.resources.text().has_cosmic_buffer(idle_key),
        "frontend composition must not reconstruct an evicted text buffer",
    );
}

/// Changing only the wrap target keeps the cached unbounded shape; only the
/// wrap reshape reruns.
#[test]
fn wrap_target_change_preserves_unbounded_cache() {
    let render = |slot_w: f32| {
        move |ui: &mut Ui| {
            Panel::vstack()
                .auto_id()
                .size((Sizing::fixed(slot_w), Sizing::HUG))
                .show(ui, |ui| {
                    Text::new("the quick brown fox jumps over the lazy dog")
                        .id(WidgetId::from_hash("p"))
                        .font_size(16.0)
                        .text_wrap(TextWrap::WrapWithOverflow)
                        .show(ui);
                });
        }
    };

    let mut h = UiHarness::new(UVec2::new(400, 200));
    h.frame(render(60.0));
    let after_first = h.ui.shaper().measure_calls();
    assert_eq!(
        after_first, 2,
        "first frame measures unbounded, then wraps at the 60 px slot",
    );
    h.frame(render(80.0));
    let after_second = h.ui.shaper().measure_calls();
    let delta = after_second - after_first;
    assert_eq!(
        delta, 1,
        "wrap-target change must reshape only the wrap path, not unbounded \
         (extra calls: {delta})",
    );
}

/// `.bold().italic()` reach the record independently; both default to the theme's.
#[test]
fn text_face_hatches_compose_on_the_lowered_record() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        Text::new("plain").id(WidgetId::from_hash("plain")).show(ui);
        Text::new("bold")
            .id(WidgetId::from_hash("bold"))
            .bold()
            .show(ui);
        Text::new("italic")
            .id(WidgetId::from_hash("italic"))
            .italic()
            .show(ui);
        Text::new("both")
            .id(WidgetId::from_hash("both"))
            .bold()
            .italic()
            .show(ui);
    });

    let faces: Vec<(FontWeight, FontSlant)> = h.ui.forest.trees[Layer::Main]
        .shapes
        .records
        .iter()
        .map(|record| match record {
            ShapeRecord::Text { font, .. } => (font.weight, font.slant),
            shape => panic!("expected text shape, got {shape:?}"),
        })
        .collect();
    assert_eq!(
        faces,
        vec![
            (FontWeight::REGULAR, FontSlant::Normal),
            (FontWeight::BOLD, FontSlant::Normal),
            (FontWeight::REGULAR, FontSlant::Italic),
            (FontWeight::BOLD, FontSlant::Italic),
        ],
    );
}

#[test]
fn widget_text_inputs_lower_exact_bytes() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        let borrowed = String::from("borrowed");
        Text::new(borrowed.as_str())
            .id(WidgetId::from_hash("borrowed"))
            .show(ui);
        Text::new(String::from("owned"))
            .id(WidgetId::from_hash("owned"))
            .show(ui);
        let owned_interned = ui.intern(String::from("owned interned"));
        Text::new(owned_interned)
            .id(WidgetId::from_hash("owned-interned"))
            .show(ui);
        let interned = ui.intern("interned");
        let interned = ui.intern(interned);
        Text::new(interned)
            .id(WidgetId::from_hash("interned"))
            .show(ui);
        let formatted = ui.fmt(format_args!("formatted {}", 7));
        Text::new(formatted)
            .id(WidgetId::from_hash("formatted"))
            .show(ui);
    });

    let store = &h.ui.forest.record_store;
    let interned_text = store.interned_text();
    assert_eq!(
        interned_text.all(),
        "borrowedownedowned internedinternedformatted 7"
    );
    let records = &h.ui.forest.trees[Layer::Main].shapes.records;
    assert_eq!(records.len(), 5);
    for (record, expected) in records.iter().zip([
        "borrowed",
        "owned",
        "owned interned",
        "interned",
        "formatted 7",
    ]) {
        match record {
            ShapeRecord::Text { text, .. } => {
                assert_eq!(interned_text.resolve(text.span), expected);
            }
            shape => panic!("expected text shape, got {shape:?}"),
        }
    }
}

/// `InternedStr` is valid only for the record pass that minted it; the store
/// clears with a fresh epoch each pass. Three cases: a later frame, the
/// second pass of a double-layout frame, and another window.
#[test]
fn interned_handles_do_not_outlive_their_record_pass() {
    fn intern_in_own_pass(h: &mut UiHarness) -> InternedStr {
        let mut escaped = None;
        h.frame(|ui| escaped = Some(ui.intern("escapee")));
        escaped.expect("the pass ran")
    }

    let mut h = UiHarness::new(SURFACE);
    let stale = intern_in_own_pass(&mut h);
    panic_probe::assert_panics_with(
        "InternedStr outlived the record pass that minted it",
        || {
            h.frame(|ui| {
                Text::new(stale).id(WidgetId::from_hash("stale")).show(ui);
            });
        },
    );

    // Pass A interns and asks for a relayout; pass B records its handle.
    let mut h = UiHarness::new(SURFACE);
    let mut held = None;
    panic_probe::assert_panics_with(
        "InternedStr outlived the record pass that minted it",
        || {
            h.frame(|ui| match held {
                None => {
                    held = Some(ui.intern("escapee"));
                    ui.request_relayout();
                }
                Some(stale) => {
                    Text::new(stale).id(WidgetId::from_hash("pass-b")).show(ui);
                }
            });
        },
    );

    let mut source = UiHarness::new(SURFACE);
    let foreign = intern_in_own_pass(&mut source);
    let mut destination = UiHarness::new(SURFACE);
    panic_probe::assert_panics_with(
        "InternedStr outlived the record pass that minted it",
        || {
            destination.frame(|ui| {
                Text::new(foreign)
                    .id(WidgetId::from_hash("cross-window"))
                    .show(ui);
            });
        },
    );
}

/// Interning in the recording pass is the contract, including the second pass
/// of a double-layout frame, where each run mints its own handle.
#[test]
fn interning_per_pass_records_the_expected_bytes() {
    let mut h = UiHarness::cold(SURFACE);
    let mut passes = 0;
    h.frame(|ui| {
        passes += 1;
        let label = ui.intern(if passes == 1 {
            "first pass"
        } else {
            "second pass"
        });
        Text::new(label)
            .id(WidgetId::from_hash("per-pass"))
            .show(ui);
    });
    assert_eq!(passes, 2, "cold first frame must record exactly twice");

    let store = &h.ui.forest.record_store;
    let interned_text = store.interned_text();
    let records = &h.ui.forest.trees[Layer::Main].shapes.records;
    let [ShapeRecord::Text { text, .. }] = records.as_slice() else {
        panic!("expected one text shape, got {records:?}");
    };
    assert_eq!(
        interned_text.resolve(text.span),
        "second pass",
        "the recorded bytes come from the pass that survived",
    );
}

/// The shared text clock counts host frames, not window frames.
///
/// A window's first frame opens a round and ticks nothing. Two windows
/// painting in rounds tick once per round (3 rounds, 3 ticks, not 6). A
/// window repainting alone ticks on each repeat (2 ticks), none for the
/// sibling after.
#[test]
fn the_text_clock_ticks_once_per_host_frame() {
    let shared = UiResources::isolated_mono();
    let clock = || shared.text().frame();
    let mut a = ui_with_shared(&shared);
    let mut b = ui_with_shared(&shared);
    let start = clock();

    a.frame(|_| {});
    b.frame(|_| {});
    assert_eq!(clock(), start, "first frames tick nothing");
    for round in 1..=3 {
        a.frame(|_| {});
        b.frame(|_| {});
        assert_eq!(clock(), start + round, "one tick per round of both windows");
    }

    a.prime(2, |_| {});
    b.frame(|_| {});
    assert_eq!(
        clock(),
        start + 5,
        "A's two repeats tick twice, and B's frame not at all"
    );
}
