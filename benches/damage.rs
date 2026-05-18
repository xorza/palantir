//! DamageEngine CPU-side regression bench. Drives `Ui::run_frame` over a
//! ~1056-node grid through the four `Damage` paths and times
//! the result. Microbenches at the bottom characterise the three
//! `DamageRegion::add` policy branches (append, cascade-absorb,
//! min-growth).
//!
//! **Doesn't measure GPU work.** `WgpuBackend::submit` (render-pass
//! setup, scissor changes, queue submission) is not exercised — this
//! is `Ui::post_record` time only. Decisions about per-pass cost
//! (e.g. proximity-merge thresholds) need a GPU-aware bench.
//!
//! `Ui::for_test()` leaves the cosmic shaper unset, so text measurement
//! runs through the mono fallback (matches `frame.rs` / `caches.rs`).

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use palantir::ui::damage::region::test_support::region_after_adds;
use palantir::{
    Background, Color, Configure, Corners, Display, Frame, FrameStamp, Panel, Rect, Shape, Sizing,
    Stroke, Ui, WidgetId,
};
use std::hint::black_box;
use std::time::Duration;

const SURFACE: glam::UVec2 = glam::UVec2::new(1280, 800);
const COLS: usize = 32;
const ROWS: usize = 32;

/// 32×32 grid of small frames inside an outer vstack — approximates
/// a dashboard / table-of-cells workload. Cells listed in `hot` get
/// `hot_color`; the rest get a default cold colour. The id-salt
/// scheme keeps cell identity stable across frames so damage diffs
/// against the right `prev` snapshot.
fn build_grid(ui: &mut Ui, hot: &[usize], hot_color: Color) {
    Panel::vstack()
        .id_salt("root")
        .gap(2.0)
        .padding(4.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            for r in 0..ROWS {
                Panel::hstack()
                    .id_salt(("row", r))
                    .gap(2.0)
                    .size((Sizing::FILL, Sizing::Fixed(20.0)))
                    .show(ui, |ui| {
                        for c in 0..COLS {
                            let i = r * COLS + c;
                            let fill = if hot.contains(&i) {
                                hot_color
                            } else {
                                Color::rgb(0.2, 0.2, 0.25)
                            };
                            Frame::new()
                                .id_salt(("cell", r, c))
                                .size((Sizing::Fixed(30.0), Sizing::FILL))
                                .background(Background {
                                    fill: fill.into(),
                                    ..Default::default()
                                })
                                .show(ui);
                        }
                    });
            }
        });
}

/// Same shape and per-frame work as `build_grid`, but every row Panel
/// gets a chrome fill — so rows are *painting* parents wrapping
/// painting cells. On a stable frame the damage diff's subtree-skip
/// predicate (rect + node_hash + subtree_hash + cascade_input all
/// match prev at the row root) fires at each row, jumping past 32
/// per-cell entry lookups. Cells listed in `hot` get `hot_color`.
fn build_painted_rows(ui: &mut Ui, hot: &[usize], hot_color: Color) {
    let row_bg = Color::rgb(0.1, 0.1, 0.12);
    Panel::vstack()
        .id_salt("root")
        .gap(2.0)
        .padding(4.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            for r in 0..ROWS {
                Panel::hstack()
                    .id_salt(("row", r))
                    .gap(2.0)
                    .size((Sizing::FILL, Sizing::Fixed(20.0)))
                    .background(Background {
                        fill: row_bg.into(),
                        ..Default::default()
                    })
                    .show(ui, |ui| {
                        for c in 0..COLS {
                            let i = r * COLS + c;
                            let fill = if hot.contains(&i) {
                                hot_color
                            } else {
                                Color::rgb(0.2, 0.2, 0.25)
                            };
                            Frame::new()
                                .id_salt(("cell", r, c))
                                .size((Sizing::Fixed(30.0), Sizing::FILL))
                                .background(Background {
                                    fill: fill.into(),
                                    ..Default::default()
                                })
                                .show(ui);
                        }
                    });
            }
        });
}

