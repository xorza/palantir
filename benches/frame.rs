//! Per-frame aggregate benchmark. Builds a synthetic but realistic UI tree
//! exercising every layout driver (HStack/VStack/ZStack/Canvas/Grid), mixed
//! `Sizing::{Fixed, Hug, Fill}` tracks, wrapping text inside both Hug-grid
//! columns and Fill stack children, grid spans, alignment, justify,
//! padding/margin, and Canvas position. Drives all five passes
//! (record/measure/arrange/cascade/encode+compose) every iteration.
//!
//! `Ui::default()` leaves the cosmic shaper unset, so text measurement runs
//! through the mono fallback.

use criterion::{Criterion, criterion_group, criterion_main};
use palantir::{Align, Button, Configure, Frame, Grid, Justify, Panel, Sizing, Text, Track, Ui};
use std::hint::black_box;
use std::rc::Rc;

const SCALE: usize = 32;

fn build_ui(ui: &mut Ui) {
    let sidebar_items = 5 * SCALE;
    let chat_messages = 2 * SCALE;
    let canvas_dots = 3 * SCALE;
    let prop_rows = 4 + SCALE;

    Panel::vstack().auto_id()
        .gap(8.0)
        .padding(12.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            // Header bar — HStack with title, spacer (Fill), action buttons.
            // Exercises stack Fill leftover + Hug siblings + child_align.
            Panel::hstack().auto_id()
                .gap(8.0)
                .size((Sizing::FILL, Sizing::Hug))
                .child_align(Align::CENTER)
                .show(ui, |ui| {
                    Text::new("Complex Layout Showcase").id_salt("title")
                        .style(palantir::TextStyle::default().with_font_size(20.0))
                        .show(ui);
                    Frame::new().id_salt("title-spacer")
                        .size((Sizing::FILL, Sizing::Fixed(1.0)))
                        .show(ui);
                    for i in 0..5 {
                        Button::new().id_salt(("hdr", i))
                            .label(format!("Action {i}"))
                            .show(ui);
                    }
                });

            // Body — HStack with Fixed sidebar + Fill main column.
            Panel::hstack().auto_id()
                .gap(12.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    // Sidebar: VStack of fill-width buttons + a justify Center
                    // sub-stack to exercise that branch too.
                    Panel::vstack().auto_id()
                        .gap(4.0)
                        .padding(8.0)
                        .size((Sizing::Fixed(220.0), Sizing::FILL))
                        .show(ui, |ui| {
                            for i in 0..sidebar_items {
                                Button::new().id_salt(("side", i))
                                    .label(format!("Sidebar item {i}"))
                                    .size((Sizing::FILL, Sizing::Hug))
                                    .show(ui);
                            }
                            Frame::new().id_salt("sb-divider")
                                .size((Sizing::FILL, Sizing::Fixed(1.0)))
                                .margin(4.0)
                                .show(ui);
                            Panel::hstack().auto_id()
                                .gap(2.0)
                                .justify(Justify::Center)
                                .size((Sizing::FILL, Sizing::Hug))
                                .show(ui, |ui| {
                                    for i in 0..3 {
                                        Button::new().id_salt(("sb-foot", i))
                                            .label(format!("F{i}"))
                                            .show(ui);
                                    }
                                });
                        });

                    // Main column: VStack of property grid + chat list +
                    // canvas overlay + footer ZStack.
                    Panel::vstack().auto_id()
                        .gap(10.0)
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui, |ui| {
                            // Property grid — Hug label col + Fill value col
                            // with wrapping text. The motivating Step B
                            // pattern from intrinsic.md.
                            let rows: Vec<Track> = (0..prop_rows).map(|_| Track::hug()).collect();
                            Grid::new().id_salt("props")
                                .cols(Rc::from([
                                    Track::hug().min(80.0),
                                    Track::fill(),
                                    Track::fixed(60.0),
                                ]))
                                .rows(Rc::<[Track]>::from(rows))
                                .gap(6.0)
                                .padding(4.0)
                                .size((Sizing::FILL, Sizing::Hug))
                                .show(ui, |ui| {
                                    let labels = [
                                        "Name",
                                        "Description",
                                        "Author",
                                        "License",
                                        "Created",
                                        "Modified",
                                        "Tags",
                                        "Notes",
                                    ];
                                    let values = [
                                        "the quick brown fox jumps over the lazy dog",
                                        "Lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor",
                                        "Jane Doe and a long author name to force wrapping in narrow viewports",
                                        "MIT-or-Apache-2.0",
                                    ];
                                    for row in 0..prop_rows {
                                        let r = row as u16;
                                        Text::new(labels[row % labels.len()]).id_salt(("plbl", row))
                                            .style(palantir::TextStyle::default().with_font_size(14.0))
                                            .grid_cell((r, 0))
                                            .show(ui);
                                        Text::new(values[row % values.len()]).id_salt(("pval", row))
                                            .style(palantir::TextStyle::default().with_font_size(14.0))
                                            .wrapping()
                                            .grid_cell((r, 1))
                                            .show(ui);
                                        Button::new().id_salt(("pact", row))
                                            .label("Edit")
                                            .grid_cell((r, 2))
                                            .show(ui);
                                    }
                                });

                            // Chat-style messages — HStack with Fixed avatar
                            // + Fill message that wraps. Step C pattern.
                            // Panels in loops need explicit ids: `track_caller`
                            // on the constructor doesn't propagate through
                            // the closure body, so every iter would resolve
                            // to the same source location and collide.
                            for i in 0..chat_messages {
                                Panel::hstack().id_salt(("chat-row", i))
                                    .gap(8.0)
                                    .size((Sizing::FILL, Sizing::Hug))
                                    .show(ui, |ui| {
                                        Frame::new().id_salt(("avatar", i))
                                            .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
                                            .show(ui);
                                        Panel::vstack().id_salt(("chat-text", i))
                                            .gap(2.0)
                                            .size((Sizing::FILL, Sizing::Hug))
                                            .show(ui, |ui| {
                                                Text::new(format!("user_{i}")).id_salt(("from", i))
                                                    .style(palantir::TextStyle::default().with_font_size(12.0))
                                                    .show(ui);
                                                Text::new("This is a longer message body that should wrap inside the Fill stack column without breaking words inside any single token.",).id_salt(("msg", i))
                                                .style(palantir::TextStyle::default().with_font_size(13.0))
                                                .wrapping()
                                                .size((Sizing::FILL, Sizing::Hug))
                                                .show(ui);
                                            });
                                    });
                            }

                            // Canvas with absolutely-positioned dots — exercises
                            // the canvas measure/arrange path.
                            Panel::canvas().auto_id()
                                .size((Sizing::FILL, Sizing::Fixed(80.0)))
                                .show(ui, |ui| {
                                    for i in 0..canvas_dots {
                                        Frame::new().id_salt(("dot", i))
                                            .size((Sizing::Fixed(16.0), Sizing::Fixed(16.0)))
                                            .position((i as f32 * 22.0, 12.0 + (i % 3) as f32 * 18.0))
                                            .show(ui);
                                    }
                                });
                        });
                });

            // Footer: ZStack overlay — bg frame + centered status text.
            Panel::zstack().auto_id()
                .size((Sizing::FILL, Sizing::Fixed(36.0)))
                .show(ui, |ui| {
                    Frame::new().id_salt("footer-bg")
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui);
                    Panel::hstack().auto_id()
                        .padding(6.0)
                        .gap(6.0)
                        .child_align(Align::CENTER)
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui, |ui| {
                            Text::new("Ready").id_salt("footer-status")
                                .style(palantir::TextStyle::default().with_font_size(12.0))
                                .show(ui);
                            Frame::new().id_salt("footer-spacer")
                                .size((Sizing::FILL, Sizing::Fixed(1.0)))
                                .show(ui);
                            Text::new("v1.2.3 · 42 nodes").id_salt("footer-meta")
                                .style(palantir::TextStyle::default().with_font_size(12.0))
                                .show(ui);
                        });
                });
        });
}

