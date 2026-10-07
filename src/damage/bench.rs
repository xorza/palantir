//! DamageEngine CPU-side regression bench: drives `Ui::frame` over a ~1056-node grid through the four `Damage` paths, plus microbenches of the three `DamageRegion::add` branches. Measures `FrameCycle::post_record` only, not GPU work; text measurement uses the mono fallback.

#![expect(
    clippy::print_stderr,
    reason = "a bench reports what criterion does not measure to the terminal"
)]

use crate::bench::Run;
use crate::damage::Damage;
use crate::damage::region::DamageRegion;
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::shape::Shape;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::panel::Panel;
use criterion::{BenchmarkId, Criterion};
use std::hint::black_box;

const SURFACE: glam::UVec2 = glam::UVec2::new(1280, 800);
const COLS: usize = 32;
const ROWS: usize = 32;

/// 32x32 grid of small frames in a vstack (a dashboard workload); cells in `hot` get `hot_color`. Id salts keep identity stable so damage diffs against the right `prev`.
fn build_grid(ui: &mut Ui, hot: &[usize], hot_color: RgbaF32) {
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
                                RgbaF32::srgb(0.2, 0.2, 0.25)
                            };
                            Block::new()
                                .id_salt(("cell", r, c))
                                .size((Sizing::fixed(30.0), Sizing::FILL))
                                .background(Background::fill(fill))
                                .show(ui);
                        }
                    });
            }
        });
}

/// `build_grid` with a chrome fill on every row Panel: rows are painting parents, so on a stable frame the subtree-skip fires at each row.
fn build_painted_rows(ui: &mut Ui, hot: &[usize], hot_color: RgbaF32) {
    let row_bg = RgbaF32::srgb(0.1, 0.1, 0.12);
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
                    .background(Background::fill(row_bg))
                    .show(ui, |ui| {
                        for c in 0..COLS {
                            let i = r * COLS + c;
                            let fill = if hot.contains(&i) {
                                hot_color
                            } else {
                                RgbaF32::srgb(0.2, 0.2, 0.25)
                            };
                            Block::new()
                                .id_salt(("cell", r, c))
                                .size((Sizing::fixed(30.0), Sizing::FILL))
                                .background(Background::fill(fill))
                                .show(ui);
                        }
                    });
            }
        });
}

/// Acks the frame: `FrameCycle::record_pass` auto-rewinds damage if the previous `FrameOutput` wasn't `Submitted`; `Partial`/`Full` need this explicit, idempotent ack.
fn run_and_ack(h: &mut UiHarness, mut record: impl FnMut(&mut Ui)) {
    let _ = h.frame(&mut record);
}

fn damage_kind(h: &UiHarness) -> &'static str {
    match Damage::new(h.collapsed_damage()) {
        None => "skip",
        Some(Damage::Full) => "full",
        Some(Damage::Partial(_)) => "partial",
    }
}

/// Warms two frames so iterations land on the intended `Damage` path (the same closure twice for `skip`, two variants for `partial`/`full`); otherwise the first iter is always `Full`.
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