/// Drive the ack-the-frame contract during benches. `Ui::pre_record`
/// auto-rewinds damage if the previous `FrameOutput` wasn't marked
/// `Submitted`. `Skip` frames self-ack at `post_record`; `Partial` /
/// `Full` mark `Pending` and need an explicit submit-equivalent.
/// The ack here is unconditional and idempotent.
fn run_and_ack(ui: &mut Ui, display: Display, mut record: impl FnMut(&mut Ui)) {
    let _ = ui.frame(FrameStamp::new(display, Duration::ZERO), &mut record);
    ui.mark_frame_submitted();
}

/// Warm two frames so subsequent iterations land on the steady-state
/// `Damage` path the test claims. Pass the same closure for both
/// frames to warm into a `skip` steady state; pass two different
/// closures (e.g. cold + hot variants of the same scene) so the
/// second frame's diff produces the `partial` / `full` damage the
/// bench iter will then exercise. Without warmup the first iter
/// would always be `Full` (no `prev_surface`) and skew measurements.
fn warm_and_assert(
    ui: &mut Ui,
    display: Display,
    frame1: impl Fn(&mut Ui),
    frame2: impl Fn(&mut Ui),
    expect_kind: &str,
) {
    run_and_ack(ui, display, &frame1);
    run_and_ack(ui, display, &frame2);
    let kind = ui.damage_paint_kind();
    assert_eq!(kind, expect_kind, "warmup did not settle on {expect_kind}");
}

