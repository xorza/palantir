//! When a frame is asked for again, and what a paint-only one may skip.

use crate::Ui;
use crate::damage::Damage;
use crate::diagnostics::DebugOverlayConfig;
use crate::input::keyboard::key::Key;
use crate::input::policy::InputPolicy;
use crate::input::policy::InputSignal;
use crate::internals::harness::UiHarness;
use crate::internals::paint_capture::PaintCall;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::brush::gradient::linear_geometry::LinearGradient;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::primitives::paint::lut_row::LutRow;
use crate::renderer::frontend::encoder;
use crate::renderer::gradient_atlas::INITIAL_ATLAS_ROWS;
use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
use crate::renderer::render_plan::RenderPlan;
use crate::scene::layer::Layer;
use crate::shape::Shape;
use crate::ui::frame_report::FrameProcessing;
use crate::ui::resources::UiResources;
use crate::ui::tests::support::{SURFACE, add_blink_shape, ui_with_shared};
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, panel::Panel, text::Text};
use glam::Vec2;
use std::collections::HashSet;
use std::time::Duration;

/// Pin: enabling `frame_stats` records a Debug-layer text widget, keeps damage `Partial` on a static scene, and updates `fps_ema` after two frames.
#[test]
fn frame_stats_overlay_records_partial_damage() {
    let mut h = UiHarness::new(SURFACE);
    h.ui.set_debug_overlay(DebugOverlayConfig {
        frame_stats: true,
        ..h.ui.debug_overlay()
    });

    let mut body = |ui: &mut Ui| {
        Block::new()
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

    let report = h.at(Duration::from_millis(16)).frame(&mut body);
    assert!(
        matches!(
            report.plan,
            Some(RenderPlan {
                damage: Damage::Partial(..),
                ..
            })
        ),
        "frame_stats should produce Partial damage on a static scene; got {:?}",
        report.plan,
    );
    assert_eq!(
        h.ui.frame_runtime.fps_ema,
        1.0 / Duration::from_millis(16).as_secs_f32(),
        "the first reading seeds fps_ema with the instantaneous rate",
    );

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

/// Distinct deadlines coexist in the queue, surface in ascending order, and each fires on a frame at or past it.
#[test]
fn request_repaint_after_queues_distinct_deadlines() {
    let mut h = UiHarness::new(SURFACE);
    let report = h.frame(|ui| {
        ui.request_repaint_after(Duration::from_secs_f32(0.5));
        ui.request_repaint_after(Duration::from_secs_f32(1.5));
    });
    assert_eq!(
        report.repaint_after,
        Some(Duration::from_secs_f32(0.5)),
        "FrameReport must surface the earliest pending wake",
    );
    assert_eq!(
        h.ui.frame_runtime.repaint_wakes.len(),
        2,
        "both distinct deadlines stay queued"
    );

    let report = h.at(Duration::from_secs_f32(0.5)).frame(|_| {});
    assert_eq!(
        report.repaint_after,
        Some(Duration::from_secs_f32(1.5)),
        "second deadline survives the first frame's drain",
    );
    assert_eq!(h.ui.frame_runtime.repaint_wakes.len(), 1);

    let report = h.at(Duration::from_secs_f32(1.5)).frame(|_| {});
    assert_eq!(report.repaint_after, None);
    assert!(h.ui.frame_runtime.repaint_wakes.is_empty());
}

/// Re-requesting a queued deadline in one frame is a no-op. Near-duplicates within `DEFAULT_REPAINT_COALESCE_DT` (1/120 s) collapse onto the later wake; those beyond it stay distinct.
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

    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        ui.request_repaint_after(Duration::from_secs_f32(0.500));
        ui.request_repaint_after(Duration::from_secs_f32(0.504));
        ui.request_repaint_after(Duration::from_secs_f32(0.512));
        ui.request_repaint_after(Duration::from_secs_f32(0.508));
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

/// The coalesce floor tracks `Display::refresh_millihertz`: wakes 12 ms apart stay distinct at the 120 Hz fallback (8.33 ms) but collapse at 60 Hz (16.67 ms).
#[test]
fn coalesce_floor_follows_refresh_rate() {
    let schedule_pair = |h: &mut UiHarness| {
        h.frame(|ui| {
            ui.request_repaint_after(Duration::from_millis(500));
            ui.request_repaint_after(Duration::from_millis(512));
        });
    };

    let mut h = UiHarness::new(SURFACE);
    schedule_pair(&mut h);
    assert_eq!(
        h.ui.frame_runtime.repaint_wakes.len(),
        2,
        "120 Hz fallback: 12 ms-apart wakes stay distinct",
    );

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

/// Entries with `deadline <= now` drain at the top of the next frame; later ones survive.
#[test]
fn request_repaint_after_drains_fired_entries() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        ui.request_repaint_after(Duration::from_secs_f32(0.5));
        ui.request_repaint_after(Duration::from_secs_f32(1.0));
        ui.request_repaint_after(Duration::from_secs_f32(2.0));
    });
    assert_eq!(h.ui.frame_runtime.repaint_wakes.len(), 3);

    let report = h.at(Duration::from_secs_f32(1.0)).frame(|_| {});
    assert_eq!(h.ui.frame_runtime.repaint_wakes.len(), 1);
    assert_eq!(report.repaint_after, Some(Duration::from_secs_f32(2.0)));
}

