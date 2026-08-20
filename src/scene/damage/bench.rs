//! DamageEngine CPU-side regression bench. Drives `Ui::frame` over a
//! ~1056-node grid through the four `Damage` paths and times
//! the result. Microbenches at the bottom characterise the three
//! `DamageRegion::add` policy branches (append, cascade-absorb,
//! min-growth).
//!
//! **Doesn't measure GPU work.** `WgpuBackend::submit` (render-pass
//! setup, scissor changes, queue submission) is not exercised — this
//! is `FrameCycle::post_record` time only. Decisions about per-pass cost
//! (e.g. proximity-merge thresholds) need a GPU-aware bench.
//!
//! `UiHarness::new(SURFACE)` leaves the cosmic shaper unset, so text measurement
//! runs through the mono fallback (matches the frame and measure-cache
//! benches).

use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::scene::damage::Damage;
use crate::scene::damage::region::DamageRegion;
use crate::scene::node::Configure;
use crate::shape::Shape;
use crate::ui::Ui;
use crate::ui::harness::UiHarness;
use crate::widgets::frame::Frame;
use crate::widgets::panel::Panel;
use criterion::{BenchmarkId, Criterion};
use std::hint::black_box;

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
                    .size((Sizing::FILL, Sizing::fixed(20.0)))
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
                                .size((Sizing::fixed(30.0), Sizing::FILL))
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
                    .size((Sizing::FILL, Sizing::fixed(20.0)))
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
                                .size((Sizing::fixed(30.0), Sizing::FILL))
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

/// Drive the ack-the-frame contract during benches. `FrameCycle::record_pass`
/// auto-rewinds damage if the previous `FrameOutput` wasn't marked
/// `Submitted`. `Skip` frames self-ack at `post_record`; `Partial` /
/// `Full` mark `Pending` and need an explicit submit-equivalent.
/// The ack here is unconditional and idempotent.
fn run_and_ack(h: &mut UiHarness, mut record: impl FnMut(&mut Ui)) {
    let _ = h.frame(&mut record);
}

fn damage_kind(h: &UiHarness) -> &'static str {
    match Damage::new(h.collapsed_damage()) {
        Damage::Skip => "skip",
        Damage::Full => "full",
        Damage::Partial(_) => "partial",
    }
}

/// Warm two frames so subsequent iterations land on the steady-state
/// `Damage` path the test claims. Pass the same closure for both
/// frames to warm into a `skip` steady state; pass two different
/// closures (e.g. cold + hot variants of the same scene) so the
/// second frame's diff produces the `partial` / `full` damage the
/// bench iter will then exercise. Without warmup the first iter
/// would always be `Full` (no `prev_surface`) and skew measurements.
fn warm_and_assert(
    h: &mut UiHarness,
    frame1: impl Fn(&mut Ui),
    frame2: impl Fn(&mut Ui),
    expect_kind: &str,
) {
    run_and_ack(h, &frame1);
    run_and_ack(h, &frame2);
    let kind = damage_kind(h);
    assert_eq!(kind, expect_kind, "warmup did not settle on {expect_kind}");
}

/// Run frames until the paint-snapshot arena stops growing, and answer
/// the size it settled at.
///
/// The property this replaced compaction to get: a churn workload takes
/// blocks out of its size classes' free lists rather than extending the
/// arena, so after warm-up the storage is flat and no frame pays for
/// another frame's churn. Warming on that — rather than on a fixed frame
/// count — is also what makes the arms below measure a *settled* arena
/// instead of one still climbing, and the matching post-bench assertion
/// is the regression guard: a change that reintroduced tail-appending
/// would show up as an arena that never stops growing.
fn warm_until_arena_settles<B: FnMut(&mut Ui)>(
    h: &mut UiHarness,
    build: impl Fn(u32) -> B,
    from_frame: u32,
) -> ArenaSettle {
    /// Consecutive flat frames that count as settled. Comfortably past
    /// the 256-canvas rotation in the partial-churn arm, whose period is
    /// the canvas count.
    const FLAT_FRAMES: u32 = 512;
    /// Give up rather than spin: a workload that never settles is a
    /// finding, and the assertion below reports it.
    const MAX_FRAMES: u32 = 4096;

    let mut frame = from_frame;
    let mut settled_at = h.engines.damage.paints.slots.len();
    let mut flat = 0;
    while flat < FLAT_FRAMES && frame - from_frame < MAX_FRAMES {
        run_and_ack(h, build(frame));
        frame += 1;
        let now = h.engines.damage.paints.slots.len();
        flat = if now == settled_at { flat + 1 } else { 0 };
        settled_at = now;
    }
    assert!(
        flat >= FLAT_FRAMES,
        "the paint arena never stopped growing in {MAX_FRAMES} frames (at {settled_at} entries) \
         — block recycling is not reclaiming what the churn frees",
    );
    ArenaSettle {
        entries: settled_at,
        classes: h.engines.damage.paints.classes_with_free_blocks(),
        next_frame: frame,
    }
}