/// Runs frames until the paint-snapshot arena stops growing and returns the size it settled at. A churn workload recycles blocks, so storage is flat after warm-up; warming on that makes the arms measure a settled arena, and the post-bench assertion guards against tail-appending returning.
fn warm_until_arena_settles<B: FnMut(&mut Ui)>(
    h: &mut UiHarness,
    build: impl Fn(u32) -> B,
    from_frame: u32,
) -> ArenaSettle {
    const FLAT_FRAMES: u32 = 512;
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

#[derive(Clone, Copy, Debug)]
struct ArenaSettle {
    entries: usize,
    /// Size classes parked with a free block; a count tracking the frame number means drifting lengths.
    classes: usize,
    next_frame: u32,
}

fn bench_workloads(c: &mut Criterion, run: Run<'_>) {
    let cold = RgbaF32::srgb(0.2, 0.4, 0.8);
    let hot = RgbaF32::srgb(0.9, 0.4, 0.2);
    let mut group = run.subgroup(c, "workload");

    // Skip path: nothing dirty; non-painting rows, so the diff walks every painting leaf.
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

    // Skip path with painting rows: the subtree-skip fires at every row; compare against `skip` to isolate its win.
    {
        let mut h = UiHarness::new(SURFACE).scale(2.0);
        warm_and_assert(
            &mut h,
            |ui| build_painted_rows(ui, &[], cold),
            |ui| build_painted_rows(ui, &[], cold),
            "skip",
        );
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

    // Partial multi-rect: two distant cells flip; the LVGL merge rule rejects (huge bbox waste), driving the multi-pass path.
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
                                        Block::new()
                                            .id_salt(("cell", r, c))
                                            .size((Sizing::fixed(30.0), Sizing::FILL))
                                            .background(Background {
                                                fill: RgbaF32::srgb(
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

    // Shape-count churn benches exercise the per-shape diff's grow/shrink/orphan path: `shape_churn_partial` mutates one canvas per frame (like a graph canvas where ~1 connection changes), `shape_churn_full` all of them. The arena settling is asserted during warmup so all-Skip degeneration doesn't pass.

    // Logical surface 640x400: a 16x16 grid of 40x25 px canvases fits; a vstack would push most off-surface.
    let canvas_body = |c: usize, count: u32, ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash(("canvas", c)))
            .size((Sizing::fixed(40.0), Sizing::fixed(25.0)))
            .background(Background::fill(RgbaF32::srgb(0.1, 0.1, 0.12)))
            .show(ui, |ui| {
                for s in 0..count {
                    ui.add_shape(
                        Shape::rect(Rect::new((s as f32) * 4.0, 2.0, 3.0, 20.0))
                            .corners(1.0)
                            .fill(RgbaF32::srgb(0.3 + (s as f32) * 0.05, 0.4, 0.6)),
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

    // Case A: 256 canvases in a 16x16 grid, one mutates per frame (rotating), flipping between 7 and 8 shapes: the common grow/shrink-by-one pattern.
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
        // The regression guard: thousands of churn frames must not add one entry.
        assert_eq!(
            h.engines.damage.paints.slots.len(),
            settled.entries,
            "[shape_churn_partial] the arena grew over {} measured frames",
            frame_n - settled.next_frame,
        );
    }

    // Case B: full churn, measuring the per-shape diff merge cost (Pass 1); damage likely escalates to `Full`, which is fine.
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
        assert_eq!(
            h.engines.damage.paints.slots.len(),
            settled.entries,
            "[shape_churn_full] the arena grew over {} measured frames",
            frame_n - settled.next_frame,
        );
    }

    group.finish();
}

fn bench_region_add(c: &mut Criterion, run: Run<'_>) {
    let mut group = run.subgroup(c, "region/add");

    // One scenario per `DamageRegion::add` branch: **append** (8 disjoint rects, exactly under the cap), **min_growth** (16 disjoint, min-growth from the 9th; the cliff vs `append` is the cap-overflow cost), **cascade** (8 overlapping rects collapsing to 1).
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

/// Sibling counts for the paint-order arms, spaced to show whether a quadratic pair walk dominates (~16x from 128 to 512).
const ORDER_FANOUT: [usize; 4] = [64, 128, 256, 512];

/// `count` overlapping sibling frames under one parent, painted in `order`.
///
/// A `ZStack`, because stacking is the only case where raising one means anything and every extent overlaps every other (the worst case); a list would overflow the viewport and tip damage to `full`. Ids follow the content, not the slot, so reordering leaves rows exact-matched and only their relative positions change.
fn build_ordered_siblings(ui: &mut Ui, order: &[usize]) {
    Panel::zstack()
        .id_salt("order-root")
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            for &i in order {
                Block::new()
                    .id_salt(("sib", i))
                    .size((Sizing::fixed(120.0), Sizing::fixed(60.0)))
                    .background(Background::fill(RgbaF32::srgb(0.2, 0.2, 0.25)))
                    .show(ui);
            }
        });
}

/// Raises one child to the front, as clicking a node on a graph canvas does.
///
/// `emit_inverted_overlaps` enumerates every `(j1, j2)` pair once `has_order_inversion` fires, while raising one child inverts only `n`; these arms show whether that is visible, and from what fanout. Each iteration alternates orders so every frame trips the inversion.
fn bench_paint_order_inversion(c: &mut Criterion, run: Run<'_>) {
    let mut group = run.subgroup(c, "paint_order");

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

pub(crate) fn bench(c: &mut Criterion, run: Run<'_>) {
    bench_workloads(c, run);
    bench_paint_order_inversion(c, run);
    bench_region_add(c, run);
}
