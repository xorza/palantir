//! When a frame is asked for again, and what a paint-only one may skip.

use crate::Ui;
use crate::diagnostics::DebugOverlayConfig;
use crate::host::shared::HostShared;
use crate::primitives::background::Background;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{color::Color, rect::Rect};
use crate::renderer::render_plan::{RenderKind, RenderPlan};
use crate::renderer::texture_limit::TextureLimit;
use crate::scene::layer::Layer;
use crate::scene::node::Configure;
use crate::ui::harness::UiHarness;
use crate::ui::tests::support::{SURFACE, add_blink_shape, ui_with_shared};
use crate::widgets::{frame::Frame, panel::Panel, text::Text};
use glam::Vec2;
use std::time::Duration;

/// Pin: enabling `frame_stats` records a Debug-layer text widget,
/// keeps damage `Partial` (not `Full`) on an otherwise-static scene,
/// and updates `fps_ema` once two frames have elapsed.
#[test]
fn frame_stats_overlay_records_partial_damage() {
    let mut h = UiHarness::new(SURFACE);
    h.ui.set_debug_overlay(DebugOverlayConfig {
        frame_stats: true,
        ..h.ui.debug_overlay()
    });

    // Warm-up frame at t = 0. `fps_ema` stays zero (no prior `time` to
    // diff against), but the Debug layer should already carry the
    // readout.
    let mut body = |ui: &mut Ui| {
        Frame::new()
            .id(WidgetId::from_hash("body"))
            .size(50.0)
            .show(ui);
    };
    h.frame(&mut body);
    assert_eq!(h.ui.frame_runtime.fps_ema, 0.0);
    assert!(
        !h.ui.forest.trees[Layer::Debug].records.is_empty(),
        "Debug layer must carry the frame_stats readout",
    );

    // Second frame at t = 16ms. Main scene is unchanged; only the
    // Debug-layer readout dirties → expect `Partial`, not `Full`,
    // and not `None` either. `fps_ema` picks up its first instantaneous
    // reading (~62.5).
    let report = h.at(Duration::from_millis(16)).frame(&mut body);
    assert!(
        matches!(
            report.plan,
            Some(RenderPlan {
                kind: RenderKind::Partial { .. },
                ..
            })
        ),
        "frame_stats should produce Partial damage on a static scene; got {:?}",
        report.plan,
    );
    assert!(
        h.ui.frame_runtime.fps_ema > 0.0,
        "fps_ema must update after the second frame; got {}",
        h.ui.frame_runtime.fps_ema,
    );

    // Disabling the flag mid-stream evicts the Debug-layer node next
    // frame.
    h.ui.set_debug_overlay(DebugOverlayConfig {
        frame_stats: false,
        ..h.ui.debug_overlay()
    });
    h.at(Duration::from_millis(32)).frame(&mut body);
    assert!(
        h.ui.forest.trees[Layer::Debug].records.is_empty(),
        "Debug layer must clear once frame_stats is turned off",
    );
}

/// Multiple distinct deadlines coexist in the queue and surface
/// in ascending order; each fires independently on a frame at or
/// past its deadline.
#[test]
fn request_repaint_after_queues_distinct_deadlines() {
    let mut h = UiHarness::new(SURFACE);
    let report = h.frame(|ui| {
        ui.request_repaint_after(Duration::from_secs_f32(0.5));
        ui.request_repaint_after(Duration::from_secs_f32(1.5));
    });
    // Earliest deadline wins the report slot.
    assert_eq!(
        report.repaint_after,
        Some(Duration::from_secs_f32(0.5)),
        "FrameReport must surface the earliest pending wake",
    );
    // Both entries are still queued (neither has fired).
    assert_eq!(
        h.ui.frame_runtime.repaint_wakes.len(),
        2,
        "both distinct deadlines stay queued"
    );

    // Run a frame at the first deadline. The earliest entry drains;
    // the second survives.
    let report = h.at(Duration::from_secs_f32(0.5)).frame(|_| {});
    assert_eq!(
        report.repaint_after,
        Some(Duration::from_secs_f32(1.5)),
        "second deadline survives the first frame's drain",
    );
    assert_eq!(h.ui.frame_runtime.repaint_wakes.len(), 1);

    // Run a frame at the second deadline. Queue empties.
    let report = h.at(Duration::from_secs_f32(1.5)).frame(|_| {});
    assert_eq!(report.repaint_after, None);
    assert!(h.ui.frame_runtime.repaint_wakes.is_empty());
}

