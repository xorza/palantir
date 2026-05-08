//! Cache-effectiveness A/B benchmark. One nested workload, three axes
//! (measure / encode / compose), each axis benched in two arms:
//!
//! - `cached`: warm-up frame primes every cache; subsequent iterations hit
//!   the cache at the highest stable subtree root every frame (in steady
//!   state, the root itself).
//! - `forced_miss`: warm-up primes the *other two* caches; each iteration
//!   then clears only the cache for this axis before `end_frame`, so the
//!   axis under test rebuilds from scratch while the other two stay pure
//!   cache hits.
//!
//! Ratio of `cached / forced_miss` quantifies the cache's contribution on
//! a comparable workload across all three axes. See
//! `src/layout/measure-cache.md`,
//! `src/renderer/frontend/encoder/encode-cache.md`,
//! `src/renderer/frontend/composer/compose-cache.md`.
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
    Background, Color, Configure, Corners, CosmicMeasure, Display, Frame, InputEvent, Panel,
    Scroll, Sizing, Stroke, Surface, Text, TextStyle, Ui, share,
};
use std::hint::black_box;

const GROUPS: usize = 100;
const ROWS_PER_GROUP: usize = 10;

const HEAVY_GROUPS: usize = 50;
const HEAVY_ROWS_PER_GROUP: usize = 8;

