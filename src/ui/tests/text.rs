//! What a frame reshapes, what it reuses, and what the shared caches keep.

use crate::TextStyle;
use crate::Ui;
use crate::host::shared::HostShared;
use crate::primitives::color::Color;
use crate::primitives::widget_id::WidgetId;
use crate::renderer::frontend::Frontend;
use crate::renderer::texture_limit::TextureLimit;
use crate::scene::layer::Layer;
use crate::scene::node::configure::Configure;
use crate::text::RENDERED_RUN_KEEP_FRAMES;
use crate::text::glyph_font::GlyphFont;
use crate::text::wrap::TextWrap;
use crate::ui::harness::UiHarness;
use crate::ui::tests::support::{SURFACE, measure_calls, ui_with_shared};
use crate::widgets::{panel::Panel, text::Text};
use glam::UVec2;
use std::time::Duration;

/// Per-`WidgetId` text reuse cache: an unchanged Text across frames
/// must hit the reuse-slot cache and skip shaping. Covers
/// single-line, wrapped, and grid-intrinsic-query paths.
#[test]
fn text_reshape_skipped_when_unchanged() {
    use crate::layout::types::{sizing::Sizing, track::Track};
    use crate::widgets::{grid::Grid, text::Text};

    type Build = fn(&mut Ui);

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
                    .style(&TextStyle::default().with_font_size(16.0))
                    .text_wrap(TextWrap::WrapWithOverflow)
                    .show(ui);
            });
    };
    let grid_intrinsic: Build = |ui| {
        Grid::new()
            .id(WidgetId::from_hash("g"))
            .size((Sizing::fixed(200.0), Sizing::HUG))
            .cols([Track::hug(), Track::fill()])
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

    for (label, build) in [
        ("single-line", single),
        ("wrapped", wrapped),
        ("grid-intrinsic", grid_intrinsic),
    ] {
        let mut h = UiHarness::new(UVec2::new(400, 200));
        h.frame(build);
        let after_first = measure_calls(&h.ui);
        assert!(
            after_first > 0,
            "{label}: first frame should drive at least one measure call",
        );
        h.frame(build);
        let after_second = measure_calls(&h.ui);
        assert_eq!(
            after_second,
            after_first,
            "{label}: second identical frame must reuse cached TextMeasurement \
             (extra calls: {})",
            after_second - after_first,
        );
    }
}

/// Pin: changing the Text's content invalidates the reuse entry and
/// drives a fresh measure.
#[test]
fn text_reshape_runs_when_content_changes() {
    use crate::widgets::text::Text;

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
    let before = measure_calls(&h.ui);
    h.frame(render("second"));
    let after = measure_calls(&h.ui);
    assert!(
        after > before,
        "content change must trigger fresh measure (before={before}, after={after})",
    );
}

