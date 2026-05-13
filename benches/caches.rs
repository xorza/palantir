//! Cache-effectiveness A/B benchmark. One nested workload, three axes
//! (measure / encode / compose), each axis benched in two arms:
//!
//! - `cached`: warm-up frame primes every cache; subsequent iterations hit
//!   the cache at the highest stable subtree root every frame (in steady
//!   state, the root itself).
//! - `forced_miss`: warm-up primes the *other two* caches; each iteration
//!   then clears only the cache for this axis before `post_record`, so the
//!   axis under test rebuilds from scratch while the other two stay pure
//!   cache hits.
//!
//! Ratio of `cached / forced_miss` quantifies the cache's contribution
//! on a comparable workload. See `src/layout/measure-cache.md`. The
//! encode and compose caches were removed after their contributions
//! turned out to be < 1% — see `docs/encode-cache-investigation.md`
//! and `docs/compose-cache-under-scroll.md`.
//!
//! Gated behind the `bench-deep` feature so default `cargo bench` runs
//! only the steady-state aggregate in `frame.rs`. Run with
//! `cargo bench --features "internals bench-deep"` to exercise these.
//!
//! `Ui::new()` leaves the cosmic shaper unset, so text measurement runs
//! through the mono fallback — same shaper-free path as `benches/frame.rs`.

use criterion::{Criterion, criterion_group, criterion_main};
use glam::Vec2;
use palantir::support::internals;
use palantir::{
    Background, Color, Configure, Corners, Display, Frame, InputEvent, Panel, Rect, Scroll, Shadow,
    Shape, Sizing, Stroke, Text, TextShaper, TextStyle, Ui,
};
use std::hint::black_box;

const GROUPS: usize = 100;
const ROWS_PER_GROUP: usize = 10;

const HEAVY_GROUPS: usize = 50;
const HEAVY_ROWS_PER_GROUP: usize = 8;

const DENSE_GROUPS: usize = 80;
const DENSE_ROWS_PER_GROUP: usize = 12;
/// Decorative shapes pushed onto each row's stack via `add_shape`.
/// Inflates cmd count per node so encode cache hit/miss is dominated
/// by cmd-stream copy size, not just per-node walk overhead.
const DENSE_SHAPES_PER_ROW: usize = 6;

fn build(ui: &mut Ui) {
    Panel::vstack()
        .id_salt("nested-root")
        .gap(4.0)
        .padding(8.0)
        .size((Sizing::FILL, Sizing::Hug))
        .show(ui, |ui| {
            for g in 0..GROUPS {
                Panel::vstack()
                    .id_salt(("group", g))
                    .gap(2.0)
                    .padding(4.0)
                    .size((Sizing::FILL, Sizing::Hug))
                    .show(ui, |ui| {
                        Text::new("Group header")
                            .id_salt(("g-hdr", g))
                            .style(palantir::TextStyle::default().with_font_size(14.0))
                            .show(ui);
                        for r in 0..ROWS_PER_GROUP {
                            Panel::hstack()
                                .id_salt(("row", g, r))
                                .gap(6.0)
                                .size((Sizing::FILL, Sizing::Hug))
                                .show(ui, |ui| {
                                    Frame::new()
                                        .id_salt(("avatar", g, r))
                                        .size((Sizing::Fixed(20.0), Sizing::Fixed(20.0)))
                                        .show(ui);
                                    Text::new("row name")
                                        .id_salt(("name", g, r))
                                        .style(palantir::TextStyle::default().with_font_size(12.0))
                                        .show(ui);
                                    Text::new("meta info")
                                        .id_salt(("meta", g, r))
                                        .style(palantir::TextStyle::default().with_font_size(11.0))
                                        .show(ui);
                                });
                        }
                        Frame::new()
                            .id_salt(("g-ftr", g))
                            .size((Sizing::FILL, Sizing::Fixed(2.0)))
                            .show(ui);
                    });
            }
        });
}