/// Re-requesting an already-queued deadline within the same frame
/// is a no-op — the queue is sorted + dedup'd. Near-duplicates within
/// `DEFAULT_REPAINT_COALESCE_DT` (1/120 s, the headless default)
/// collapse onto the later wake to minimize host wake-ups; entries
/// spaced beyond the window stay distinct.
#[test]
fn request_repaint_after_dedups_within_frame() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        for _ in 0..10 {
            ui.request_repaint_after(Duration::from_secs_f32(0.5));
        }
        ui.request_repaint_after(Duration::from_secs_f32(0.5));
    });
    assert_eq!(
        h.ui.frame_runtime.repaint_wakes.len(),
        1,
        "exact duplicate deadlines collapse to one entry",
    );

    // Near-duplicates within the 1/120 s window collapse onto the
    // later deadline (prefer the longer wait); deadlines spaced
    // beyond the window stay distinct.
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        // Earlier request first; second request lands ~4 ms later
        // (well under 1/120 s ≈ 8.33 ms). Expect the later deadline
        // to win.
        ui.request_repaint_after(Duration::from_secs_f32(0.500));
        ui.request_repaint_after(Duration::from_secs_f32(0.504));
        // Reversed order — later first, then a near-earlier
        // request. Existing later wake should suppress the earlier
        // one (same outcome: only the later survives).
        ui.request_repaint_after(Duration::from_secs_f32(0.512));
        ui.request_repaint_after(Duration::from_secs_f32(0.508));
        // Beyond the window — must stay distinct.
        ui.request_repaint_after(Duration::from_secs_f32(0.600));
    });
    let deadlines: Vec<Duration> =
        h.ui.frame_runtime
            .repaint_wakes
            .iter()
            .map(|w| w.deadline)
            .collect();
    assert_eq!(
        deadlines,
        vec![
            Duration::from_secs_f32(0.512),
            Duration::from_secs_f32(0.600),
        ],
        "near-duplicate wakes collapse onto the later deadline",
    );
}

/// The coalesce floor tracks `Display::refresh_millihertz`: two wakes
/// 12 ms apart stay distinct at the unknown-rate 120 Hz fallback
/// (≈8.33 ms window) but collapse at 60 Hz (≈16.67 ms window),
/// proving the floor is derived from the display in `schedule_wake`.
#[test]
fn coalesce_floor_follows_refresh_rate() {
    let schedule_pair = |h: &mut UiHarness| {
        h.frame(|ui| {
            ui.request_repaint_after(Duration::from_millis(500));
            ui.request_repaint_after(Duration::from_millis(512));
        });
    };

    // Unknown refresh → 120 Hz fallback: 12 ms > 8.33 ms → distinct.
    let mut h = UiHarness::new(SURFACE);
    schedule_pair(&mut h);
    assert_eq!(
        h.ui.frame_runtime.repaint_wakes.len(),
        2,
        "120 Hz fallback: 12 ms-apart wakes stay distinct",
    );

    // 60 Hz refresh → 16.67 ms window: 12 ms < window → collapse.
    let mut h = UiHarness::new(SURFACE).refresh_millihertz(60_000);
    schedule_pair(&mut h);
    assert_eq!(
        h.ui.frame_runtime.repaint_wakes.len(),
        1,
        "60 Hz floor: 12 ms-apart wakes collapse",
    );
    assert_eq!(
        h.ui.frame_runtime.repaint_wakes[0].deadline,
        Duration::from_millis(512),
        "the later deadline survives the collapse",
    );
}