/// Anim-only fast path: a lone paint-anim quantum wake skips record and post-record and emits `FrameProcessing::PaintOnly`.
#[test]
fn paint_only_fast_path_fires_on_anim_quantum_boundary() {
    fn body(ui: &mut Ui, half: Duration) {
        Panel::hstack().auto_id().show(ui, |ui| {
            Block::new()
                .id(WidgetId::from_hash("blinker"))
                .size(20.0)
                .show(ui);
            add_blink_shape(ui, half);
        });
    }

    let half = Duration::from_millis(500);

    let mut h = UiHarness::new(SURFACE);

    let r0 = h.frame(|ui| body(ui, half));
    assert_eq!(r0.processing, FrameProcessing::SingleLayout);
    assert_eq!(r0.repaint_after, Some(half));
    let (rendered, recorded) = (h.ui.render_frame_id(), h.ui.frame_id());

    let r1 = h.at(half).frame(|ui| body(ui, half));
    assert_eq!(r1.processing, FrameProcessing::PaintOnly);

    // `frame_id` must not count the painted frame, or retained state stamping it reads an idle blink as a skipped surface.
    assert_eq!(h.ui.render_frame_id(), rendered + 1);
    assert_eq!(h.ui.frame_id(), recorded);

    match r1.plan {
        Some(RenderPlan {
            damage: Damage::Partial(damage),
            ..
        }) => {
            let rects: Vec<_> = damage.region.iter_rects().collect();
            assert_eq!(rects.len(), 1, "expected single damage rect, got {rects:?}");
            let r = rects[0];
            assert_eq!(
                r,
                Rect::new(0.0, 0.0, 4.0, 12.0),
                "PaintOnly damage is the anim's tight rect",
            );
        }
        other => panic!("expected RenderPlan::Partial on PaintOnly, got {other:?}"),
    }
    assert_eq!(r1.repaint_after, Some(half + half));
    let r2 = h.at(half + half).frame(|ui| body(ui, half));
    assert_eq!(r2.processing, FrameProcessing::PaintOnly);

    // A pending close request vetoes the fast path: the app reads `close_requested` only during record.
    h.ui.window_frame.close_requested = true;
    let r3 = h.at(half * 3).frame(|ui| body(ui, half));
    assert_eq!(r3.processing, FrameProcessing::SingleLayout);
    h.ui.window_frame.close_requested = false;
    let r4 = h.at(half * 4).frame(|ui| body(ui, half));
    assert_eq!(r4.processing, FrameProcessing::PaintOnly);

    assert_eq!(h.ui.render_frame_id(), rendered + 4);
    assert_eq!(h.ui.frame_id(), recorded + 1);
}