/// Pin: when a Text widget disappears from the tree, its `text_reuse`
/// entry is evicted on the same frame.
#[test]
fn text_reuse_evicts_disappeared_widgets() {
    use crate::widgets::text::Text;

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

#[test]
fn text_reuse_is_window_local_while_cosmic_buffers_are_shared() {
    use crate::layout::types::sizing::Sizing;
    use crate::text::shaper::TextShaper;

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

    let shared = HostShared::new(TextShaper::new(), TextureLimit::default());
    let mut a = ui_with_shared(&shared);
    let mut b = ui_with_shared(&shared);
    let text_id = WidgetId::from_hash("shared-text");

    a.frame(|ui| text_window(ui, "window A", 120.0));
    let a_key = a.ui.layout[Layer::Main].text_shapes[0].key;
    b.frame(|ui| text_window(ui, "window B", 120.0));
    let b_key = b.ui.layout[Layer::Main].text_shapes[0].key;

    assert_ne!(a_key, b_key, "different window text needs distinct keys");
    for (label, shaper) in [("A", &a.ui.resources.text), ("B", &b.ui.resources.text)] {
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

    let after_b = a.ui.resources.text.measure_calls();
    a.frame(|ui| text_window(ui, "window A", 140.0));
    assert_eq!(
        a.ui.resources.text.measure_calls(),
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

    let after_b_removal = a.ui.resources.text.measure_calls();
    a.frame(|ui| text_window(ui, "window A", 160.0));
    assert_eq!(
        a.ui.resources.text.measure_calls(),
        after_b_removal,
        "window B removal must not evict window A's reuse row",
    );
}

/// Every frame that reaches the screen advances the shared text clock,
/// `PaintOnly` ones included.
///
/// Retention is the mild half of why. The sharp half is the glyph
/// atlas: `eviction_candidate` only offers a slot whose `last_use <
/// current_frame`, so a stalled clock means *nothing* is evictable. A
/// full atlas then starves every insert — `Rasterized::AtlasFull`,
/// glyphs dropped from painted text — and cannot recover on its own,
/// because the only thing that would free a slot is the clock the
/// paint-only streak is not turning.
///
/// The clock ticks in `TextSystem::end_full_record`, which lives in
/// `finalize_frame` and so runs only for `FullRecord`. This pins the
/// separate tick the `PaintOnly` arm owes.
#[test]
fn paint_only_frames_advance_the_shared_text_clock() {
    use crate::host::shared::HostShared;
    use crate::layout::types::align::Align;
    use crate::layout::types::sizing::Sizing;
    use crate::scene::node::Node;
    use crate::scene::tree::paint_anims::PaintAnim;
    use crate::shape::Shape;
    use crate::text::shaper::TextShaper;
    use crate::text::{FontFamily, FontWeight};
    use crate::ui::frame_report::FrameProcessing;

    const HALF: Duration = Duration::from_millis(500);

    // A blinking text boundary is what makes the harness produce
    // paint-only frames at all: it repaints on a timer without
    // re-recording.
    fn blinking_text(ui: &mut Ui) {
        let mut node = Node::leaf();
        node.size = Some((Sizing::fixed(160.0), Sizing::fixed(30.0)).into());
        ui.widget(node).record(ui, None, |ui| {
            let text = ui.intern("paint-only clock");
            ui.add_shape_animated(
                Shape::text(
                    text,
                    GlyphFont {
                        line_height_px: 19.2,
                        ..GlyphFont::new(16.0)
                    },
                )
                .color(Color::WHITE)
                .wrap(TextWrap::SingleLine)
                .align(Align::default())
                .family(FontFamily::Sans)
                .weight(FontWeight::Regular),
                PaintAnim::BlinkOpacity {
                    half_period: HALF,
                    started_at: HALF,
                    stop_after: Duration::MAX,
                },
            );
        });
    }

    let shared = HostShared::new(TextShaper::new(), TextureLimit::default());
    let mut ui = UiHarness::from_resources(shared.resources.clone(), SURFACE);
    let shaper = ui.ui.resources.text.clone();

    let first = ui.frame(blinking_text);
    assert_eq!(first.repaint_after, Some(HALF));
    let recorded = shaper.frame();

    // Several paint-only frames in a row: the streak that used to
    // freeze the clock outright.
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

    // The clock advancing is only half of it. What the streak has to
    // produce is *ageing*: a populated cache whose entries are no longer
    // being asked for must reach its retention window and expire, on
    // paint-only frames alone. A stalled clock ticks nothing out, which
    // is the failure this streak is long enough to see — the run's
    // buffer was promoted to the protected window when it was recorded,
    // and no paint-only frame looks it up again.
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
    assert!(
        over_the_streak.expiries > 0,
        "a paint-only streak past the protected window must age the \
         shaped-buffer cache; counts over the streak = {over_the_streak:?}",
    );
    assert_eq!(
        over_the_streak.shapes, 0,
        "and must do it without reshaping anything — paint-only records \
         nothing, so there is nothing to shape",
    );
}

#[test]
fn shared_cache_eviction_preserves_idle_windows_paint_only_text_source() {
    use crate::host::shared::HostShared;
    use crate::layout::types::align::Align;
    use crate::layout::types::sizing::Sizing;
    use crate::scene::node::Node;
    use crate::scene::tree::paint_anims::PaintAnim;
    use crate::shape::Shape;
    use crate::text::shaper::TextShaper;
    use crate::text::{FontFamily, FontWeight};
    use crate::ui::frame_report::FrameProcessing;

    const HALF: Duration = Duration::from_millis(500);

    fn idle_body(ui: &mut Ui) {
        let mut node = Node::leaf();
        node.size = Some((Sizing::fixed(160.0), Sizing::fixed(30.0)).into());
        ui.widget(node).record(ui, None, |ui| {
            let text = ui.intern("idle interned window text");
            ui.add_shape_animated(
                Shape::text(
                    text,
                    GlyphFont {
                        line_height_px: 19.2,
                        ..GlyphFont::new(16.0)
                    },
                )
                .color(Color::WHITE)
                .wrap(TextWrap::SingleLine)
                .align(Align::default())
                .family(FontFamily::Sans)
                .weight(FontWeight::Regular),
                PaintAnim::BlinkOpacity {
                    half_period: HALF,
                    started_at: HALF,
                    stop_after: Duration::MAX,
                },
            );
        });
    }

    let shared = HostShared::new(TextShaper::new(), TextureLimit::default());
    let mut idle = UiHarness::from_resources(shared.resources.clone(), SURFACE);
    let mut active = UiHarness::from_resources(shared.resources.clone(), SURFACE);

    let idle_first = idle.frame(idle_body);
    assert_eq!(idle_first.repaint_after, Some(HALF));
    let idle_key = idle.ui.layout[Layer::Main].text_shapes[0].key;

    active.frame(|ui| {
        Panel::vstack().auto_id().show(ui, |ui| {
            Text::new("active window one").auto_id().show(ui);
            Text::new("active window two").auto_id().show(ui);
        });
    });
    idle.ui.resources.text.drop_cosmic_buffers();
    assert!(
        !idle.ui.resources.text.has_cosmic_buffer(idle_key),
        "the idle window's shaped buffer must be gone before the paint",
    );

    let idle_paint = idle
        .at(HALF)
        .frame(|_| panic!("PaintOnly must retain the idle window's prior tree"));
    assert_eq!(idle_paint.processing, FrameProcessing::PaintOnly);
    let plan = idle_paint
        .plan
        .expect("the animated text boundary must produce a paint plan");
    assert!(!idle.ui.resources.text.has_cosmic_buffer(idle_key));

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
    let interned_text = scene.payloads.interned_text();
    assert_eq!(
        run.text.source.resolve(&interned_text),
        "idle interned window text",
        "PaintOnly must retain the source needed for backend reconstruction",
    );
    assert!(
        !idle.ui.resources.text.has_cosmic_buffer(idle_key),
        "frontend composition must not reconstruct an evicted text buffer",
    );
}

/// Pin: when authoring is unchanged but the wrap target (parent's
/// available width) shifts between frames, the cached *unbounded* shape
/// is preserved — only the *wrap* reshape runs again.
#[test]
fn wrap_target_change_preserves_unbounded_cache() {
    use crate::layout::types::sizing::Sizing;
    use crate::widgets::text::Text;

    let render = |slot_w: f32| {
        move |ui: &mut Ui| {
            Panel::vstack()
                .auto_id()
                .size((Sizing::fixed(slot_w), Sizing::HUG))
                .show(ui, |ui| {
                    Text::new("the quick brown fox jumps over the lazy dog")
                        .id(WidgetId::from_hash("p"))
                        .style(&TextStyle::default().with_font_size(16.0))
                        .text_wrap(TextWrap::WrapWithOverflow)
                        .show(ui);
                });
        }
    };

    let mut h = UiHarness::new(UVec2::new(400, 200));
    h.frame(render(60.0));
    let after_first = measure_calls(&h.ui);
    assert!(
        after_first >= 2,
        "first frame should measure both unbounded and wrap (got {after_first})",
    );
    h.frame(render(80.0));
    let after_second = measure_calls(&h.ui);
    let delta = after_second - after_first;
    assert_eq!(
        delta, 1,
        "wrap-target change must reshape only the wrap path, not unbounded \
         (extra calls: {delta})",
    );
}

#[test]
fn widget_text_inputs_lower_exact_bytes() {
    use crate::scene::shapes::record::ShapeRecord;

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

    let payloads = h.ui.forest.record_store.payloads.borrow();
    let interned_text = payloads.interned_text();
    assert_eq!(
        interned_text.bytes,
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
                assert_eq!(text.source.resolve(&interned_text), expected);
            }
            shape => panic!("expected text shape, got {shape:?}"),
        }
    }
}

/// `InternedStr` is valid for the record pass that minted it and no
/// longer. The store clears its arena and takes a fresh epoch at the top
/// of every pass, so a handle held across one addresses bytes that are
/// gone — and resolving it anyway would record whatever text now sits at
/// those offsets, which is a wrong-label bug with nothing to trace it
/// back to.
///
/// Three cases, one rule. A later *frame*; the second pass of a
/// double-layout *frame*, which is the easy one to trip by caching a
/// handle in app state; and another *window*, whose store never shared
/// the epoch at all.
#[test]
fn interned_handles_do_not_outlive_their_record_pass() {
    use crate::InternedStr;

    fn intern_in_own_pass(h: &mut UiHarness) -> InternedStr {
        let mut escaped = None;
        h.frame(|ui| escaped = Some(ui.intern("escapee")));
        escaped.expect("the pass ran")
    }

    // A later frame in the same window.
    let mut h = UiHarness::new(SURFACE);
    let stale = intern_in_own_pass(&mut h);
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        h.frame(|ui| {
            Text::new(stale).id(WidgetId::from_hash("stale")).show(ui);
        });
    }));
    assert!(
        caught.is_err(),
        "a handle from a previous frame must not lower"
    );

    // Another window, which never shared the epoch.
    let mut source = UiHarness::new(SURFACE);
    let foreign = intern_in_own_pass(&mut source);
    let mut destination = UiHarness::new(SURFACE);
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        destination.frame(|ui| {
            Text::new(foreign)
                .id(WidgetId::from_hash("cross-window"))
                .show(ui);
        });
    }));
    assert!(
        caught.is_err(),
        "a handle from another window must not lower"
    );
}

/// The rule the panic above enforces, stated positively: interning in
/// the pass that records is the whole contract, and it holds across the
/// second pass of a double-layout frame — where the closure runs twice
/// and each run mints its own handle.
#[test]
fn interning_per_pass_records_the_expected_bytes() {
    use crate::scene::shapes::record::ShapeRecord;

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

    let payloads = h.ui.forest.record_store.payloads.borrow();
    let interned_text = payloads.interned_text();
    let records = &h.ui.forest.trees[Layer::Main].shapes.records;
    let [ShapeRecord::Text { text, .. }] = records.as_slice() else {
        panic!("expected one text shape, got {records:?}");
    };
    assert_eq!(
        text.source.resolve(&interned_text),
        "second pass",
        "the recorded bytes come from the pass that survived",
    );
}