/// Entries with `deadline <= now` drain at the top of the next
/// frame; entries strictly past `now` survive.
#[test]
fn request_repaint_after_drains_fired_entries() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        ui.request_repaint_after(Duration::from_secs_f32(0.5));
        ui.request_repaint_after(Duration::from_secs_f32(1.0));
        ui.request_repaint_after(Duration::from_secs_f32(2.0));
    });
    assert_eq!(h.ui.frame_runtime.repaint_wakes.len(), 3);

    // Frame at t=1.0 drains entries at 0.5 and 1.0; 2.0 survives.
    let report = h.at(Duration::from_secs_f32(1.0)).frame(|_| {});
    assert_eq!(h.ui.frame_runtime.repaint_wakes.len(), 1);
    assert_eq!(report.repaint_after, Some(Duration::from_secs_f32(2.0)));
}

// `app_state_round_trip_across_frame` and `app_without_install_panics`
// were removed when `Ui` lost its `<T>` parameter. App-owned state now
// lives in the caller's frame-builder closure (capture it) — see the
// `app_state` showcase for the canonical pattern.

/// Anim-only fast path: when the only wake fired is a paint-anim
/// quantum boundary (no input, no `request_repaint`, no real wake),
/// `Ui::frame` skips record + post-record and emits
/// `FrameProcessing::PaintOnly`.
#[test]
fn paint_only_fast_path_fires_on_anim_quantum_boundary() {
    use crate::ui::frame_report::FrameProcessing;

    let half = Duration::from_millis(500);

    fn body(ui: &mut Ui, half: Duration) {
        Panel::hstack().auto_id().show(ui, |ui| {
            Frame::new()
                .id(WidgetId::from_hash("blinker"))
                .size(20.0)
                .show(ui);
            add_blink_shape(ui, half);
        });
    }

    let mut h = UiHarness::new(SURFACE);

    // Frame 0: record. Full path; schedules anim wake at `half`.
    let r0 = h.frame(|ui| body(ui, half));
    assert_eq!(r0.processing, FrameProcessing::SingleLayout);
    assert_eq!(r0.repaint_after, Some(half));
    let (rendered, recorded) = (h.ui.render_frame_id(), h.ui.frame_id());

    // Frame 1 at the blink boundary: only anim wake fires → fast path.
    let r1 = h.at(half).frame(|ui| body(ui, half));
    assert_eq!(r1.processing, FrameProcessing::PaintOnly);

    // The two clocks part company exactly here. `render_frame_id` counts
    // the painted frame; `frame_id` must not, or retained state that stamps
    // it to notice it was skipped reads an idle blink as "my surface was
    // away" and drops whatever it had in flight.
    assert_eq!(h.ui.render_frame_id(), rendered + 1);
    assert_eq!(h.ui.frame_id(), recorded);

    // PaintOnly must emit a Partial damage plan covering the anim's
    // tight rect — not Full (defeats the point) and not None (the
    // blink phase actually flipped). Pin both invariants.
    match r1.plan {
        Some(RenderPlan {
            kind: RenderKind::Partial { damage },
            ..
        }) => {
            let rects: Vec<_> = damage.region.iter_rects().collect();
            assert_eq!(rects.len(), 1, "expected single damage rect, got {rects:?}");
            let r = rects[0];
            assert!(
                r.size.w <= 8.0 && r.size.h <= 16.0,
                "PaintOnly damage should be the anim's tight rect, got {r:?}",
            );
        }
        other => panic!("expected RenderPlan::Partial on PaintOnly, got {other:?}"),
    }
    // Bug regression: PaintOnly skips post_record, but must still
    // re-fold the retained paint_anims so the *next* blink boundary
    // is queued. Without this fold the caret stops blinking until
    // input forces a FullRecord (mouse-move regression).
    assert_eq!(r1.repaint_after, Some(half + half));
    let r2 = h.at(half + half).frame(|ui| body(ui, half));
    assert_eq!(r2.processing, FrameProcessing::PaintOnly);

    // A pending OS close request vetoes the fast path: the app can only
    // read `close_requested` (and veto via `keep_open`) during record,
    // so an anim-wake frame escalates to Full while `wants_close` is
    // set — and drops back to PaintOnly once it clears.
    h.ui.window_frame.close_requested = true;
    let r3 = h.at(half * 3).frame(|ui| body(ui, half));
    assert_eq!(r3.processing, FrameProcessing::SingleLayout);
    h.ui.window_frame.close_requested = false;
    let r4 = h.at(half * 4).frame(|ui| body(ui, half));
    assert_eq!(r4.processing, FrameProcessing::PaintOnly);

    // Four frames on from the stamp above, one of which recorded (`r3`,
    // the close-request escalation). A reader that recorded on both `r0`
    // and `r3` sees consecutive `frame_id`s across the paint-only frames
    // between them — which is what "no gap" has to mean.
    assert_eq!(h.ui.render_frame_id(), rendered + 4);
    assert_eq!(h.ui.frame_id(), recorded + 1);
}