/// Heavier workload variant exercising the composer's slow paths:
/// rounded-stencil clips on every group + every row (lights up the
/// stencil pipeline), real cosmic-text shaping (no mono fallback), an
/// extra zstack layer per row for deeper nesting, and a stroke on each
/// group surface (DrawRectStroked instead of DrawRect). Used to verify
/// the compose-cache contribution finding from the simpler `build`
/// workload — if the cache earns < 1% here too, deletion is justified.
fn build_heavy(ui: &mut Ui) {
    let group_bg = Background {
        fill: Color::hex(0x1a1a1a).into(),
        stroke: Stroke::solid(Color::hex(0x4d5663), 1.5),
        radius: Corners::all(12.0),
        shadow: Shadow::NONE,
    };
    let row_bg = Background {
        fill: Color::hex(0x252525).into(),
        stroke: Stroke::ZERO,
        radius: Corners::all(6.0),
        shadow: Shadow::NONE,
    };
    let avatar_bg = Background {
        fill: Color::hex(0x3a4a5c).into(),
        stroke: Stroke::ZERO,
        radius: Corners::all(10.0),
        shadow: Shadow::NONE,
    };
    Panel::vstack()
        .id_salt("heavy-root")
        .gap(6.0)
        .padding(12.0)
        .size((Sizing::FILL, Sizing::Hug))
        .show(ui, |ui| {
            for g in 0..HEAVY_GROUPS {
                Panel::vstack()
                    .id_salt(("h-group", g))
                    .gap(4.0)
                    .padding(8.0)
                    .size((Sizing::FILL, Sizing::Hug))
                    .background(group_bg)
                    .clip_rounded()
                    .show(ui, |ui| {
                        Text::new("Group header — interesting copy that wraps")
                            .id_salt(("h-g-hdr", g))
                            .style(TextStyle::default().with_font_size(15.0))
                            .show(ui);
                        for r in 0..HEAVY_ROWS_PER_GROUP {
                            Panel::hstack()
                                .id_salt(("h-row", g, r))
                                .gap(8.0)
                                .padding(6.0)
                                .size((Sizing::FILL, Sizing::Hug))
                                .background(row_bg)
                                .clip_rounded()
                                .show(ui, |ui| {
                                    // Inner zstack adds a nesting level — exercises
                                    // measure on a deeper tree.
                                    Panel::zstack()
                                        .id_salt(("h-avatar-wrap", g, r))
                                        .size((Sizing::Fixed(24.0), Sizing::Fixed(24.0)))
                                        .show(ui, |ui| {
                                            Frame::new()
                                                .id_salt(("h-avatar", g, r))
                                                .size((Sizing::FILL, Sizing::FILL))
                                                .background(avatar_bg)
                                                .show(ui);
                                        });
                                    Text::new("row name with longer text content")
                                        .id_salt(("h-name", g, r))
                                        .style(TextStyle::default().with_font_size(13.0))
                                        .show(ui);
                                    Text::new("meta info — secondary detail")
                                        .id_salt(("h-meta", g, r))
                                        .style(TextStyle::default().with_font_size(11.0))
                                        .show(ui);
                                });
                        }
                    });
            }
        });
}

/// Encode-stressing workload: dense per-node shape decoration. Each
/// row gets `DENSE_SHAPES_PER_ROW` decorative `RoundedRect` shapes
/// pushed via `add_shape`, in addition to the row's avatar + two
/// labels. Goal: inflate cmd count per leaf so the encode cache's
/// memcpy-vs-walk asymmetry shows up if there is one. Keeps
/// `Sizing::Fixed` everywhere so measure stays cheap and the encode
/// signal isn't drowned by text shaping.
fn build_dense(ui: &mut Ui) {
    let avatar_bg = Background {
        fill: Color::hex(0x3a4a5c).into(),
        stroke: Stroke::ZERO,
        radius: Corners::all(8.0),
        shadow: Shadow::NONE,
    };
    Panel::vstack()
        .id_salt("dense-root")
        .gap(2.0)
        .padding(4.0)
        .size((Sizing::FILL, Sizing::Hug))
        .show(ui, |ui| {
            for g in 0..DENSE_GROUPS {
                Panel::vstack()
                    .id_salt(("d-group", g))
                    .gap(1.0)
                    .size((Sizing::FILL, Sizing::Hug))
                    .show(ui, |ui| {
                        for r in 0..DENSE_ROWS_PER_GROUP {
                            Panel::hstack()
                                .id_salt(("d-row", g, r))
                                .gap(4.0)
                                .padding(2.0)
                                .size((Sizing::FILL, Sizing::Fixed(20.0)))
                                .show(ui, |ui| {
                                    // Decorative shapes attached directly
                                    // to the panel — emitted as DrawRect
                                    // by the encoder, no descendant
                                    // structure to amortize over.
                                    for s in 0..DENSE_SHAPES_PER_ROW {
                                        let x = (s as f32) * 4.0;
                                        ui.add_shape(Shape::RoundedRect {
                                            local_rect: Some(Rect::new(x, 2.0, 3.0, 16.0)),
                                            radius: Corners::all(1.5),
                                            fill: Color::hex(0x556677).into(),
                                            stroke: Stroke::ZERO,
                                        });
                                    }
                                    Frame::new()
                                        .id_salt(("d-avatar", g, r))
                                        .size((Sizing::Fixed(16.0), Sizing::Fixed(16.0)))
                                        .background(avatar_bg)
                                        .show(ui);
                                    Text::new("name")
                                        .id_salt(("d-name", g, r))
                                        .style(TextStyle::default().with_font_size(11.0))
                                        .show(ui);
                                    Text::new("meta")
                                        .id_salt(("d-meta", g, r))
                                        .style(TextStyle::default().with_font_size(10.0))
                                        .show(ui);
                                });
                        }
                    });
            }
        });
}

