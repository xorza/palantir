//! Text rendering fixtures.

use glam::UVec2;
use palantir::{Background, Configure, Panel, RgbaF32, Sizing, Text, TextStyle};

use crate::golden_name::GoldenName;
use crate::goldens::assert_scene_matches_golden;

/// Multi-line paragraph with mixed sizes/colors; looser tolerance as glyph AA varies across drivers.
#[test]
fn text_paragraph_matches_golden() {
    assert_scene_matches_golden(GoldenName::TextParagraph, UVec2::new(360, 140), |ui| {
        Panel::vstack()
            .auto_id()
            .padding(16.0)
            .gap(6.0)
            .show(ui, |ui| {
                Text::new("Palantir")
                    .id_salt("title")
                    .style(
                        &TextStyle::default()
                            .with_font_size(20.0)
                            .with_color(RgbaF32::srgb(0.92, 0.94, 1.00)),
                    )
                    .show(ui);
                Text::new("Immediate-mode UI with WPF-style layout.")
                    .id_salt("body")
                    .style(
                        &TextStyle::default()
                            .with_font_size(13.0)
                            .with_color(RgbaF32::srgb(0.72, 0.76, 0.84)),
                    )
                    .show(ui);
                Text::new("Rendered headlessly through wgpu.")
                    .id_salt("body2")
                    .style(
                        &TextStyle::default()
                            .with_font_size(13.0)
                            .with_color(RgbaF32::srgb(0.72, 0.76, 0.84)),
                    )
                    .show(ui);
            });
    });
}

/// Many labels under per-row backgrounds: text-batch coalescing across distinct scissors (each row's background makes a group), the composer fusing all text into one glyphon `prepare`/`render`. Every label must read on its row, no glyphs missing.
#[test]
fn text_row_list_batches_into_one_render() {
    let rows = [
        ("Alpha", RgbaF32::srgb(0.20, 0.40, 0.60)),
        ("Bravo", RgbaF32::srgb(0.60, 0.30, 0.20)),
        ("Charlie", RgbaF32::srgb(0.25, 0.55, 0.30)),
        ("Delta", RgbaF32::srgb(0.55, 0.40, 0.65)),
        ("Echo", RgbaF32::srgb(0.35, 0.55, 0.55)),
        ("Foxtrot", RgbaF32::srgb(0.65, 0.55, 0.30)),
    ];
    assert_scene_matches_golden(GoldenName::TextRowListBatched, UVec2::new(220, 200), |ui| {
        Panel::vstack()
            .auto_id()
            .padding(8.0)
            .gap(4.0)
            .show(ui, |ui| {
                for (label, bg) in rows {
                    Panel::hstack()
                        .id_salt(label)
                        .padding(6.0)
                        .background(Background::fill(bg))
                        .size((Sizing::fixed(200.0), Sizing::HUG))
                        .show(ui, |ui| {
                            Text::new(label)
                                .auto_id()
                                .style(
                                    &TextStyle::default()
                                        .with_font_size(14.0)
                                        .with_color(RgbaF32::srgb(0.95, 0.95, 1.00)),
                                )
                                .show(ui);
                        });
                }
            });
    });
}
