//! Damage benchmarks. Two layers:
//!
//! - **Workload benches** (`damage/workload/*`) drive realistic
//!   scenarios — repeated identical frames (Skip path), single-button
//!   color flip (single small Partial rect), two-corner indicator
//!   flip (multi-rect Partial), dense changes (most leaves dirty).
//!   Each scenario reports `end_frame` time + rect count.
//! - **Microbenches** (`damage/region/*`) hammer
//!   `DamageRegion::add` directly with synthetic rect inputs to
//!   characterise the merge-policy branches: append-cap, LVGL merge,
//!   min-growth merge.
//!
//! Decisions this bench unblocks:
//!
//! - **`DAMAGE_RECT_CAP` value (4 / 8 / 16).** Microbench shows
//!   how often min-growth fires vs append at each cap.
//! - **Encoder cost on sparse damage.** Workload benches measure
//!   end_frame on a 1k-node tree with one tiny dirty rect. If
//!   encoder dominates, the subtree-cull lever in `damage.md` is
//!   worth shipping.
//!
//! `Ui::new()` leaves the cosmic shaper unset, so text measurement
//! runs through the mono fallback (matches `frame.rs` / `caches.rs`).

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use palantir::support::internals;
use palantir::{Background, Color, Configure, Display, Frame, Panel, Rect, Sizing, Ui};
use std::hint::black_box;
use std::time::Duration;

const SURFACE: glam::UVec2 = glam::UVec2::new(1280, 800);

/// Build a 1k-ish-node grid: 32 columns × 32 rows of small frames
/// inside a vstack root. Approximates a dashboard / table-of-cells
/// workload — tightly bounded rects, mostly-disjoint screen
/// coordinates, the shape damage benefits from most.
fn build_grid(ui: &mut Ui, hot_idx: Option<usize>, hot_color: Color) {
    const COLS: usize = 32;
    const ROWS: usize = 32;
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
                            let fill = if Some(i) == hot_idx {
                                hot_color
                            } else {
                                Color::rgb(0.2, 0.2, 0.25)
                            };
                            Frame::new()
                                .id_salt(("cell", r, c))
                                .size((Sizing::Fixed(30.0), Sizing::FILL))
                                .background(Background {
                                    fill,
                                    ..Default::default()
                                })
                                .show(ui);
                        }
                    });
            }
        });
}

/// Same shape, but two cells "hot" (top-left + bottom-right corners).
/// Drives the multi-rect damage path — these two corners produce
/// disjoint damage regions that don't merge under the LVGL rule.
fn build_grid_two_hot(ui: &mut Ui, hot_color: Color) {
    const COLS: usize = 32;
    const ROWS: usize = 32;
    let tl = 0; // (row 0, col 0)
    let br = (ROWS - 1) * COLS + (COLS - 1);
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
                            let fill = if i == tl || i == br {
                                hot_color
                            } else {
                                Color::rgb(0.2, 0.2, 0.25)
                            };
                            Frame::new()
                                .id_salt(("cell", r, c))
                                .size((Sizing::Fixed(30.0), Sizing::FILL))
                                .background(Background {
                                    fill,
                                    ..Default::default()
                                })
                                .show(ui);
                        }
                    });
            }
        });
}

/// Drive the ack-the-frame contract during benches. `Ui::begin_frame`
/// auto-rewinds damage if the previous `FrameOutput` wasn't marked
/// `Submitted`. `Skip` frames self-ack at `end_frame` (since hosts
/// early-bail without submit on those), but `Partial` and `Full`
/// mark `Pending` and require an explicit submit-equivalent. The
/// ack here is unconditional and idempotent — keeps the bench
/// loop simple regardless of which path each iteration takes.
fn run_and_ack(ui: &mut Ui, display: Display, mut build: impl FnMut(&mut Ui)) {
    let out = ui.run_frame(display, Duration::ZERO, &mut build);
    internals::mark_frame_submitted(&out);
}