/// Regression: `Ui::frame` cleared the record store on `PaintOnly` frames too, leaving retained `ShapeRecord`s' payload indices dangling and panicking the encoder. The clear now lives in `record_pass`. Pinned with a retained gradient, text and an animated shape forcing PaintOnly on frame 1.
#[test]
fn paint_only_preserves_record_store_for_retained_shapes() {
    fn body(ui: &mut Ui, half: Duration) {
        Panel::hstack().auto_id().show(ui, |ui| {
            Block::new()
                .id(WidgetId::from_hash("grad_bg"))
                .size(50.0)
                .background(Background {
                    fill: Brush::Linear(LinearGradient::two_stop(
                        0.0,
                        RgbaF32::srgb(1.0, 0.0, 0.0),
                        RgbaF32::srgb(0.0, 0.0, 1.0),
                    )),
                    ..Default::default()
                })
                .show(ui);
            let label = ui.fmt(format_args!("retained {}", 7));
            Text::new(label)
                .id(WidgetId::from_hash("retained-text"))
                .show(ui);
            add_blink_shape(ui, half);
        });
    }

    let half = Duration::from_millis(500);

    let mut h = UiHarness::new(SURFACE);

    let r0 = h.frame(|ui| body(ui, half));
    assert_eq!(r0.processing, FrameProcessing::SingleLayout);
    {
        let store = &h.ui.forest.record_store;
        assert_eq!(store.interned_text().all(), "retained 7");
    }

    let r1 = h.at(half).frame(|ui| body(ui, half));
    assert_eq!(r1.processing, FrameProcessing::PaintOnly);

    assert_eq!(
        h.ui.forest.record_store.gradients.records.len(),
        1,
        "PaintOnly must preserve gradient payloads so retained \
         ShapeBrush::Gradient indices remain valid",
    );
    {
        let store = &h.ui.forest.record_store;
        assert_eq!(
            store.interned_text().all(),
            "retained 7",
            "PaintOnly must preserve bytes referenced by retained text",
        );
    }

    let _ = h.encode_paint();
}