/// Regression: `Ui::frame` used to clear `record_store` unconditionally
/// at entry, including on `PaintOnly` frames. But on PaintOnly the
/// record pass is skipped, so `tree.shapes` retains last frame's
/// `ShapeRecord`s — which reference record payloads by index
/// (`ShapeBrush::Gradient(id)`, polyline/mesh spans, arena-backed text
/// spans). Clearing left those indices dangling; the encoder then
/// panicked on the first gradient lookup with
/// `index out of bounds: the len is 0 but the index is N`.
/// Fix: clear inside `record_pass` instead (only fires when we're
/// rebuilding shapes). This test pins it with retained gradient and
/// recorded text entries plus an animated shape that forces
/// PaintOnly on frame 1, then re-runs the encoder.
#[test]
fn paint_only_preserves_record_store_for_retained_shapes() {
    use crate::primitives::brush::Brush;
    use crate::primitives::brush::gradient::linear_geometry::LinearGradient;
    use crate::ui::frame_report::FrameProcessing;

    let half = Duration::from_millis(500);

    fn body(ui: &mut Ui, half: Duration) {
        Panel::hstack().auto_id().show(ui, |ui| {
            // Gradient-filled chrome: `lower::background` interns a
            // `RecordedGradient` into `RecordPayloads::gradients` each record
            // pass, and the resulting `ChromeRow` stores the index.
            Frame::new()
                .id(WidgetId::from_hash("grad_bg"))
                .size(50.0)
                .background(Background {
                    fill: Brush::Linear(LinearGradient::two_stop(
                        0.0,
                        Color::rgb(1.0, 0.0, 0.0),
                        Color::rgb(0.0, 0.0, 1.0),
                    )),
                    ..Default::default()
                })
                .show(ui);
            let label = ui.fmt(format_args!("retained {}", 7));
            Text::new(label)
                .id(WidgetId::from_hash("retained-text"))
                .show(ui);
            // Animated shape, drives the PaintOnly wake on frame 1.
            add_blink_shape(ui, half);
        });
    }

    let mut h = UiHarness::new(SURFACE);

    // Frame 0: full record. Populates the gradient payloads and stamps
    // `ShapeBrush::Gradient(GradientId(0))` into the chrome row for the frame.
    let r0 = h.frame(|ui| body(ui, half));
    assert_eq!(r0.processing, FrameProcessing::SingleLayout);
    {
        let payloads = h.ui.forest.record_store.payloads.borrow();
        assert_eq!(payloads.interned_text().bytes, "retained 7");
    }

    // Frame 1 at the blink boundary: only the anim wake fires →
    // PaintOnly. With the old (buggy) clear, the gradient payloads
    // would be empty here and the encoder below would panic.
    let r1 = h.at(half).frame(|ui| body(ui, half));
    assert_eq!(r1.processing, FrameProcessing::PaintOnly);

    // Direct pin: the gradient interned during frame 0's record must
    // still be live for the encoder on a PaintOnly frame.
    assert_eq!(
        h.ui.forest
            .record_store
            .payloads
            .borrow()
            .gradients
            .records
            .len(),
        1,
        "PaintOnly must preserve gradient payloads so retained \
         ShapeBrush::Gradient indices remain valid",
    );
    {
        let payloads = h.ui.forest.record_store.payloads.borrow();
        assert_eq!(
            payloads.interned_text().bytes,
            "retained 7",
            "PaintOnly must preserve bytes referenced by retained text",
        );
    }

    // Indirect pin: re-run the encoder against the retained tree
    // + record store. With the bug, this panicked on `gradients[id]`.
    let _ = h.encode_paint();
}

