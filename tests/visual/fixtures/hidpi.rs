//! Hi-dpi (scale > 1.0) fixtures. Pixel-snap and sub-pixel positioning
//! diverge from the 1.0-scale path here.

use glam::UVec2;
use palantir::{Button, Color, Configure, Frame, Grid, Panel, Sizing, Stroke, Styled, Text, Track};

use crate::diff::Tolerance;
use crate::fixtures::DARK_BG;
use crate::golden::assert_matches_golden;
use crate::harness::Harness;

/// Complex multi-region scene at scale 2.0. Exercises:
///   - header / sidebar / content / footer grid layout,
///   - nested vstacks + hstacks with mixed sizing,
///   - text at multiple sizes,
///   - rounded-rect AA + strokes at sub-pixel positions (scale 2.0
///     puts logical pixel edges on physical half-pixels),
///   - the renderer's pixel_snap path under non-1.0 scale.
///
/// Physical 800×600 = logical 400×300 at scale 2.0.
#[test]
fn dashboard_matches_golden() {
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
                        for i in 0..5 {
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
    // Hi-dpi text AA is more sensitive than rect-only scenes.
    let tol = Tolerance {
        per_channel: 4,
        max_ratio: 0.005,
    };
    assert_matches_golden("dashboard_hidpi", &img, tol);
}