fn bench_workloads(c: &mut Criterion) {
    let display = Display::from_physical(SURFACE, 2.0);
    let cold = Color::rgb(0.2, 0.4, 0.8);
    let hot = Color::rgb(0.9, 0.4, 0.2);

    let mut group = c.benchmark_group("damage/workload");

    // --- skip: identical frame every iteration. -------------------
    {
        let mut ui = Ui::new();
        run_and_ack(&mut ui, display, |ui| build_grid(ui, None, cold));
        run_and_ack(&mut ui, display, |ui| build_grid(ui, None, cold));
        // Confirm we're actually exercising the Skip path, not Full
        // (first-frame escalation) — fails loud if the warmup didn't
        // settle.
        assert_eq!(internals::damage_paint_kind(&ui), "skip");

        group.bench_function("skip", |b| {
            b.iter(|| {
                run_and_ack(&mut ui, display, |ui| build_grid(ui, None, cold));
                black_box(&ui);
            });
        });
    }

    // --- single_button_change: one cell flips color each frame. ---
    {
        let mut ui = Ui::new();
        run_and_ack(&mut ui, display, |ui| build_grid(ui, Some(42), cold));
        run_and_ack(&mut ui, display, |ui| build_grid(ui, Some(42), hot));
        let kind = internals::damage_paint_kind(&ui);
        let rects = internals::damage_rect_count(&ui);
        assert_eq!(
            kind, "partial",
            "single change should be Partial; got {kind}"
        );
        assert!(
            rects <= 2,
            "single change should produce ≤2 rects (prev+curr); got {rects}",
        );

        let mut toggle = false;
        group.bench_function("single_button_change", |b| {
            b.iter(|| {
                toggle = !toggle;
                let color = if toggle { hot } else { cold };
                run_and_ack(&mut ui, display, |ui| build_grid(ui, Some(42), color));
                black_box(&ui);
            });
        });
    }

    // --- two_corner_change: two distant cells flip together. ------
    {
        let mut ui = Ui::new();
        run_and_ack(&mut ui, display, |ui| build_grid_two_hot(ui, cold));
        run_and_ack(&mut ui, display, |ui| build_grid_two_hot(ui, hot));
        let kind = internals::damage_paint_kind(&ui);
        let rects = internals::damage_rect_count(&ui);
        assert_eq!(
            kind, "partial",
            "two-corner change should be Partial; got {kind}"
        );
        assert!(
            rects >= 2,
            "two-corner change should produce ≥2 disjoint rects; got {rects}",
        );

        let mut toggle = false;
        group.bench_function("two_corner_change", |b| {
            b.iter(|| {
                toggle = !toggle;
                let color = if toggle { hot } else { cold };
                run_and_ack(&mut ui, display, |ui| build_grid_two_hot(ui, color));
                black_box(&ui);
            });
        });
    }

    // --- full_repaint: every cell gets a different color each frame.
    // Drives the Full path (damage region above coverage threshold).
    {
        let mut ui = Ui::new();
        let mut frame_n = 0u32;
        run_and_ack(&mut ui, display, |ui| build_grid(ui, None, cold));
        // Force a Full by changing every cell — colour rotation per frame.
        let varying = |ui: &mut Ui, n: u32| {
            const COLS: usize = 32;
            const ROWS: usize = 32;
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
                                    let phase = (i as u32 + n) as f32 * 0.013;
                                    let fill = Color::rgb(
                                        0.4 + (phase.sin() * 0.4),
                                        0.4 + (phase.cos() * 0.4),
                                        0.6,
                                    );
                                    Frame::new()
                                        .id_salt(("cell", r, c))
                                        .size((Sizing::Fixed(30.0), Sizing::FILL))
                                        .background(Background {
                                            fill,
                                            ..Default::default()
                                        })
                                        .show(ui);
                                }
                            });
                    }
                });
        };
        run_and_ack(&mut ui, display, |ui| varying(ui, 1));
        let kind = internals::damage_paint_kind(&ui);
        assert_eq!(kind, "full", "every-cell change should be Full; got {kind}");

        group.bench_function("full_repaint", |b| {
            b.iter(|| {
                frame_n = frame_n.wrapping_add(1);
                let n = frame_n;
                run_and_ack(&mut ui, display, |ui| varying(ui, n));
                black_box(&ui);
            });
        });
    }

    group.finish();
}

fn bench_region_add(c: &mut Criterion) {
    let mut group = c.benchmark_group("damage/region/add");

    // Disjoint corners — each call appends until cap, then min-growth
    // merges. Past N=8 we're measuring the min-growth path.
    for &n in &[1usize, 2, 4, 8, 16, 32] {
        let rects: Vec<Rect> = (0..n)
            .map(|i| Rect::new(i as f32 * 1000.0, 0.0, 5.0, 5.0))
            .collect();

        // Verify scenario: how many rects survive after add()?
        let retained = internals::damage_region_after_adds(&rects);

        group.bench_with_input(
            BenchmarkId::new("disjoint", format!("{n}_in_{retained}_out")),
            &rects,
            |b, rects| {
                b.iter(|| {
                    let count = internals::damage_region_after_adds(rects);
                    black_box(count);
                });
            },
        );
    }

    // Axis-aligned overlapping pairs — exercises the LVGL merge
    // (cascade-absorb) path. N inputs, all pairwise mergeable, should
    // collapse to 1 rect via cascade.
    for &n in &[2usize, 4, 8, 16] {
        let rects: Vec<Rect> = (0..n)
            .map(|i| Rect::new(i as f32 * 5.0, 0.0, 10.0, 10.0))
            .collect();

        let retained = internals::damage_region_after_adds(&rects);

        group.bench_with_input(
            BenchmarkId::new("overlapping", format!("{n}_in_{retained}_out")),
            &rects,
            |b, rects| {
                b.iter(|| {
                    let count = internals::damage_region_after_adds(rects);
                    black_box(count);
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_workloads, bench_region_add);
criterion_main!(benches);
