//! Visual regression suite: drives `Ui` headlessly through wgpu, reads
//! the rendered texture into an `RgbaImage`, and compares against
//! committed golden PNGs in `tests/visual/golden/`. Missing goldens
//! are auto-created on first run; failures dump artifacts under
//! `tests/visual/output/<name>/`. See `docs/visual-testing.md`.

mod diff;
mod golden;
mod harness;

use glam::UVec2;
use image::Rgba;
use palantir::{
    Align, Button, Color, Configure, Frame, Grid, Panel, Sizing, Stroke, Styled, Text, Track,
};

use crate::diff::Tolerance;
use crate::golden::assert_matches_golden;
use crate::harness::Harness;

#[test]
fn readback_returns_clear_color_for_empty_scene() {
    let mut h = Harness::new();
    let size = UVec2::new(16, 16);
    let clear = Color::rgb(0.5, 0.25, 0.75);
    let img = h.render(size, 1.0, clear, |_| {});
    assert_eq!(img.dimensions(), (size.x, size.y));

    // sRGB-encoded clear color, allowing ±1 for rounding through the
    // linear↔sRGB pipeline.
    let expected = Rgba([
        (clear.r.powf(1.0 / 2.2) * 255.0).round() as u8,
        (clear.g.powf(1.0 / 2.2) * 255.0).round() as u8,
        (clear.b.powf(1.0 / 2.2) * 255.0).round() as u8,
        255,
    ]);
    for p in img.pixels() {
        for c in 0..4 {
            assert!(
                p.0[c].abs_diff(expected.0[c]) <= 2,
                "pixel {p:?} far from expected clear {expected:?}",
            );
        }
    }
}

const DARK_BG: Color = Color::rgb(0.08, 0.08, 0.10);

#[test]
fn button_hello_matches_golden() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(256, 96), 1.0, DARK_BG, |ui| {
        Button::new()
            .label("hello")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui);
    });
    assert_matches_golden("button_hello", &img, Tolerance::default());
}

/// Exercises the rounded-rect SDF AA path: solid fill, visible stroke,
/// non-trivial corner radius, padded inside a darker scene.
#[test]
fn frame_filled_with_stroke_matches_golden() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(220, 140), 1.0, DARK_BG, |ui| {
        Panel::vstack().padding(20.0).show(ui, |ui| {
            Frame::with_id("card")
                .size((Sizing::FILL, Sizing::FILL))
                .fill(Color::rgb(0.20, 0.30, 0.55))
                .stroke(Stroke {
                    width: 2.0,
                    color: Color::rgb(0.65, 0.80, 1.00),
                })
                .radius(16.0)
                .show(ui);
        });
    });
    assert_matches_golden("frame_filled_with_stroke", &img, Tolerance::default());
}

/// Exercises Fill weight distribution: three rows of 1/2/1 should split
/// the available height in 25/50/25 ratios.
#[test]
fn vstack_fill_weights_matches_golden() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(160, 200), 1.0, DARK_BG, |ui| {
        Panel::vstack()
            .padding(8.0)
            .gap(4.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Frame::with_id("a")
                    .size((Sizing::FILL, Sizing::Fill(1.0)))
                    .fill(Color::rgb(0.85, 0.30, 0.30))
                    .show(ui);
                Frame::with_id("b")
                    .size((Sizing::FILL, Sizing::Fill(2.0)))
                    .fill(Color::rgb(0.30, 0.85, 0.40))
                    .show(ui);
                Frame::with_id("c")
                    .size((Sizing::FILL, Sizing::Fill(1.0)))
                    .fill(Color::rgb(0.30, 0.50, 0.95))
                    .show(ui);
            });
    });
    assert_matches_golden("vstack_fill_weights", &img, Tolerance::default());
}