fn bench(c: &mut Criterion) {
    let display = Display::from_physical(glam::UVec2::new(1280, 800), 2.0);
    let mut group = c.benchmark_group("caches");

    group.bench_function("measure/cached", |b| {
        let mut ui = Ui::new();
        let _ = ui.frame(display, std::time::Duration::ZERO, build);
        b.iter(|| {
            black_box(ui.frame(display, std::time::Duration::ZERO, build));
        });
    });

    group.bench_function("measure/forced_miss", |b| {
        let mut ui = Ui::new();
        let _ = ui.frame(display, std::time::Duration::ZERO, build);
        b.iter(|| {
            internals::clear_measure_cache(&mut ui);
            black_box(ui.frame(display, std::time::Duration::ZERO, build));
        });
    });

    // Whole-pipeline cost under scroll. Wrapping the workload in a
    // `Scroll` adds an ancestor `current_transform` that mutates per
    // frame whenever the pan offset changes. Two arms:
    //
    // - `scroll/idle`: scroll widget wraps content, offset stays at 0,
    //   no `PushTransform` emitted. Steady-state floor with a Scroll
    //   in place.
    // - `scroll/active`: alternating ±1 px wheel deltas per iteration.
    //   Offset oscillates, transform fires, descendant screen rects
    //   shift. Originally added to measure compose-cache cost when
    //   `cascade_fp` busts; kept post-deletion as a sanity check that
    //   scroll-driven transform changes don't tax the rest of the
    //   pipeline.
    group.bench_function("scroll/idle", |b| {
        let mut ui = Ui::new();
        let _ = ui.frame(display, std::time::Duration::ZERO, build_scrolling);
        b.iter(|| {
            black_box(ui.frame(display, std::time::Duration::ZERO, build_scrolling));
        });
    });

    group.bench_function("scroll/active", |b| {
        let mut ui = Ui::new();
        // Frame 1: register the scroll viewport's rect/content/cascade.
        let _ = ui.frame(display, std::time::Duration::ZERO, build_scrolling);
        // Hover the pointer over the viewport so wheel events route to
        // the scroll target. `recompute_scroll_target` reads cascades,
        // so this needs the post-frame-1 cascade index.
        ui.on_input(InputEvent::PointerMoved(Vec2::new(640.0, 400.0)));
        // Frame 2: apply pointer-route + warm caches a second time.
        let _ = ui.frame(display, std::time::Duration::ZERO, build_scrolling);
        let mut sign: f32 = 1.0;
        b.iter(|| {
            // Alternating ±1 px keeps the offset bounded near 0 across
            // arbitrary iteration counts; both signs still produce a
            // non-zero `current_transform` whenever the running offset
            // is non-zero, so cascade_fp still busts.
            ui.on_input(InputEvent::ScrollPixels(Vec2::new(0.0, sign)));
            sign = -sign;
            black_box(ui.frame(display, std::time::Duration::ZERO, build_scrolling));
        });
    });

    // Heavy-workload variant: rounded-stencil clips on every group +
    // row, real cosmic-text shaping, deeper nesting, strokes. Heavier
    // baseline for the measure cache.
    group.bench_function("heavy/measure/cached", |b| {
        let mut ui = fresh_heavy_ui();
        let _ = ui.frame(display, std::time::Duration::ZERO, build_heavy);
        b.iter(|| {
            black_box(ui.frame(display, std::time::Duration::ZERO, build_heavy));
        });
    });

    group.bench_function("heavy/measure/forced_miss", |b| {
        let mut ui = fresh_heavy_ui();
        let _ = ui.frame(display, std::time::Duration::ZERO, build_heavy);
        b.iter(|| {
            internals::clear_measure_cache(&mut ui);
            black_box(ui.frame(display, std::time::Duration::ZERO, build_heavy));
        });
    });

    // Dense-workload variant: many decorative shapes per row inflate
    // cmd count, originally added to expose any encode-cache value in
    // a high-cmd-density workload (none found; encode cache later
    // deleted). Kept as another baseline for measure.
    group.bench_function("dense/measure/cached", |b| {
        let mut ui = Ui::new();
        let _ = ui.frame(display, std::time::Duration::ZERO, build_dense);
        b.iter(|| {
            black_box(ui.frame(display, std::time::Duration::ZERO, build_dense));
        });
    });

    group.bench_function("dense/measure/forced_miss", |b| {
        let mut ui = Ui::new();
        let _ = ui.frame(display, std::time::Duration::ZERO, build_dense);
        b.iter(|| {
            internals::clear_measure_cache(&mut ui);
            black_box(ui.frame(display, std::time::Duration::ZERO, build_dense));
        });
    });

    group.finish();
}

fn build_scrolling(ui: &mut Ui) {
    Scroll::vertical().id_salt("scroll-root").show(ui, build);
}

/// New `Ui` with a fresh cosmic shaper installed. Heavy workload uses
/// real cosmic-text shaping (no mono fallback) so text measurement
/// reflects realistic per-glyph cost. Each call constructs a fresh
/// `CosmicMeasure`; calling once per bench arm and reusing across
/// `b.iter` invocations amortizes font-database parsing.
fn fresh_heavy_ui() -> Ui {
    Ui::with_text(TextShaper::with_bundled_fonts())
}

criterion_group!(benches, bench);
criterion_main!(benches);