fn bench_frame(c: &mut Criterion) {
    use palantir::Display;
    let display = Display::from_physical(glam::UVec2::new(1280, 800), 2.0);
    let mut ui = Ui::default();

    c.bench_function("frame/post_record", |b| {
        b.iter(|| {
            black_box(ui.frame(display, std::time::Duration::ZERO, &mut (), build_ui));
            // #[cfg(feature = "internals")]
            // palantir::support::internals::mark_frame_submitted(&ui);
        });
    });

    // Same workload, but the window resizes every iteration so the
    // measure/encode caches see a fresh `available` quantization each frame.
    // Approximates a live drag-resize.
    let mut ui = Ui::default();
    let mut frame = 0u32;
    c.bench_function("frame/post_record_resizing", |b| {
        b.iter(|| {
            let w = 1024 + (frame % 512);
            let h = 640 + ((frame / 7) % 320);
            frame = frame.wrapping_add(1);
            let display = Display::from_physical(glam::UVec2::new(w, h), 2.0);
            black_box(ui.frame(display, std::time::Duration::ZERO, &mut (), build_ui));
            // #[cfg(feature = "internals")]
            // palantir::support::internals::mark_frame_submitted(&ui);
        });
    });
}

criterion_group!(benches, bench_frame);
criterion_main!(benches);