/// Grid with mixed track types (fixed / fill / hug), gap, and a spanning
/// header row. Tests the grid layout driver end to end.
#[test]
fn grid_mixed_tracks_matches_golden() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(320, 200), 1.0, DARK_BG, |ui| {
        Grid::with_id("g")
            .cols([Track::fixed(80.0), Track::fill(), Track::fixed(60.0)])
            .rows([Track::fixed(40.0), Track::fill()])
            .gap(6.0)
            .padding(10.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Frame::with_id("header")
                    .grid_cell((0, 0))
                    .grid_span((1, 3))
                    .fill(Color::rgb(0.25, 0.30, 0.45))
                    .radius(4.0)
                    .show(ui);
                Frame::with_id("side")
                    .grid_cell((1, 0))
                    .fill(Color::rgb(0.35, 0.45, 0.30))
                    .radius(4.0)
                    .show(ui);
                Frame::with_id("body")
                    .grid_cell((1, 1))
                    .fill(Color::rgb(0.20, 0.20, 0.28))
                    .radius(4.0)
                    .show(ui);
                Frame::with_id("aside")
                    .grid_cell((1, 2))
                    .fill(Color::rgb(0.50, 0.30, 0.45))
                    .radius(4.0)
                    .show(ui);
            });
    });
    assert_matches_golden("grid_mixed_tracks", &img, Tolerance::default());
}

/// ZStack layering: a tinted background frame with a centered button on
/// top. Tests paint order (background drawn first, foreground on top)
/// and `Align::CENTER` arrangement.
#[test]
fn zstack_centered_button_matches_golden() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(240, 160), 1.0, DARK_BG, |ui| {
        Panel::zstack()
            .padding(12.0)
            .size((Sizing::FILL, Sizing::FILL))
            .fill(Color::rgb(0.16, 0.20, 0.28))
            .stroke(Stroke {
                width: 1.0,
                color: Color::rgb(0.30, 0.36, 0.46),
            })
            .radius(10.0)
            .show(ui, |ui| {
                Button::with_id("btn")
                    .align(Align::CENTER)
                    .label("centered")
                    .show(ui);
            });
    });
    assert_matches_golden("zstack_centered_button", &img, Tolerance::default());
}

/// Multi-line text on a solid background. Slightly looser tolerance —
/// glyph AA varies more across drivers than rect-only scenes.
#[test]
fn text_paragraph_matches_golden() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(360, 140), 1.0, DARK_BG, |ui| {
        Panel::vstack().padding(16.0).gap(6.0).show(ui, |ui| {
            Text::with_id("title", "Palantir")
                .size_px(20.0)
                .color(Color::rgb(0.92, 0.94, 1.00))
                .show(ui);
            Text::with_id("body", "Immediate-mode UI with WPF-style layout.")
                .size_px(13.0)
                .color(Color::rgb(0.72, 0.76, 0.84))
                .show(ui);
            Text::with_id("body2", "Rendered headlessly through wgpu.")
                .size_px(13.0)
                .color(Color::rgb(0.72, 0.76, 0.84))
                .show(ui);
        });
    });
    let tol = Tolerance {
        per_channel: 4,
        max_ratio: 0.005,
    };
    assert_matches_golden("text_paragraph", &img, tol);
}

