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
//! `new_ui()` leaves the cosmic shaper unset, so text measurement
//! runs through the mono fallback (matches `frame.rs` / `caches.rs`).

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use palantir::support::internals;
use palantir::{
    Background, Color, Configure, Display, Frame, Panel, Rect, Sizing, TextShaper, Ui, new_handle,
};
use std::hint::black_box;
use std::time::Duration;

fn new_ui() -> Ui {
    Ui::new(TextShaper::default(), new_handle())
}

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
    let _ = ui.frame(display, Duration::ZERO, &mut (), &mut record);
    internals::mark_frame_submitted(ui);
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
    let kind = internals::damage_paint_kind(ui);
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
        let mut ui = new_ui();
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
        let mut ui = new_ui();
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
        assert!(
            internals::damage_subtree_skips(&ui) >= ROWS as u32,
            "expected >= {} subtree skips, got {}",
            ROWS,
            internals::damage_subtree_skips(&ui),
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
        let mut ui = new_ui();
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
        let mut ui = new_ui();
        let cells = [0usize, (ROWS - 1) * COLS + (COLS - 1)];
        warm_and_assert(
            &mut ui,
            display,
            |ui| build_grid(ui, &cells, cold),
            |ui| build_grid(ui, &cells, hot),
            "partial",
        );
        assert!(internals::damage_rect_count(&ui) >= 2);
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
        let mut ui = new_ui();
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
        assert_eq!(internals::damage_paint_kind(&ui), "full");
        let mut frame_n = 2u32;
        group.bench_function("full_repaint", |b| {
            b.iter(|| {
                frame_n = frame_n.wrapping_add(1);
                run_and_ack(&mut ui, display, varying(frame_n));
                black_box(&ui);
            });
        });
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
        let retained = internals::damage_region_after_adds(rects);
        group.bench_with_input(
            BenchmarkId::new(*label, format!("{}_in_{}_out", rects.len(), retained)),
            rects,
            |b, rects| {
                b.iter(|| black_box(internals::damage_region_after_adds(rects)));
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_workloads, bench_region_add);
criterion_main!(benches);