#[test]
fn paint_only_reresolves_gradient_after_other_window_evicts_its_row() {
    use crate::primitives::brush::gradient::linear_geometry::LinearGradient;
    use crate::primitives::color::ColorU8;

    use crate::primitives::lut_row::LutRow;

    use crate::renderer::frontend::capture::PaintCall;
    use crate::renderer::frontend::encoder;
    use crate::renderer::gradient_atlas::INITIAL_ATLAS_ROWS;
    use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
    use crate::shape::Shape;
    use crate::text::shaper::TextShaper;
    use crate::ui::frame_report::FrameProcessing;
    use std::collections::HashSet;

    fn rows(ui: &Ui, atlas: &SharedGradientAtlas) -> Vec<LutRow> {
        let plan = RenderPlan {
            clear: ui.theme.window_clear,
            kind: RenderKind::Full,
        };
        encoder::test_support::encode(ui.frame_scene(), atlas, plan)
            .calls
            .iter()
            .filter_map(|command| match command {
                PaintCall::Quad(payload) if payload.fill_lut_row != LutRow::FALLBACK => {
                    Some(payload.fill_lut_row)
                }
                _ => None,
            })
            .collect()
    }

    fn window_a(ui: &mut Ui, half: Duration) {
        Panel::hstack().size(20.0).show(ui, |ui| {
            ui.add_shape(Shape::rect(Rect::new(0.0, 0.0, 8.0, 8.0)).fill(
                LinearGradient::two_stop(0.0, ColorU8::rgb(255, 0, 0), ColorU8::rgb(0, 0, 255)),
            ));
            add_blink_shape(ui, half);
        });
    }

    let shared = HostShared::new(TextShaper::test_mono(), TextureLimit::default());
    let atlas = shared.gradient_atlas.clone();
    let mut a = ui_with_shared(&shared);
    let mut b = ui_with_shared(&shared);
    let half = Duration::from_millis(500);

    a.frame(|ui| window_a(ui, half));
    let original_row = rows(&a.ui, &atlas)[0];
    atlas.flush_with(|_| ());

    b.frame(|ui| {
        Panel::hstack().size(20.0).show(ui, |ui| {
            for index in 0..INITIAL_ATLAS_ROWS - 1 {
                ui.add_shape(Shape::rect(Rect::new(0.0, 0.0, 8.0, 8.0)).fill(
                    LinearGradient::two_stop(
                        0.0,
                        ColorU8::rgb(
                            index as u8,
                            (index >> u8::BITS) as u8,
                            (index >> (u8::BITS * 2)) as u8,
                        ),
                        ColorU8::WHITE,
                    ),
                ));
            }
        });
    });
    let b_rows: HashSet<LutRow> = rows(&b.ui, &atlas).into_iter().collect();
    assert_eq!(b_rows.len(), (INITIAL_ATLAS_ROWS - 1) as usize);
    assert!(b_rows.contains(&original_row));
    atlas.flush_with(|_| ());

    let report = a.at(half).frame(|ui| window_a(ui, half));
    assert_eq!(report.processing, FrameProcessing::PaintOnly);
    let resolved_row = rows(&a.ui, &atlas)[0];
    assert_ne!(
        resolved_row, original_row,
        "PaintOnly must resolve retained gradient content after its old row is reused",
    );
}