/// Complex multi-region scene at scale 2.0 (hi-dpi). Exercises:
///   - header / sidebar / content / footer grid layout,
///   - nested vstacks + hstacks with mixed sizing,
///   - text at multiple sizes,
///   - rounded-rect AA + strokes at sub-pixel positions (scale 2.0
///     puts logical pixel edges on physical half-pixels),
///   - the renderer's pixel_snap path under non-1.0 scale.
///
/// Physical 800×600 = logical 400×300 at scale 2.0.
#[test]
fn dashboard_hidpi_matches_golden() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(800, 600), 2.0, DARK_BG, |ui| {
        Grid::with_id("shell")
            .cols([Track::fixed(110.0), Track::fill()])
            .rows([Track::fixed(40.0), Track::fill(), Track::fixed(24.0)])
            .gap(8.0)
            .padding(12.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                // Header: title + action buttons, spans both columns.
                Panel::hstack_with_id("header")
                    .grid_cell((0, 0))
                    .grid_span((1, 2))
                    .padding((10.0, 14.0, 10.0, 14.0))
                    .gap(8.0)
                    .fill(Color::rgb(0.18, 0.22, 0.32))
                    .stroke(Stroke {
                        width: 1.0,
                        color: Color::rgb(0.30, 0.36, 0.46),
                    })
                    .radius(6.0)
                    .show(ui, |ui| {
                        Text::with_id("brand", "Palantir")
                            .size_px(16.0)
                            .color(Color::rgb(0.92, 0.94, 1.00))
                            .show(ui);
                        Frame::with_id("spacer")
                            .size((Sizing::FILL, Sizing::Fixed(1.0)))
                            .show(ui);
                        Button::with_id("btn-save").label("save").show(ui);
                        Button::with_id("btn-export").label("export").show(ui);
                    });

                // Sidebar: vertical stack of tinted nav items.
                Panel::vstack_with_id("sidebar")
                    .grid_cell((1, 0))
                    .padding(8.0)
                    .gap(4.0)
                    .fill(Color::rgb(0.14, 0.17, 0.24))
                    .radius(6.0)
                    .show(ui, |ui| {
                        for (i, label) in ["Home", "Inbox", "Files", "Tags", "Trash"]
                            .iter()
                            .enumerate()
                        {
                            Frame::with_id(("nav-bg", i))
                                .size((Sizing::FILL, Sizing::Fixed(28.0)))
                                .padding((6.0, 8.0, 6.0, 8.0))
                                .fill(if i == 1 {
                                    Color::rgb(0.22, 0.30, 0.46)
                                } else {
                                    Color::TRANSPARENT
                                })
                                .radius(4.0)
                                .show(ui);
                            // Render the label as a sibling in the next pass:
                            // here we keep it simple and let the frame stand
                            // alone — text-on-frame requires zstack which we
                            // exercise elsewhere. Visual covers the row tint.
                            let _ = label;
                        }
                    });

                // Content: 2x2 grid of cards.
                Grid::with_id("cards")
                    .grid_cell((1, 1))
                    .cols([Track::fill(), Track::fill()])
                    .rows([Track::fill(), Track::fill()])
                    .gap(8.0)
                    .show(ui, |ui| {
                        let palette = [
                            Color::rgb(0.30, 0.45, 0.70),
                            Color::rgb(0.55, 0.35, 0.55),
                            Color::rgb(0.35, 0.55, 0.40),
                            Color::rgb(0.60, 0.45, 0.30),
                        ];
                        for (i, c) in palette.iter().enumerate() {
                            let row = (i / 2) as u16;
                            let col = (i % 2) as u16;
                            Panel::vstack_with_id(("card", i))
                                .grid_cell((row, col))
                                .padding(12.0)
                                .gap(6.0)
                                .fill(*c)
                                .stroke(Stroke {
                                    width: 1.0,
                                    color: Color::rgba(1.0, 1.0, 1.0, 0.18),
                                })
                                .radius(8.0)
                                .show(ui, |ui| {
                                    Text::with_id(("card-title", i), "Card")
                                        .size_px(14.0)
                                        .color(Color::rgb(0.95, 0.96, 1.00))
                                        .show(ui);
                                    Text::with_id(("card-body", i), "Some metric here")
                                        .size_px(11.0)
                                        .color(Color::rgba(1.0, 1.0, 1.0, 0.75))
                                        .show(ui);
                                });
                        }
                    });

                // Footer status bar.
                Panel::hstack_with_id("footer")
                    .grid_cell((2, 0))
                    .grid_span((1, 2))
                    .padding((4.0, 10.0, 4.0, 10.0))
                    .fill(Color::rgb(0.10, 0.12, 0.18))
                    .radius(4.0)
                    .show(ui, |ui| {
                        Text::with_id("status", "ready · 4 cards · scale 2.0")
                            .size_px(11.0)
                            .color(Color::rgb(0.65, 0.70, 0.80))
                            .show(ui);
                    });
            });
    });
    // Hi-dpi text AA is more sensitive than rect-only scenes; loosen
    // the tolerance accordingly.
    let tol = Tolerance {
        per_channel: 4,
        max_ratio: 0.005,
    };
    assert_matches_golden("dashboard_hidpi", &img, tol);
}