#[test]
fn paint_only_reresolves_gradient_after_other_window_evicts_its_row() {
    fn rows(ui: &Ui, atlas: &SharedGradientAtlas) -> Vec<LutRow> {
        let plan = RenderPlan {
            clear: ui.theme.window_clear,
            damage: Damage::Full,
        };
        encoder::internals::encode(ui.frame_scene(), atlas, plan)
            .calls
            .iter()
            .filter_map(|command| match command {
                PaintCall::Quad(payload) if payload.fill.lut_row != LutRow::FALLBACK => {
                    Some(payload.fill.lut_row)
                }
                _ => None,
            })
            .collect()
    }

    fn window_a(ui: &mut Ui, half: Duration) {
        Panel::hstack().size(20.0).show(ui, |ui| {
            ui.add_shape(Shape::rect(Rect::new(0.0, 0.0, 8.0, 8.0)).fill(
                LinearGradient::two_stop(0.0, RgbaF32::hex(0xff0000), RgbaF32::hex(0x0000ff)),
            ));
            add_blink_shape(ui, half);
        });
    }

    let shared = UiResources::isolated_mono();
    let atlas = shared.gradient_atlas().clone();
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
                        RgbaF32::from_srgba(SrgbaU8::rgb(
                            index as u8,
                            (index >> u8::BITS) as u8,
                            (index >> (u8::BITS * 2)) as u8,
                        )),
                        RgbaF32::WHITE,
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

/// `request_repaint` co-firing with an anim wake gives `REAL | ANIM`, so the classifier picks Full.
#[test]
fn paint_only_skipped_when_widget_requested_repaint() {
    fn body(ui: &mut Ui, half: Duration) {
        Panel::hstack().auto_id().show(ui, |ui| {
            Block::new()
                .id(WidgetId::from_hash("blinker"))
                .size(20.0)
                .show(ui);
            add_blink_shape(ui, half);
        });
    }

    let half = Duration::from_millis(500);

    let mut h = UiHarness::new(SURFACE);

    let r0 = h.frame(|ui| {
        body(ui, half);
        ui.request_repaint();
    });
    assert!(r0.repaint_requested);

    let r1 = h.at(half).frame(|ui| body(ui, half));
    assert_eq!(r1.processing, FrameProcessing::SingleLayout);
}

/// At an anim-only wake the classifier picks `PaintOnly`. Under `InputPolicy::OnDelta` an inert pointer move doesn't disqualify it; under `Always` it upgrades to `SingleLayout`. Action input (click / key / IME) always upgrades.
#[test]
fn input_policy_routes_paint_only_gate() {
    fn body(ui: &mut Ui, half: Duration) {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("inert"))
                    .size(80.0)
                    .show(ui);
                add_blink_shape(ui, half);
            });
    }

    let half = Duration::from_millis(500);

    {
        let mut h = UiHarness::new(SURFACE);
        h.ui.set_input_policy(InputPolicy::OnDelta);
        let r0 = h.frame(|ui| body(ui, half));
        assert_eq!(r0.processing, FrameProcessing::SingleLayout);

        h.move_to(Vec2::new(40.0, 40.0));
        assert_eq!(
            h.ui.input.signal_since_last_frame(),
            InputSignal::Inert,
            "an inert pointer move registers as Inert",
        );

        let r1 = h.at(half).frame(|ui| body(ui, half));
        assert_eq!(
            r1.processing,
            FrameProcessing::PaintOnly,
            "OnDelta + inert pointer move + anim wake → PaintOnly",
        );

        assert_eq!(h.ui.input.signal_since_last_frame(), InputSignal::None);
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

    {
        use crate::primitives::identity::widget_id::WidgetId;
        let mut h = UiHarness::new(SURFACE);
        h.ui.set_input_policy(InputPolicy::OnDelta);
        let _ = h.frame(|ui| body(ui, half));
        h.ui.input.set_focus(Some(WidgetId::from_hash("editor")));

        h.key(Key::Enter);
        assert_eq!(
            h.ui.input.signal_since_last_frame(),
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

/// The fps EMA reads the true frame delta; the MAX_DT clamp is for the animation integrator only. Hand-computed: sample 1 at 1 s → inst 1.0 seeds the EMA; sample 2 after a 2 s stall → inst 0.5, EMA = 1.0·0.9 + 0.5·0.1 = 0.95. The clamp would record both stalls as 10 fps (EMA 10.0).
#[test]
fn fps_ema_reads_unclamped_frame_delta() {
    let mut h = UiHarness::new(SURFACE);
    let mut noop = |_: &mut Ui| {};
    h.frame(&mut noop);
    h.at(Duration::from_secs(1)).frame(&mut noop);
    assert_eq!(h.ui.frame_runtime.fps_ema, 1.0);
    h.at(Duration::from_secs(3)).frame(&mut noop);
    assert_eq!(h.ui.frame_runtime.fps_ema, 1.0f32 * 0.9 + 0.5 * 0.1);
}

/// `Ui::request_relayout` outside a record pass is a caller error, not a silently dropped no-op; inside a record it stays legal.
#[test]
#[should_panic(expected = "outside a record pass")]
fn request_relayout_between_frames_is_a_caller_error() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|_| {});
    h.ui.request_relayout();
}

#[test]
fn request_relayout_during_record_is_honoured() {
    let mut h = UiHarness::new(SURFACE);
    let mut asked = false;
    let report = h.frame(|ui| {
        if !asked {
            asked = true;
            ui.request_relayout();
        }
    });
    assert_eq!(
        report.processing,
        FrameProcessing::DoubleLayout,
        "the request re-runs this frame's record",
    );
}

/// The record-pass gate is frame-level, not per-layer: `Ui::layer` pushes a layer without opening a node, which a per-layer gate wrongly rejected.
#[test]
fn request_relayout_is_legal_from_a_layer_scope_with_nothing_recorded_yet() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        ui.layer(Layer::Popup).show(|ui| {
            ui.request_relayout();
        });
    });
}