/// `request_repaint` co-firing with an anim wake produces the
/// `REAL | ANIM` mix, so the classifier picks Full.
#[test]
fn paint_only_skipped_when_widget_requested_repaint() {
    use crate::ui::frame_report::FrameProcessing;

    let half = Duration::from_millis(500);

    fn body(ui: &mut Ui, half: Duration) {
        Panel::hstack().auto_id().show(ui, |ui| {
            Frame::new()
                .id(WidgetId::from_hash("blinker"))
                .size(20.0)
                .show(ui);
            add_blink_shape(ui, half);
        });
    }

    let mut h = UiHarness::new(SURFACE);

    // Frame 0: record + `request_repaint`. Next frame must be Full.
    let r0 = h.frame(|ui| {
        body(ui, half);
        ui.request_repaint();
    });
    assert!(r0.repaint_requested);

    let r1 = h.at(half).frame(|ui| body(ui, half));
    assert_eq!(r1.processing, FrameProcessing::SingleLayout);
}

/// At an anim-only wake boundary, the classifier picks `PaintOnly`.
/// Under `InputPolicy::OnDelta` (default) an inert pointer move
/// since the last frame doesn't disqualify it — `requests_repaint`
/// stayed `false`. Under `InputPolicy::Always` the same input
/// upgrades the frame to `SingleLayout`.
///
/// Action input (click / key / IME) is unconditionally upgraded
/// under both policies because `on_input` returns
/// `requests_repaint = true` for them — exercised in the second
/// half of the test.
#[test]
fn input_policy_routes_paint_only_gate() {
    use crate::input::keyboard::Key;
    use crate::input::policy::{InputPolicy, InputSignal};
    use crate::ui::frame_report::FrameProcessing;

    let half = Duration::from_millis(500);

    // Body declares an inert Frame *and* an anim shape so the next
    // frame's wake fires `ANIM`. Pointer-over-inert hits no Sense
    // entry, so OnDelta sees `requests_repaint = false`.
    fn body(ui: &mut Ui, half: Duration) {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("inert"))
                    .size(80.0)
                    .show(ui);
                add_blink_shape(ui, half);
            });
    }

    {
        let mut h = UiHarness::new(SURFACE);
        h.ui.set_input_policy(InputPolicy::OnDelta);
        let r0 = h.frame(|ui| body(ui, half));
        assert_eq!(r0.processing, FrameProcessing::SingleLayout);

        h.move_to(Vec2::new(40.0, 40.0));
        // One assertion for what used to need two: the move arrived,
        // and it was not repaint-worthy.
        assert_eq!(
            h.ui.input.signal_since_last_frame,
            InputSignal::Inert,
            "an inert pointer move registers as Inert",
        );

        let r1 = h.at(half).frame(|ui| body(ui, half));
        assert_eq!(
            r1.processing,
            FrameProcessing::PaintOnly,
            "OnDelta + inert pointer move + anim wake → PaintOnly",
        );

        // PaintOnly path must have drained the input signal and queues.
        assert_eq!(h.ui.input.signal_since_last_frame, InputSignal::None);
    }

    {
        let mut h = UiHarness::new(SURFACE);
        h.ui.set_input_policy(InputPolicy::Always);
        let _ = h.frame(|ui| body(ui, half));

        h.move_to(Vec2::new(40.0, 40.0));
        let r1 = h.at(half).frame(|ui| body(ui, half));
        assert_eq!(
            r1.processing,
            FrameProcessing::SingleLayout,
            "Always + any input forces SingleLayout",
        );
    }

    // only with focus or a chord watcher, so prime focus first.
    {
        use crate::primitives::widget_id::WidgetId;
        let mut h = UiHarness::new(SURFACE);
        h.ui.set_input_policy(InputPolicy::OnDelta);
        let _ = h.frame(|ui| body(ui, half));
        h.ui.input.focused = Some(WidgetId::from_hash("editor"));

        h.key(Key::Enter);
        assert_eq!(
            h.ui.input.signal_since_last_frame,
            InputSignal::Repaint,
            "KeyDown with focus held must raise the signal to Repaint",
        );
        let r1 = h.at(half).frame(|ui| body(ui, half));
        assert_ne!(
            r1.processing,
            FrameProcessing::PaintOnly,
            "OnDelta must not pick PaintOnly on action input",
        );
    }
}