/// Where [`warm_until_arena_settles`] left off.
#[derive(Clone, Copy, Debug)]
struct ArenaSettle {
    /// Arena entries once it went flat — the working set's high-water
    /// mark, which is what the arm reports and re-checks afterwards.
    entries: usize,
    /// Size classes parked with at least one free block. The other half
    /// of the health check: a churn whose row counts stay inside a
    /// handful of classes recycles, and a count that tracks the frame
    /// number is one whose lengths are drifting.
    classes: usize,
    /// The frame number the caller's own loop resumes at.
    next_frame: u32,
}

fn bench_workloads(c: &mut Criterion) {
    let cold = Color::rgb(0.2, 0.4, 0.8);
    let hot = Color::rgb(0.9, 0.4, 0.2);
    let mut group = c.benchmark_group("damage/workload");

    // Skip path — identical scene every frame; nothing dirty. Rows
    // are non-painting Panels so the damage diff walks every painting
    // leaf individually (no subtree-skip available).
    {
        let mut h = UiHarness::new(SURFACE).scale(2.0);
        warm_and_assert(
            &mut h,
            |ui| build_grid(ui, &[], cold),
            |ui| build_grid(ui, &[], cold),
            "skip",
        );
        group.bench_function("skip", |b| {
            b.iter(|| {
                run_and_ack(&mut h, |ui| build_grid(ui, &[], cold));
                black_box(&h);
            });
        });
    }

    // Skip path with painting row Panels — same node count as `skip`,
    // but each row is a painting parent of painting cells. On a stable
    // frame the damage diff's subtree-skip predicate fires at every
    // row, jumping past the 32 per-cell entry lookups underneath.
    // Compare against `skip` to isolate the subtree-skip win.
    {
        let mut h = UiHarness::new(SURFACE).scale(2.0);
        warm_and_assert(
            &mut h,
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
            h.engines.damage.counters.subtree_skips() > 0,
            "no subtree skips at all — fixture is broken",
        );
        group.bench_function("skip_painted_rows", |b| {
            b.iter(|| {
                run_and_ack(&mut h, |ui| build_painted_rows(ui, &[], cold));
                black_box(&h);
            });
        });
    }

    // Partial 1-rect — one cell flips colour each frame.
    {
        let mut h = UiHarness::new(SURFACE).scale(2.0);
        let cell = [42usize];
        warm_and_assert(
            &mut h,
            |ui| build_grid(ui, &cell, cold),
            |ui| build_grid(ui, &cell, hot),
            "partial",
        );
        let mut toggle = false;
        group.bench_function("single_button_change", |b| {
            b.iter(|| {
                toggle = !toggle;
                let color = if toggle { hot } else { cold };
                run_and_ack(&mut h, |ui| build_grid(ui, &cell, color));
                black_box(&h);
            });
        });
    }

    // Partial multi-rect — two distant cells flip together. LVGL
    // merge rule rejects (bbox waste huge), so the region keeps both
    // — drives the multi-pass path.
    {
        let mut h = UiHarness::new(SURFACE).scale(2.0);
        let cells = [0usize, (ROWS - 1) * COLS + (COLS - 1)];
        warm_and_assert(
            &mut h,
            |ui| build_grid(ui, &cells, cold),
            |ui| build_grid(ui, &cells, hot),
            "partial",
        );
        assert!(h.damage_region().iter_rects().count() >= 1);
        let mut toggle = false;
        group.bench_function("two_corner_change", |b| {
            b.iter(|| {
                toggle = !toggle;
                let color = if toggle { hot } else { cold };
                run_and_ack(&mut h, |ui| build_grid(ui, &cells, color));
                black_box(&h);
            });
        });
    }

    // Full path — every cell varies each frame; total damage area
    // exceeds the threshold and escalates to `Full`.
    {
        let mut h = UiHarness::new(SURFACE).scale(2.0);
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
                                .size((Sizing::FILL, Sizing::fixed(20.0)))
                                .show(ui, |ui| {
                                    for c in 0..COLS {
                                        let i = r * COLS + c;
                                        let phase = (i as u32 + frame_n) as f32 * 0.013;
                                        Frame::new()
                                            .id_salt(("cell", r, c))
                                            .size((Sizing::fixed(30.0), Sizing::FILL))
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
        run_and_ack(&mut h, varying(0));
        run_and_ack(&mut h, varying(1));
        assert_eq!(damage_kind(&h), "full");
        let mut frame_n = 2u32;
        group.bench_function("full_repaint", |b| {
            b.iter(|| {
                frame_n = frame_n.wrapping_add(1);
                run_and_ack(&mut h, varying(frame_n));
                black_box(&h);
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
    // many canvases mutate per frame. The compaction counter is
    // asserted non-zero during warmup so a silent
    // degeneration (e.g. all-Skip frames) doesn't pass the bench
    // unnoticed.

    // Logical surface = 640×400 (SURFACE / scale 2.0). A 16×16 grid
    // of 40×25 px canvases fits with margin. Earlier vstack-only
    // layout pushed most canvases off-surface, so the diff's
    // off-surface skip made the bench measure ~10 widgets, not 256.
    let canvas_body = |c: usize, count: u32, ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash(("canvas", c)))
            .size((Sizing::fixed(40.0), Sizing::fixed(25.0)))
            .background(Background {
                fill: Color::rgb(0.1, 0.1, 0.12).into(),
                ..Default::default()
            })
            .show(ui, |ui| {
                for s in 0..count {
                    ui.add_shape(
                        Shape::rect(Rect::new((s as f32) * 4.0, 2.0, 3.0, 20.0))
                            .corners(1.0)
                            .fill(Color::rgb(0.3 + (s as f32) * 0.05, 0.4, 0.6)),
                    );
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
                        .size((Sizing::FILL, Sizing::fixed(25.0)))
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

        let mut h = UiHarness::new(SURFACE).scale(2.0);
        let settled = warm_until_arena_settles(&mut h, build, 0);
        // Sanity: the arena should hold roughly STABLE_COUNT × CANVASES
        // live entries. Catches off-surface regressions where most
        // canvases skip insert and the bench silently measures a much
        // smaller pool.
        assert!(
            settled.entries >= CANVASES * (STABLE_COUNT as usize - 1),
            "partial churn: arena underpopulated (len={}, expected >= {})",
            settled.entries,
            CANVASES * (STABLE_COUNT as usize - 1),
        );
        eprintln!(
            "[shape_churn_partial] warmup: {} frames, arena settled at {} entries \
             across {} recycling size classes",
            settled.next_frame, settled.entries, settled.classes,
        );
        let mut frame_n = settled.next_frame;
        group.bench_function("shape_churn_partial", |b| {
            b.iter(|| {
                frame_n = frame_n.wrapping_add(1);
                run_and_ack(&mut h, build(frame_n));
                black_box(&h);
            });
        });
        // The regression guard: thousands of measured churn frames must
        // not add one entry. A tail-appending arena would climb here,
        // and so would a size class that stopped reclaiming its own
        // blocks.
        assert_eq!(
            h.engines.damage.paints.slots.len(),
            settled.entries,
            "[shape_churn_partial] the arena grew over {} measured frames",
            frame_n - settled.next_frame,
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

        let mut h = UiHarness::new(SURFACE).scale(2.0);
        let settled = warm_until_arena_settles(&mut h, build, 0);
        assert!(
            settled.entries >= CANVASES * BASE_SHAPES as usize,
            "full churn: arena underpopulated (len={}, expected >= {})",
            settled.entries,
            CANVASES * BASE_SHAPES as usize,
        );
        eprintln!(
            "[shape_churn_full] warmup: {} frames, arena settled at {} entries \
             across {} recycling size classes",
            settled.next_frame, settled.entries, settled.classes,
        );
        let mut frame_n = settled.next_frame;
        group.bench_function("shape_churn_full", |b| {
            b.iter(|| {
                frame_n = frame_n.wrapping_add(1);
                run_and_ack(&mut h, build(frame_n));
                black_box(&h);
            });
        });
        // Every canvas changes its row count every frame, so this is the
        // harder half of the guard: four size classes in rotation, and
        // still not one new entry over the measured run.
        assert_eq!(
            h.engines.damage.paints.slots.len(),
            settled.entries,
            "[shape_churn_full] the arena grew over {} measured frames",
            frame_n - settled.next_frame,
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
        let retained = DamageRegion::from_rects(rects).iter_rects().count();
        group.bench_with_input(
            BenchmarkId::new(*label, format!("{}_in_{}_out", rects.len(), retained)),
            rects,
            |b, rects| {
                b.iter(|| black_box(DamageRegion::from_rects(rects).iter_rects().count()));
            },
        );
    }

    group.finish();
}

/// Sibling counts for the paint-order arms. Spaced so the shape of the
/// curve is readable: if the quadratic pair walk dominates, 512 costs
/// ~16x what 128 does; if it is noise beside the rest of the diff, the
/// four numbers stay within a small factor.
const ORDER_FANOUT: [usize; 4] = [64, 128, 256, 512];

/// `count` overlapping sibling frames under one parent, painted in
/// `order`.
///
/// A `ZStack` rather than a list, for two reasons. It is the shape a
/// graph canvas actually has — nodes sit on top of one another, which
/// is the only situation where raising one *means* anything — and it
/// keeps every child's extent overlapping every other's, so each
/// inverted pair the walk finds yields a real intersection instead of
/// being discarded. That is the worst case, which is what a cliff
/// hunt wants. Stacking them instead would also overflow the viewport
/// past a few hundred rows and tip damage to `full`.
///
/// Ids travel with the *content*, not the slot, so reordering `order`
/// leaves every row exact-matched and only their relative positions
/// change — which is precisely what the inversion check looks for. A
/// list keyed by slot would re-key the rows instead and never reach it.
fn build_ordered_siblings(ui: &mut Ui, order: &[usize]) {
    Panel::zstack()
        .id_salt("order-root")
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            for &i in order {
                Frame::new()
                    .id_salt(("sib", i))
                    .size((Sizing::fixed(120.0), Sizing::fixed(60.0)))
                    .background(Background {
                        fill: Color::rgb(0.2, 0.2, 0.25).into(),
                        ..Default::default()
                    })
                    .show(ui);
            }
        });
}

/// Raising one child to the front, which is what clicking a node on a
/// graph canvas does.
///
/// `has_order_inversion` is an O(n) gate, but once it fires
/// `emit_inverted_overlaps` enumerates every `(j1, j2)` pair — and
/// raising a single child inverts only `n` of those, so all but a
/// vanishing fraction of the walk finds nothing. These arms exist to
/// say whether that difference is visible against the rest of the
/// damage diff, and from what fanout.
///
/// Each iteration alternates between the raised and unraised orders so
/// every frame trips the inversion; holding one order steady would
/// settle into `skip` and measure nothing.
fn bench_paint_order_inversion(c: &mut Criterion) {
    let mut group = c.benchmark_group("damage/paint_order");

    for count in ORDER_FANOUT {
        let flat: Vec<usize> = (0..count).collect();
        let mut raised = flat.clone();
        let last = raised.pop().expect("fanout is non-empty");
        raised.insert(0, last);

        let mut h = UiHarness::new(SURFACE);
        warm_and_assert(
            &mut h,
            |ui| build_ordered_siblings(ui, &flat),
            |ui| build_ordered_siblings(ui, &raised),
            "partial",
        );

        let mut flipped = false;
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
            b.iter(|| {
                flipped = !flipped;
                let order = if flipped { &raised } else { &flat };
                run_and_ack(&mut h, |ui| build_ordered_siblings(ui, order));
                black_box(h.collapsed_damage());
            });
        });
    }
    group.finish();
}

pub(crate) fn bench(c: &mut Criterion, _: crate::bench::Run<'_>) {
    bench_workloads(c);
    bench_paint_order_inversion(c);
    bench_region_add(c);
}
