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