//
// Pin the first-frame behavior added to `Ui::frame`: when the
// recorder has never run before, do a blackout record pass (input
// swapped for `InputState::default()`) to build the cascade, then
// re-route the held `pointer_pos` against it before the user-visible
// pass. Tests below intentionally construct a bare `Ui` to exercise true
// cold-start; `UiHarness::new(SURFACE)` pre-marks the recorder warm to keep the
// rest of the test suite on single-record semantics.

/// The fps EMA reads the TRUE frame delta — the MAX_DT clamp is for
/// the animation integrator only. Hand-computed: sample 1 at 1 s →
/// inst 1.0 seeds the EMA; sample 2 after a 2 s stall → inst 0.5,
/// EMA = 1.0·0.9 + 0.5·0.1 = 0.95. The clamp would have recorded both
/// stalls as 10 fps samples (EMA 10.0), reporting a HIGHER rate the
/// longer the stall.
#[test]
fn fps_ema_reads_unclamped_frame_delta() {
    let mut h = UiHarness::new(SURFACE);
    let mut noop = |_: &mut Ui| {};
    h.frame(&mut noop);
    h.at(Duration::from_secs(1)).frame(&mut noop);
    assert!(
        (h.ui.frame_runtime.fps_ema - 1.0).abs() < 1e-6,
        "got {}",
        h.ui.frame_runtime.fps_ema
    );
    h.at(Duration::from_secs(3)).frame(&mut noop);
    assert!(
        (h.ui.frame_runtime.fps_ema - 0.95).abs() < 1e-6,
        "got {}",
        h.ui.frame_runtime.fps_ema
    );
}

/// `Ui::request_relayout` outside a record pass is a caller error, not a
/// no-op.
///
/// It re-runs *this* frame's record after measure, so there is nothing
/// for it to retry when no record is in flight — and `FrameCycle::run`
/// clears the flag on its way in, so an out-of-frame call used to set a
/// bit the next line dropped. The retry silently never happened. Inside
/// a record it stays legal, which is what the second half pins.
#[test]
#[should_panic(expected = "outside a record pass")]
fn request_relayout_between_frames_is_a_caller_error() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|_| {});
    // Between frames: no node open, so no record is in flight.
    h.ui.request_relayout();
}

#[test]
fn request_relayout_during_record_is_honoured() {
    let mut h = UiHarness::new(SURFACE);
    let mut asked = false;
    h.frame(|ui| {
        if !asked {
            asked = true;
            ui.request_relayout();
        }
    });
    assert!(asked, "the record closure ran");
}

/// The record-pass gate is a *frame*-level question, not a per-layer
/// one. `Ui::layer` pushes a layer without opening anything in it, so a
/// gate that asked "is a node open on the current layer" rejected a
/// perfectly legal call made from an overlay scope before that scope had
/// recorded its first widget.
#[test]
fn request_relayout_is_legal_from_a_layer_scope_with_nothing_recorded_yet() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        ui.layer(Layer::Popup).show(|ui| {
            // First statement in the overlay body: the layer is pushed,
            // but nothing has been recorded into it yet.
            ui.request_relayout();
        });
    });
}