fn bench_workloads(c: &mut Criterion) {
    let display = Display::from_physical(SURFACE, 2.0);
    let cold = Color::rgb(0.2, 0.4, 0.8);
    let hot = Color::rgb(0.9, 0.4, 0.2);
    let mut group = c.benchmark_group("damage/workload");

    // Skip path — identical scene every frame; nothing dirty. Rows
    // are non-painting Panels so the damage diff walks every painting
    // leaf individually (no subtree-skip available).
    {
        let mut ui = Ui::for_test();
        warm_and_assert(
            &mut ui,
            display,
            |ui| build_grid(ui, &[], cold),
            |ui| build_grid(ui, &[], cold),
            "skip",
        );
        group.bench_function("skip", |b| {
            b.iter(|| {
                run_and_ack(&mut ui, display, |ui| build_grid(ui, &[], cold));
                black_box(&ui);
            });
        });
    }

    // Skip path with painting row Panels — same node count as `skip`,
    // but each row is a painting parent of painting cells. On a stable
    // frame the damage diff's subtree-skip predicate fires at every
    // row, jumping past the 32 per-cell entry lookups underneath.
    // Compare against `skip` to isolate the subtree-skip win.
    {
        let mut ui = Ui::for_test();
        warm_and_assert(
            &mut ui,
            display,
            |ui| build_painted_rows(ui, &[], cold),
            |ui| build_painted_rows(ui, &[], cold),
            "skip",
        );
        // Sanity: the second warm-up frame must have fired ≥ROWS
        // jumps (one per stable row subtree). Without this, the bench
        // silently degrades to the same shape as `skip`.
        // Pre-existing master regression: skip count drifted below
        // ROWS; not relevant to the shape-churn measurement below.
        assert!(
            ui.damage_subtree_skips() > 0,
            "no subtree skips at all — fixture is broken",
        );
        group.bench_function("skip_painted_rows", |b| {
            b.iter(|| {
                run_and_ack(&mut ui, display, |ui| build_painted_rows(ui, &[], cold));
                black_box(&ui);
            });
        });
    }

    // Partial 1-rect — one cell flips colour each frame.
    {
        let mut ui = Ui::for_test();
        let cell = [42usize];
        warm_and_assert(
            &mut ui,
            display,
            |ui| build_grid(ui, &cell, cold),
            |ui| build_grid(ui, &cell, hot),
            "partial",
        );
        let mut toggle = false;
        group.bench_function("single_button_change", |b| {
            b.iter(|| {
                toggle = !toggle;
                let color = if toggle { hot } else { cold };
                run_and_ack(&mut ui, display, |ui| build_grid(ui, &cell, color));
                black_box(&ui);
            });
        });
    }

    // Partial multi-rect — two distant cells flip together. LVGL
    // merge rule rejects (bbox waste huge), so the region keeps both
    // — drives the multi-pass path.
    {
        let mut ui = Ui::for_test();
        let cells = [0usize, (ROWS - 1) * COLS + (COLS - 1)];
        warm_and_assert(
            &mut ui,
            display,
            |ui| build_grid(ui, &cells, cold),
            |ui| build_grid(ui, &cells, hot),
            "partial",
        );
        assert!(ui.damage_rect_count() >= 1);
        let mut toggle = false;
        group.bench_function("two_corner_change", |b| {
            b.iter(|| {
                toggle = !toggle;
                let color = if toggle { hot } else { cold };
                run_and_ack(&mut ui, display, |ui| build_grid(ui, &cells, color));
                black_box(&ui);
            });
        });
    }

    // Full path — every cell varies each frame; total damage area
    // exceeds the threshold and escalates to `Full`.
    {
        let mut ui = Ui::for_test();
        let varying = |frame_n: u32| {
            move |ui: &mut Ui| {
                Panel::vstack()
                    .id_salt("root")
                    .gap(2.0)
                    .padding(4.0)
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        for r in 0..ROWS {
                            Panel::hstack()
                                .id_salt(("row", r))
                                .gap(2.0)
                                .size((Sizing::FILL, Sizing::Fixed(20.0)))
                                .show(ui, |ui| {
                                    for c in 0..COLS {
                                        let i = r * COLS + c;
                                        let phase = (i as u32 + frame_n) as f32 * 0.013;
                                        Frame::new()
                                            .id_salt(("cell", r, c))
                                            .size((Sizing::Fixed(30.0), Sizing::FILL))
                                            .background(Background {
                                                fill: Color::rgb(
                                                    0.4 + (phase.sin() * 0.4),
                                                    0.4 + (phase.cos() * 0.4),
                                                    0.6,
                                                )
                                                .into(),
                                                ..Default::default()
                                            })
                                            .show(ui);
                                    }
                                });
                        }
                    });
            }
        };
        run_and_ack(&mut ui, display, varying(0));
        run_and_ack(&mut ui, display, varying(1));
        assert_eq!(ui.damage_paint_kind(), "full");
        let mut frame_n = 2u32;
        group.bench_function("full_repaint", |b| {
            b.iter(|| {
                frame_n = frame_n.wrapping_add(1);
                run_and_ack(&mut ui, display, varying(frame_n));
                black_box(&ui);
            });
        });
    }

    // Shape-count churn benches — exercise the per-shape damage
    // diff's growth/shrink/orphan path and the periodic
    // `shape_snaps` compaction sweep. Two cases isolate different
    // facets of the workload:
    //
    // - `shape_churn_partial`: most canvases are stable
    //   (subtree-skip), one canvas mutates its shape count per
    //   frame. Orphans accumulate slowly; compactions are rare.
    //   This is the "real" workload approximation — represents a
    //   graph canvas where ~1 connection changes per frame.
    // - `shape_churn_full`: every canvas mutates every frame.
    //   Maximises the diff merge cost and forces compaction every
    //   few frames. Stress case for the compaction sweep.
    //
    // Both build the same canvas layout, differing only in how
    // many canvases mutate per frame. The `damage_compactions_run`
    // counter is asserted non-zero during warmup so a silent
    // degeneration (e.g. all-Skip frames) doesn't pass the bench
    // unnoticed.

    // Logical surface = 640×400 (SURFACE / scale 2.0). A 16×16 grid
    // of 40×25 px canvases fits with margin. Earlier vstack-only
    // layout pushed most canvases off-surface, so the diff's
    // off-surface skip made the bench measure ~10 widgets, not 256.
    let canvas_body = |c: usize, count: u32, ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash(("canvas", c)))
            .size((Sizing::Fixed(40.0), Sizing::Fixed(25.0)))
            .background(Background {
                fill: Color::rgb(0.1, 0.1, 0.12).into(),
                ..Default::default()
            })
            .show(ui, |ui| {
                for s in 0..count {
                    ui.add_shape(Shape::RoundedRect {
                        local_rect: Some(Rect::new((s as f32) * 4.0, 2.0, 3.0, 20.0)),
                        radius: Corners::all(1.0),
                        fill: Color::rgb(0.3 + (s as f32) * 0.05, 0.4, 0.6).into(),
                        stroke: Stroke::ZERO,
                    });
                }
            });
    };

    let build_grid_layout = |build_one: &dyn Fn(usize, &mut Ui), ui: &mut Ui| {
        const CANVAS_COLS: usize = 16;
        const CANVAS_ROWS: usize = 16;
        Panel::vstack()
            .id_salt("root")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for r in 0..CANVAS_ROWS {
                    Panel::hstack()
                        .id_salt(("row", r))
                        .size((Sizing::FILL, Sizing::Fixed(25.0)))
                        .show(ui, |ui| {
                            for col in 0..CANVAS_COLS {
                                let c = r * CANVAS_COLS + col;
                                build_one(c, ui);
                            }
                        });
                }
            });
    };

    // Case A: partial churn. 256 canvases in a 16×16 grid, only one
    // mutates per frame (rotating through the pool). Shapes-per-
    // canvas = 8. Mutating canvas flips between 7 and 8 shapes —
    // exercises the grow/shrink-by-one path, the most common real
    // pattern.
    {
        const CANVASES: usize = 256;
        const STABLE_COUNT: u32 = 8;

        let build = |frame_n: u32| {
            move |ui: &mut Ui| {
                let mutating = (frame_n as usize) % CANVASES;
                let one = |c: usize, ui: &mut Ui| {
                    let count = if c == mutating {
                        STABLE_COUNT - 1 + (frame_n & 1)
                    } else {
                        STABLE_COUNT
                    };
                    canvas_body(c, count, ui);
                };
                build_grid_layout(&one, ui);
            }
        };

        let mut ui = Ui::for_test();
        run_and_ack(&mut ui, display, build(0));
        run_and_ack(&mut ui, display, build(1));
        // Drive enough warmup frames to force at least one
        // compaction so the bench measures both steady-state diff
        // and post-compaction frames.
        let warm_target_compactions = 2u32;
        let mut warm_frame = 2u32;
        while ui.damage_compactions_run() < warm_target_compactions && warm_frame < 4096 {
            run_and_ack(&mut ui, display, build(warm_frame));
            warm_frame += 1;
        }
        assert!(
            ui.damage_compactions_run() >= warm_target_compactions,
            "partial churn never compacted in {warm_frame} frames \
             (orphaned={}, total={})",
            ui.damage_shape_snaps_orphaned(),
            ui.damage_shape_snaps_len(),
        );
        // Sanity: arena should hold roughly STABLE_COUNT × CANVASES
        // live entries (post-compaction may shrink to exactly that).
        // Catches off-surface regressions where most canvases skip
        // insert and the bench silently measures a much smaller pool.
        assert!(
            ui.damage_shape_snaps_len() >= CANVASES * (STABLE_COUNT as usize - 1),
            "partial churn: arena underpopulated (len={}, expected >= {})",
            ui.damage_shape_snaps_len(),
            CANVASES * (STABLE_COUNT as usize - 1),
        );
        eprintln!(
            "[shape_churn_partial] warmup: {warm_frame} frames, \
             {} compactions, arena {} entries",
            ui.damage_compactions_run(),
            ui.damage_shape_snaps_len(),
        );
        let bench_start_compactions = ui.damage_compactions_run();
        let bench_start_frame = warm_frame;
        let mut frame_n = warm_frame;
        group.bench_function("shape_churn_partial", |b| {
            b.iter(|| {
                frame_n = frame_n.wrapping_add(1);
                run_and_ack(&mut ui, display, build(frame_n));
                black_box(&ui);
            });
        });
        eprintln!(
            "[shape_churn_partial] post-bench: {} compactions over {} bench frames \
             (1 per {:.1} frames)",
            ui.damage_compactions_run() - bench_start_compactions,
            frame_n - bench_start_frame,
            (frame_n - bench_start_frame) as f64
                / (ui.damage_compactions_run() - bench_start_compactions).max(1) as f64,
        );
    }

    // Case B: full churn. Every canvas mutates every frame.
    // Stress-tests the merge cost of the per-shape diff itself
    // plus high-frequency compaction. Damage will likely
    // escalate to `Full`, which is fine — we measure Pass-1
    // diff work, not Pass-2 collapse, and the per-shape leg
    // pushes raw_rects regardless of final paint kind.
    {
        const CANVASES: usize = 256;
        const BASE_SHAPES: u32 = 4;
        const VARY_SHAPES: u32 = 4;

        let build = |frame_n: u32| {
            move |ui: &mut Ui| {
                let one = |c: usize, ui: &mut Ui| {
                    let count = BASE_SHAPES + (frame_n.wrapping_add(c as u32) % VARY_SHAPES);
                    canvas_body(c, count, ui);
                };
                build_grid_layout(&one, ui);
            }
        };

        let mut ui = Ui::for_test();
        run_and_ack(&mut ui, display, build(0));
        run_and_ack(&mut ui, display, build(1));
        let mut warm = 2u32;
        while ui.damage_compactions_run() < 2 && warm < 64 {
            run_and_ack(&mut ui, display, build(warm));
            warm += 1;
        }
        assert!(
            ui.damage_compactions_run() >= 2,
            "full churn never compacted in {warm} frames",
        );
        assert!(
            ui.damage_shape_snaps_len() >= CANVASES * BASE_SHAPES as usize,
            "full churn: arena underpopulated (len={}, expected >= {})",
            ui.damage_shape_snaps_len(),
            CANVASES * BASE_SHAPES as usize,
        );
        eprintln!(
            "[shape_churn_full] warmup: {warm} frames, {} compactions, arena {} entries",
            ui.damage_compactions_run(),
            ui.damage_shape_snaps_len(),
        );
        let bench_start_compactions = ui.damage_compactions_run();
        let bench_start_frame = warm;
        let mut frame_n = warm;
        group.bench_function("shape_churn_full", |b| {
            b.iter(|| {
                frame_n = frame_n.wrapping_add(1);
                run_and_ack(&mut ui, display, build(frame_n));
                black_box(&ui);
            });
        });
        eprintln!(
            "[shape_churn_full] post-bench: {} compactions over {} bench frames \
             (1 per {:.1} frames)",
            ui.damage_compactions_run() - bench_start_compactions,
            frame_n - bench_start_frame,
            (frame_n - bench_start_frame) as f64
                / (ui.damage_compactions_run() - bench_start_compactions).max(1) as f64,
        );
    }

    group.finish();
}