fn build(ui: &mut Ui) {
    Panel::vstack()
        .with_id("nested-root")
        .gap(4.0)
        .padding(8.0)
        .size((Sizing::FILL, Sizing::Hug))
        .show(ui, |ui| {
            for g in 0..GROUPS {
                Panel::vstack()
                    .with_id(("group", g))
                    .gap(2.0)
                    .padding(4.0)
                    .size((Sizing::FILL, Sizing::Hug))
                    .show(ui, |ui| {
                        Text::new("Group header")
                            .with_id(("g-hdr", g))
                            .style(palantir::TextStyle::default().with_font_size(14.0))
                            .show(ui);
                        for r in 0..ROWS_PER_GROUP {
                            Panel::hstack()
                                .with_id(("row", g, r))
                                .gap(6.0)
                                .size((Sizing::FILL, Sizing::Hug))
                                .show(ui, |ui| {
                                    Frame::new()
                                        .with_id(("avatar", g, r))
                                        .size((Sizing::Fixed(20.0), Sizing::Fixed(20.0)))
                                        .show(ui);
                                    Text::new("row name")
                                        .with_id(("name", g, r))
                                        .style(palantir::TextStyle::default().with_font_size(12.0))
                                        .show(ui);
                                    Text::new("meta info")
                                        .with_id(("meta", g, r))
                                        .style(palantir::TextStyle::default().with_font_size(11.0))
                                        .show(ui);
                                });
                        }
                        Frame::new()
                            .with_id(("g-ftr", g))
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
    let group_surface = Surface::clip_rounded_with_bg(Background {
        fill: Color::hex(0x1a1a1a),
        stroke: Some(Stroke {
            width: 1.5,
            color: Color::hex(0x4d5663),
        }),
        radius: Corners::all(12.0),
    });
    let row_surface = Surface::clip_rounded_with_bg(Background {
        fill: Color::hex(0x252525),
        stroke: None,
        radius: Corners::all(6.0),
    });
    let avatar_bg = Background {
        fill: Color::hex(0x3a4a5c),
        stroke: None,
        radius: Corners::all(10.0),
    };
    Panel::vstack()
        .with_id("heavy-root")
        .gap(6.0)
        .padding(12.0)
        .size((Sizing::FILL, Sizing::Hug))
        .show(ui, |ui| {
            for g in 0..HEAVY_GROUPS {
                Panel::vstack()
                    .with_id(("h-group", g))
                    .gap(4.0)
                    .padding(8.0)
                    .size((Sizing::FILL, Sizing::Hug))
                    .background(group_surface)
                    .show(ui, |ui| {
                        Text::new("Group header — interesting copy that wraps")
                            .with_id(("h-g-hdr", g))
                            .style(TextStyle::default().with_font_size(15.0))
                            .show(ui);
                        for r in 0..HEAVY_ROWS_PER_GROUP {
                            Panel::hstack()
                                .with_id(("h-row", g, r))
                                .gap(8.0)
                                .padding(6.0)
                                .size((Sizing::FILL, Sizing::Hug))
                                .background(row_surface)
                                .show(ui, |ui| {
                                    // Inner zstack adds a nesting level — exercises
                                    // the encode subtree-skip threshold and gives
                                    // the composer another EnterSubtree to track.
                                    Panel::zstack()
                                        .with_id(("h-avatar-wrap", g, r))
                                        .size((Sizing::Fixed(24.0), Sizing::Fixed(24.0)))
                                        .show(ui, |ui| {
                                            Frame::new()
                                                .with_id(("h-avatar", g, r))
                                                .size((Sizing::FILL, Sizing::FILL))
                                                .background(avatar_bg)
                                                .show(ui);
                                        });
                                    Text::new("row name with longer text content")
                                        .with_id(("h-name", g, r))
                                        .style(TextStyle::default().with_font_size(13.0))
                                        .show(ui);
                                    Text::new("meta info — secondary detail")
                                        .with_id(("h-meta", g, r))
                                        .style(TextStyle::default().with_font_size(11.0))
                                        .show(ui);
                                });
                        }
                    });
            }
        });
}

#[derive(Copy, Clone)]
enum Axis {
    Measure,
    Encode,
}

impl Axis {
    fn name(self) -> &'static str {
        match self {
            Axis::Measure => "measure",
            Axis::Encode => "encode",
        }
    }

    fn clear(self, ui: &mut Ui) {
        match self {
            Axis::Measure => internals::clear_measure_cache(ui),
            Axis::Encode => internals::clear_encode_cache(ui),
        }
    }
}

fn bench(c: &mut Criterion) {
    let display = Display::from_physical(glam::UVec2::new(1280, 800), 2.0);
    let mut group = c.benchmark_group("caches");

    for axis in [Axis::Measure, Axis::Encode] {
        group.bench_function(format!("{}/cached", axis.name()), |b| {
            let mut ui = Ui::new();
            ui.begin_frame(display);
            build(&mut ui);
            let _ = ui.end_frame();
            b.iter(|| {
                ui.begin_frame(display);
                build(&mut ui);
                black_box(ui.end_frame());
            });
        });

        group.bench_function(format!("{}/forced_miss", axis.name()), |b| {
            let mut ui = Ui::new();
            ui.begin_frame(display);
            build(&mut ui);
            let _ = ui.end_frame();
            b.iter(|| {
                axis.clear(&mut ui);
                ui.begin_frame(display);
                build(&mut ui);
                black_box(ui.end_frame());
            });
        });
    }

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
        ui.begin_frame(display);
        build_scrolling(&mut ui);
        let _ = ui.end_frame();
        b.iter(|| {
            ui.begin_frame(display);
            build_scrolling(&mut ui);
            black_box(ui.end_frame());
        });
    });

    group.bench_function("scroll/active", |b| {
        let mut ui = Ui::new();
        // Frame 1: register the scroll viewport's rect/content/cascade.
        ui.begin_frame(display);
        build_scrolling(&mut ui);
        let _ = ui.end_frame();
        // Hover the pointer over the viewport so wheel events route to
        // the scroll target. `recompute_scroll_target` reads cascades,
        // so this needs the post-frame-1 cascade index.
        ui.on_input(InputEvent::PointerMoved(Vec2::new(640.0, 400.0)));
        // Frame 2: apply pointer-route + warm caches a second time.
        ui.begin_frame(display);
        build_scrolling(&mut ui);
        let _ = ui.end_frame();
        let mut sign: f32 = 1.0;
        b.iter(|| {
            // Alternating ±1 px keeps the offset bounded near 0 across
            // arbitrary iteration counts; both signs still produce a
            // non-zero `current_transform` whenever the running offset
            // is non-zero, so cascade_fp still busts.
            ui.on_input(InputEvent::Scroll(Vec2::new(0.0, sign)));
            sign = -sign;
            ui.begin_frame(display);
            build_scrolling(&mut ui);
            black_box(ui.end_frame());
        });
    });

    // Heavy-workload variants. Same cached-vs-forced-miss split as the
    // light arms, but the workload exercises composer slow paths
    // (rounded-stencil clips on every group + row, real cosmic-text
    // shaping, deeper nesting, strokes). Originally added to verify
    // the compose-cache deletion was justified; kept as a heavier
    // baseline for the remaining caches.
    for axis in [Axis::Measure, Axis::Encode] {
        group.bench_function(format!("heavy/{}/cached", axis.name()), |b| {
            let mut ui = fresh_heavy_ui();
            ui.begin_frame(display);
            build_heavy(&mut ui);
            let _ = ui.end_frame();
            b.iter(|| {
                ui.begin_frame(display);
                build_heavy(&mut ui);
                black_box(ui.end_frame());
            });
        });

        group.bench_function(format!("heavy/{}/forced_miss", axis.name()), |b| {
            let mut ui = fresh_heavy_ui();
            ui.begin_frame(display);
            build_heavy(&mut ui);
            let _ = ui.end_frame();
            b.iter(|| {
                axis.clear(&mut ui);
                ui.begin_frame(display);
                build_heavy(&mut ui);
                black_box(ui.end_frame());
            });
        });
    }

    group.finish();
}

fn build_scrolling(ui: &mut Ui) {
    Scroll::vertical().with_id("scroll-root").show(ui, build);
}

/// New `Ui` with a fresh cosmic shaper installed. Heavy workload uses
/// real cosmic-text shaping (no mono fallback) so text measurement
/// reflects realistic per-glyph cost. Each call constructs a fresh
/// `CosmicMeasure`; calling once per bench arm and reusing across
/// `b.iter` invocations amortizes font-database parsing.
fn fresh_heavy_ui() -> Ui {
    let mut ui = Ui::new();
    ui.set_cosmic(share(CosmicMeasure::with_bundled_fonts()));
    ui
}

criterion_group!(benches, bench);
criterion_main!(benches);
