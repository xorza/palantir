//! Pure-input dispatch throughput. Builds a complex UI with hundreds
//! of overlapping clickable / focusable regions across multiple
//! ZStacks + a popup layer, warms it through two frames so cascades
//! and hit-index are populated, then streams `on_input` events in the
//! inner loop **without** running a frame each iteration.
//!
//! What this measures: `Ui::on_input` cost — pointer hover recompute
//! (`recompute_hover` + `recompute_scroll_target` linear walk over
//! cascade entries), press/release hit-tests, scroll target lookup.
//! `Cascades::hit_test` is a reverse linear scan, so overlap density
//! is the dominant cost driver — the inner ZStacks intentionally
//! pile O(N) clickable rects on each pointer position.
//!
//! Cases:
//! - `input/pointer_move_stream` — oscillating cursor across the
//!   layout, the realistic per-frame burst (many `CursorMoved` events
//!   coalesced from winit before the next redraw).
//! - `input/click_stream` — press/release pairs, hits the focus +
//!   click hit-test paths.
//! - `input/scroll_stream` — `ScrollPixels` against a scroll target,
//!   accumulator-only path.
//! - `input/mixed_stream` — interleaved moves / clicks / scrolls.

use criterion::{Criterion, criterion_group, criterion_main};
use glam::{UVec2, Vec2};
use palantir::{
    Button, Configure, Display, Frame, FrameStamp, InputEvent, Panel, PointerButton, Scroll, Sense,
    Sizing, Text, TextShaper, Ui, new_handle,
};
use std::hint::black_box;
use std::time::Duration;

/// Local mono-fallback `Ui` constructor; `support::testing::new_ui`
/// is gated behind `cfg(test)` and not visible from bench targets.
fn new_ui() -> Ui {
    Ui::new(TextShaper::default(), new_handle())
}

const SIZE: UVec2 = UVec2::new(1280, 800);
const SCALE: f32 = 2.0;
const OVERLAP_LAYERS: usize = 64;
const GRID_COLS: usize = 12;
const GRID_ROWS: usize = 8;

fn build_ui(ui: &mut Ui) {
    Panel::zstack()
        .auto_id()
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            // Bottom layer: dense clickable grid filling the viewport.
            // Every cell is a Button (Sense::CLICK + focusable), so the
            // cascade has G*G entries before any overlap stack starts.
            Panel::vstack()
                .auto_id()
                .gap(0.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    for r in 0..GRID_ROWS {
                        Panel::hstack()
                            .id_salt(("grid-row", r))
                            .gap(0.0)
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |ui| {
                                for c in 0..GRID_COLS {
                                    Button::new()
                                        .id_salt(("cell", r, c))
                                        .label("·")
                                        .size((Sizing::FILL, Sizing::FILL))
                                        .show(ui);
                                }
                            });
                    }
                });

            // Overlap stack: OVERLAP_LAYERS sensing full-rect frames
            // piled on top of each other inside a ZStack. Every pointer
            // position lies inside all of them — worst case for the
            // topmost-first reverse hit scan. Sense rotates HOVER /
            // CLICK / DRAG / SCROLL so all three hit-test filters
            // (hovers/clicks/scrolls) walk a populated path.
            Panel::zstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    for i in 0..OVERLAP_LAYERS {
                        let sense = match i % 4 {
                            0 => Sense::HOVER,
                            1 => Sense::CLICK,
                            2 => Sense::CLICK | Sense::DRAG,
                            _ => Sense::SCROLL,
                        };
                        Frame::new()
                            .id_salt(("ovl", i))
                            .sense(sense)
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui);
                    }
                });

            // Scrollable region in the middle covering the viewport
            // center so `recompute_scroll_target` succeeds — exercises
            // the scroll-target update branch on pointer move.
            Scroll::both()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Panel::vstack()
                        .auto_id()
                        .gap(0.0)
                        .size((Sizing::Fixed(4000.0), Sizing::Fixed(4000.0)))
                        .show(ui, |ui| {
                            for i in 0..64 {
                                Text::new("scroll content")
                                    .id_salt(("scrolltxt", i))
                                    .show(ui);
                            }
                            Frame::new()
                                .auto_id()
                                .size((Sizing::Fixed(4000.0), Sizing::Fixed(4000.0)))
                                .show(ui);
                        });
                });
        });
}

fn warmed_ui() -> (Ui, Display) {
    let mut ui = new_ui();
    let display = Display::from_physical(SIZE, SCALE);
    // Two frames: first builds cascades, second latches scroll-target
    // and any post_record state once the pointer is inside.
    ui.frame(FrameStamp::new(display, Duration::ZERO), &mut (), build_ui);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(320.0, 200.0)));
    ui.frame(FrameStamp::new(display, Duration::ZERO), &mut (), build_ui);
    (ui, display)
}

/// Pointer position that walks a Lissajous path across the logical
/// surface (640×400). Different positions hit different bottom-layer
/// grid cells, so hover transitions actually fire.
fn pointer_at(i: u32) -> Vec2 {
    let t = i as f32 * 0.037;
    let x = 320.0 + (t.cos() * 280.0);
    let y = 200.0 + ((t * 1.31).sin() * 160.0);
    Vec2::new(x, y)
}

fn bench_input(c: &mut Criterion) {
    {
        let (mut ui, _display) = warmed_ui();
        let mut i: u32 = 0;
        c.bench_function("input/pointer_move_stream", |b| {
            b.iter(|| {
                let delta = ui.on_input(InputEvent::PointerMoved(pointer_at(i)));
                i = i.wrapping_add(1);
                black_box(delta);
            });
        });
    }

    {
        let (mut ui, _display) = warmed_ui();
        let mut i: u32 = 0;
        c.bench_function("input/click_stream", |b| {
            b.iter(|| {
                // Move first so the press hits a fresh cell — without
                // this every press lands on the same active widget and
                // the focus hit-test gets memoized into the warm path.
                ui.on_input(InputEvent::PointerMoved(pointer_at(i)));
                ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
                let d = ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
                i = i.wrapping_add(1);
                black_box(d);
            });
        });
    }

    {
        let (mut ui, _display) = warmed_ui();
        let mut i: u32 = 0;
        c.bench_function("input/scroll_stream", |b| {
            b.iter(|| {
                let t = i as f32 * 0.05;
                let d = ui.on_input(InputEvent::ScrollPixels(Vec2::new(
                    t.cos() * 5.0,
                    (t * 0.7).cos() * 5.0,
                )));
                i = i.wrapping_add(1);
                black_box(d);
            });
        });
    }

    {
        let (mut ui, _display) = warmed_ui();
        let mut i: u32 = 0;
        c.bench_function("input/mixed_stream", |b| {
            b.iter(|| {
                // ~realistic burst between two redraws: several moves,
                // a scroll, occasional click.
                ui.on_input(InputEvent::PointerMoved(pointer_at(i)));
                ui.on_input(InputEvent::PointerMoved(pointer_at(i.wrapping_add(1))));
                ui.on_input(InputEvent::PointerMoved(pointer_at(i.wrapping_add(2))));
                ui.on_input(InputEvent::ScrollPixels(Vec2::new(0.0, 3.0)));
                if i.is_multiple_of(16) {
                    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
                    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
                }
                i = i.wrapping_add(3);
                black_box(&ui);
            });
        });
    }
}

criterion_group!(benches, bench_input);
criterion_main!(benches);
