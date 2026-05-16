//! Scroll + zoom interaction bench. Drives the showcase's
//! `pan_zoom::build` (a `Scroll::both().with_zoom()` viewport over a
//! 24×24 button grid) through the full CPU pipeline each iteration
//! with synthetic pointer / scroll-wheel / pinch-zoom events so the
//! widget state mutates every frame. No GPU.
//!
//! Four cases isolate which input drives cost:
//! - `scrollzoom/idle`       — no input, measure-cache hit floor.
//! - `scrollzoom/pan_only`   — oscillating scroll delta, no zoom.
//! - `scrollzoom/zoom_only`  — oscillating pinch, no pan (transform
//!   invalidation path).
//! - `scrollzoom/pan_zoom`   — combined, realistic interactive load.
//!
//! Deltas are cosine-shaped so the integrated offset / zoom level
//! stays bounded and never saturates against scroll/zoom clamps —
//! the bench keeps measuring real per-frame work, not work against a
//! pinned state.
//!
//! `Ui::for_test()` uses the mono text fallback (matches `frame.rs`); the
//! cosmic shaper is not on the critical path here.

use criterion::{Criterion, criterion_group, criterion_main};
use glam::{UVec2, Vec2};
use palantir::{Display, FrameStamp, InputEvent, UiCore};
use std::hint::black_box;
use std::time::Duration;

#[path = "../src/showcase/complex_pan_zoom.rs"]
mod pan_zoom;

const SIZE: UVec2 = UVec2::new(1280, 800);
const SCALE: f32 = 2.0;
// Inside the scroll viewport (header text + 8px gap above; viewport
// fills the rest of a 800-logical-px / SCALE=2 → 400-logical-px tall
// surface). Picking the middle is robust to small layout drift.
const VIEWPORT_CENTER: Vec2 = Vec2::new(320.0, 250.0);

fn warmed_ui() -> (UiCore, Display) {
    let mut ui = UiCore::for_test();
    let display = Display::from_physical(SIZE, SCALE);
    // Two frames so cascades populate and `post_record` latches the
    // Scroll widget as the scroll-target hit. Pointer must be inside
    // the viewport before frame 2's post_record runs.
    ui.frame(
        FrameStamp::new(display, Duration::ZERO),
        &mut (),
        pan_zoom::build,
    );
    ui.mark_frame_submitted();
    ui.on_input(InputEvent::PointerMoved(VIEWPORT_CENTER));
    ui.frame(
        FrameStamp::new(display, Duration::ZERO),
        &mut (),
        pan_zoom::build,
    );
    ui.mark_frame_submitted();
    (ui, display)
}

fn run_frame(ui: &mut UiCore, display: Display) {
    ui.frame(
        FrameStamp::new(display, Duration::ZERO),
        &mut (),
        pan_zoom::build,
    );
    ui.mark_frame_submitted();
}

fn bench_scrollzoom(c: &mut Criterion) {
    {
        let (mut ui, display) = warmed_ui();
        c.bench_function("scrollzoom/idle", |b| {
            b.iter(|| {
                run_frame(&mut ui, display);
                black_box(&ui);
            });
        });
    }

    {
        let (mut ui, display) = warmed_ui();
        let mut i: u32 = 0;
        c.bench_function("scrollzoom/pan_only", |b| {
            b.iter(|| {
                let t = i as f32 * 0.05;
                ui.on_input(InputEvent::ScrollPixels(Vec2::new(
                    t.cos() * 5.0,
                    (t * 0.7).cos() * 5.0,
                )));
                run_frame(&mut ui, display);
                i = i.wrapping_add(1);
                black_box(&ui);
            });
        });
    }

    {
        let (mut ui, display) = warmed_ui();
        let mut i: u32 = 0;
        c.bench_function("scrollzoom/zoom_only", |b| {
            b.iter(|| {
                let t = i as f32 * 0.05;
                ui.on_input(InputEvent::Zoom(1.0 + t.cos() * 0.02));
                run_frame(&mut ui, display);
                i = i.wrapping_add(1);
                black_box(&ui);
            });
        });
    }

    {
        let (mut ui, display) = warmed_ui();
        let mut i: u32 = 0;
        c.bench_function("scrollzoom/pan_zoom", |b| {
            b.iter(|| {
                let t = i as f32 * 0.05;
                ui.on_input(InputEvent::ScrollPixels(Vec2::new(
                    t.cos() * 5.0,
                    (t * 0.7).cos() * 5.0,
                )));
                ui.on_input(InputEvent::Zoom(1.0 + t.cos() * 0.02));
                run_frame(&mut ui, display);
                i = i.wrapping_add(1);
                black_box(&ui);
            });
        });
    }
}

criterion_group!(benches, bench_scrollzoom);
criterion_main!(benches);
