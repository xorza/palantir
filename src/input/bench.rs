//! Pure-input dispatch throughput: a dense UI with overlapping clickable/focusable regions, warmed through two frames, then `on_input` events streamed without recording a frame. Overlap density dominates, since `Cascade::hit_test` is a reverse linear scan.
//!
//! A press and its release never share a frame (see `InputQueue`), so a case that clicks closes the input frame between them, as a frame's end does; otherwise every later event would queue behind the release.
//!
//! Cases:
//! - `input/pointer_move_stream`: oscillating cursor, the per-frame burst of coalesced `CursorMoved`.
//! - `input/click_stream`: press, input frame, release, input frame.
//! - `input/scroll_stream`: `ScrollPixels` against a scroll target.
//! - `input/mixed_stream` — interleaved moves / clicks / scrolls, one input frame per burst.

use crate::bench::fixture::{Backend, Kind, Scale};
use crate::input::input_event::InputEvent;
use crate::input::sense::Sense;
use crate::internals::ui_harness::UiHarness;
use crate::primitives::layout::sizing::Sizing;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::button::Button;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::Scroll;
use crate::widgets::text::Text;
use criterion::Criterion;
use glam::{UVec2, Vec2};
use std::hint::black_box;

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
            // Bottom layer: a dense grid of Buttons (`Sense::CLICK` + focusable) before any overlap stack.
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

            // Overlap stack: OVERLAP_LAYERS full-rect frames piled in a ZStack, so every pointer position is the worst case for the reverse hit scan; Sense rotates HOVER/CLICK/DRAG/SCROLL so all hit-test filters walk a populated path.
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
                        Block::new()
                            .id_salt(("ovl", i))
                            .sense(sense)
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui);
                    }
                });

            // Scrollable region over the viewport center so `hit_test_targets` finds a scroll target.
            Scroll::both()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Panel::vstack()
                        .auto_id()
                        .gap(0.0)
                        .size((Sizing::fixed(4000.0), Sizing::fixed(4000.0)))
                        .show(ui, |ui| {
                            for i in 0..64 {
                                Text::new("scroll content")
                                    .id_salt(("scrolltxt", i))
                                    .show(ui);
                            }
                            Block::new()
                                .auto_id()
                                .size((Sizing::fixed(4000.0), Sizing::fixed(4000.0)))
                                .show(ui);
                        });
                });
        });
}

fn warmed_ui() -> UiHarness {
    let mut h = UiHarness::new(SIZE).scale(SCALE);
    // Two frames: the first builds the cascade, the second latches the scroll target.
    h.frame(build_ui);
    h.move_to(Vec2::new(320.0, 200.0));
    h.frame(build_ui);
    h
}

/// Pointer position on a Lissajous path over the 640×400 surface, so hover transitions fire.
fn pointer_at(i: u32) -> Vec2 {
    let t = i as f32 * 0.037;
    let x = 320.0 + (t.cos() * 280.0);
    let y = 200.0 + ((t * 1.31).sin() * 160.0);
    Vec2::new(x, y)
}

/// `atomic/input/cpu/<stream>`: one event, or one burst between redraws, dispatched into a
/// warmed dense UI with no frame between.
pub(crate) fn atomic(c: &mut Criterion) {
    let mut group = Kind::Atomic.group(c, "input", Scale::Sample);
    group.bench_function(Backend::Cpu.case("pointer_move_stream"), |b| {
        let mut ui = warmed_ui();
        let mut i: u32 = 0;
        b.iter(|| {
            let delta = ui.on_input(InputEvent::PointerMoved(pointer_at(i)));
            i = i.wrapping_add(1);
            black_box(delta);
        });
    });

    group.bench_function(Backend::Cpu.case("click_stream"), |b| {
        let mut ui = warmed_ui();
        let mut i: u32 = 0;
        b.iter(|| {
            // Each press lands on a different cell, so focus moves every click.
            ui.press_at(pointer_at(i));
            ui.ui.end_input_frame();
            let d = ui.release();
            ui.ui.end_input_frame();
            i = i.wrapping_add(1);
            black_box(d);
        });
    });

    group.bench_function(Backend::Cpu.case("scroll_stream"), |b| {
        let mut ui = warmed_ui();
        let mut i: u32 = 0;
        b.iter(|| {
            let t = i as f32 * 0.05;
            let d = ui.scroll_pixels(Vec2::new(t.cos() * 5.0, (t * 0.7).cos() * 5.0));
            i = i.wrapping_add(1);
            black_box(d);
        });
    });

    group.bench_function(Backend::Cpu.case("mixed_stream"), |b| {
        let mut ui = warmed_ui();
        let mut i: u32 = 0;
        b.iter(|| {
            // A realistic burst between redraws: several moves, a scroll, an occasional click.
            ui.move_to(pointer_at(i));
            ui.move_to(pointer_at(i.wrapping_add(1)));
            ui.scroll_pixels_at(pointer_at(i.wrapping_add(2)), Vec2::new(0.0, 3.0));
            if i.is_multiple_of(16) {
                ui.press();
                ui.ui.end_input_frame();
                ui.release();
            }
            ui.ui.end_input_frame();
            i = i.wrapping_add(3);
            black_box(&ui);
        });
    });
    group.finish();
}