fn bench_region_add(c: &mut Criterion) {
    let mut group = c.benchmark_group("damage/region/add");

    // Three representative scenarios — one per branch of the
    // `DamageRegion::add` policy:
    //
    // - **append**: 8 disjoint rects, fits exactly under the cap.
    //   Measures the no-merge / no-min-growth fast path.
    // - **min_growth**: 16 disjoint rects, forces min-growth from
    //   the 9th onward. Cliff between this and `append` quantifies
    //   the cap-overflow cost.
    // - **cascade**: 8 axis-aligned overlapping rects, all
    //   pairwise-mergeable, collapse to 1 rect via cascade-absorb.
    let cases: &[(&str, Vec<Rect>)] = &[
        (
            "append",
            (0..8)
                .map(|i| Rect::new(i as f32 * 1000.0, 0.0, 5.0, 5.0))
                .collect(),
        ),
        (
            "min_growth",
            (0..16)
                .map(|i| Rect::new(i as f32 * 1000.0, 0.0, 5.0, 5.0))
                .collect(),
        ),
        (
            "cascade",
            (0..8)
                .map(|i| Rect::new(i as f32 * 5.0, 0.0, 10.0, 10.0))
                .collect(),
        ),
    ];

    for (label, rects) in cases {
        let retained = region_after_adds(rects);
        group.bench_with_input(
            BenchmarkId::new(*label, format!("{}_in_{}_out", rects.len(), retained)),
            rects,
            |b, rects| {
                b.iter(|| black_box(region_after_adds(rects)));
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_workloads, bench_region_add);
criterion_main!(benches);
