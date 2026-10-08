//! Hi-dpi (scale > 1.0) fixtures, where pixel-snap and sub-pixel positioning diverge from scale 1.0.

use glam::UVec2;
use palantir::{
    Background, Block, Button, Configure, Corners, Grid, GridCell, Panel, RgbaF32, Sizing, Stroke,
    Text, TextStyle, Track,
};

use crate::golden_name::GoldenName;
use crate::goldens::assert_matches_golden;
use crate::harness::Harness;

/// Complex multi-region scene at scale 2.0 (physical 800×600 = logical 400×300): header/sidebar/content/footer grid, nested stacks with mixed sizing, text at several sizes, rounded-rect AA and strokes on physical half-pixels, and the renderer's pixel_snap path.
#[test]
fn dashboard_matches_golden() {
    let mut h = Harness::new();
    let img = h
        .size(UVec2::new(800, 600))
        .scale(2.0)
        .frame(|ui| {
            Grid::new()
                .id_salt("shell")
                .cols([Track::fixed(110.0), Track::FILL])
                .rows([Track::fixed(40.0), Track::FILL, Track::fixed(24.0)])
                .line_gap(8.0)
                .gap(8.0)
                .padding(12.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Panel::hstack()
                        .id_salt("header")
                        .grid_cell(GridCell::at(0, 0).with_span(1, 2))
                        .size((Sizing::FILL, Sizing::FILL))
                        .max_size((f32::INFINITY, 40.0))
                        .padding((10.0, 14.0, 10.0, 14.0))
                        .gap(8.0)
                        .background(
                            Background::rounded(RgbaF32::srgb(0.18, 0.22, 0.32), Corners::all(6.0))
                                .with_border(Stroke::new(RgbaF32::srgb(0.30, 0.36, 0.46), 1.0)),
                        )
                        .show(ui, |ui| {
                            Text::new("Palantir")
                                .id_salt("brand")
                                .style(
                                    &TextStyle::default()
                                        .with_font_size(16.0)
                                        .with_color(RgbaF32::srgb(0.92, 0.94, 1.00)),
                                )
                                .show(ui);
                            Block::new()
                                .id_salt("spacer")
                                .size((Sizing::FILL, Sizing::fixed(1.0)))
                                .show(ui);
                            Button::new().id_salt("btn-save").label("save").show(ui);
                            Button::new().id_salt("btn-export").label("export").show(ui);
                        });

                    Panel::vstack()
                        .id_salt("sidebar")
                        .grid_cell((1, 0))
                        .padding(8.0)
                        .gap(4.0)
                        .background(Background::rounded(
                            RgbaF32::srgb(0.14, 0.17, 0.24),
                            Corners::all(6.0),
                        ))
                        .show(ui, |ui| {
                            for i in 0..5 {
                                Block::new()
                                    .id_salt(("nav-bg", i))
                                    .size((Sizing::FILL, Sizing::fixed(28.0)))
                                    .padding((6.0, 8.0, 6.0, 8.0))
                                    .background(Background::rounded(
                                        if i == 1 {
                                            RgbaF32::srgb(0.22, 0.30, 0.46)
                                        } else {
                                            RgbaF32::TRANSPARENT
                                        },
                                        Corners::all(4.0),
                                    ))
                                    .show(ui);
                            }
                        });

                    Grid::new()
                        .id_salt("cards")
                        .grid_cell((1, 1))
                        .cols([Track::FILL, Track::FILL])
                        .rows([Track::FILL, Track::FILL])
                        .line_gap(8.0)
                        .gap(8.0)
                        .show(ui, |ui| {
                            let palette = [
                                RgbaF32::srgb(0.30, 0.45, 0.70),
                                RgbaF32::srgb(0.55, 0.35, 0.55),
                                RgbaF32::srgb(0.35, 0.55, 0.40),
                                RgbaF32::srgb(0.60, 0.45, 0.30),
                            ];
                            for (i, c) in palette.iter().enumerate() {
                                let row = (i / 2) as u16;
                                let col = (i % 2) as u16;
                                Panel::vstack()
                                    .id_salt(("card", i))
                                    .grid_cell((row, col))
                                    .padding(12.0)
                                    .gap(6.0)
                                    .background(
                                        Background::rounded(*c, Corners::all(8.0)).with_border(
                                            Stroke::new(RgbaF32::srgba(1.0, 1.0, 1.0, 0.18), 1.0),
                                        ),
                                    )
                                    .show(ui, |ui| {
                                        Text::new("Card")
                                            .id_salt(("card-title", i))
                                            .style(
                                                &TextStyle::default()
                                                    .with_font_size(14.0)
                                                    .with_color(RgbaF32::srgb(0.95, 0.96, 1.00)),
                                            )
                                            .show(ui);
                                        Text::new("Some metric here")
                                            .id_salt(("card-body", i))
                                            .style(
                                                &TextStyle::default()
                                                    .with_font_size(11.0)
                                                    .with_color(RgbaF32::srgba(
                                                        1.0, 1.0, 1.0, 0.75,
                                                    )),
                                            )
                                            .show(ui);
                                    });
                            }
                        });

                    Panel::hstack()
                        .id_salt("footer")
                        .grid_cell(GridCell::at(2, 0).with_span(1, 2))
                        .max_size((f32::INFINITY, 24.0))
                        .padding((4.0, 10.0, 4.0, 10.0))
                        .background(Background::rounded(
                            RgbaF32::srgb(0.10, 0.12, 0.18),
                            Corners::all(4.0),
                        ))
                        .show(ui, |ui| {
                            Text::new("ready · 4 cards · scale 2.0")
                                .id_salt("status")
                                .style(
                                    &TextStyle::default()
                                        .with_font_size(11.0)
                                        .with_color(RgbaF32::srgb(0.65, 0.70, 0.80)),
                                )
                                .show(ui);
                        });
                });
        })
        .image;
    assert_matches_golden(GoldenName::DashboardHidpi, &img);
}
