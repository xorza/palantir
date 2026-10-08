//! Layout-driver fixtures: vstack, grid, zstack.

use glam::UVec2;
use palantir::internals::frame_fixture::FrameFixture;
use palantir::{
    Align, Background, Block, Button, Configure, Corners, Grid, GridCell, Panel, RgbaF32, Sizing,
    Stroke, Text, TextStyle, TextWrap, Track,
};

use crate::golden_name::GoldenName;
use crate::goldens::assert_scene_matches_golden;
use crate::harness::FIXTURE_PALETTE;

/// `Fill(1)`/`Fill(2)`/`Fill(1)` rows split the height 25/50/25.
#[test]
fn vstack_fill_weights_matches_golden() {
    assert_scene_matches_golden(GoldenName::VstackFillWeights, UVec2::new(160, 200), |ui| {
        Panel::vstack()
            .auto_id()
            .padding(8.0)
            .gap(4.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Block::new()
                    .id_salt("a")
                    .size((Sizing::FILL, Sizing::fill(1.0)))
                    .background(Background::fill(RgbaF32::srgb(0.85, 0.30, 0.30)))
                    .show(ui);
                Block::new()
                    .id_salt("b")
                    .size((Sizing::FILL, Sizing::fill(2.0)))
                    .background(Background::fill(RgbaF32::srgb(0.30, 0.85, 0.40)))
                    .show(ui);
                Block::new()
                    .id_salt("c")
                    .size((Sizing::FILL, Sizing::fill(1.0)))
                    .background(Background::fill(RgbaF32::srgb(0.30, 0.50, 0.95)))
                    .show(ui);
            });
    });
}

/// Grid with fixed and fill tracks, a gap and a spanning header row.
#[test]
fn grid_mixed_tracks_matches_golden() {
    assert_scene_matches_golden(GoldenName::GridMixedTracks, UVec2::new(320, 200), |ui| {
        Grid::new()
            .id_salt("g")
            .cols([Track::fixed(80.0), Track::FILL, Track::fixed(60.0)])
            .rows([Track::fixed(40.0), Track::FILL])
            .line_gap(6.0)
            .gap(6.0)
            .padding(10.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Block::new()
                    .id_salt("header")
                    .grid_cell(GridCell::at(0, 0).with_span(1, 3))
                    .background(Background::rounded(
                        RgbaF32::srgb(0.25, 0.30, 0.45),
                        Corners::all(4.0),
                    ))
                    .show(ui);
                Block::new()
                    .id_salt("side")
                    .grid_cell((1, 0))
                    .background(Background::rounded(
                        RgbaF32::srgb(0.35, 0.45, 0.30),
                        Corners::all(4.0),
                    ))
                    .show(ui);
                Block::new()
                    .id_salt("body")
                    .grid_cell((1, 1))
                    .background(Background::rounded(
                        RgbaF32::srgb(0.20, 0.20, 0.28),
                        Corners::all(4.0),
                    ))
                    .show(ui);
                Block::new()
                    .id_salt("aside")
                    .grid_cell((1, 2))
                    .background(Background::rounded(
                        RgbaF32::srgb(0.50, 0.30, 0.45),
                        Corners::all(4.0),
                    ))
                    .show(ui);
            });
    });
}

/// ZStack paint order and `Align::CENTER` arrangement.
#[test]
fn zstack_centered_button_matches_golden() {
    assert_scene_matches_golden(
        GoldenName::ZstackCenteredButton,
        UVec2::new(240, 160),
        |ui| {
            Panel::zstack()
                .auto_id()
                .padding(12.0)
                .size((Sizing::FILL, Sizing::FILL))
                .background(
                    Background::rounded(RgbaF32::srgb(0.16, 0.20, 0.28), Corners::all(10.0))
                        .with_border(Stroke::new(RgbaF32::srgb(0.30, 0.36, 0.46), 1.0)),
                )
                .show(ui, |ui| {
                    Button::new()
                        .id_salt("btn")
                        .align(Align::CENTER)
                        .label("centered")
                        .show(ui);
                });
        },
    );
}

/// Two `Hug` columns: the default-wrap label keeps its natural width while the paragraph wraps.
#[test]
fn grid_two_hug_cols_label_not_clipped_matches_golden() {
    assert_scene_matches_golden(
        GoldenName::GridTwoHugColsLabelNotClipped,
        UVec2::new(440, 120),
        |ui| {
            Panel::vstack()
                .auto_id()
                .padding(12.0)
                .size((Sizing::FILL, Sizing::HUG))
                .show(ui, |ui| {
                    Grid::new()
                        .id_salt("two-hug")
                        .cols([Track::HUG, Track::HUG])
                        .rows([Track::HUG])
                        .line_gap(16.0)
                        .gap(0.0)
                        .show(ui, |ui| {
                            let style = TextStyle::default()
                                .with_font_size(14.0)
                                .with_color(FIXTURE_PALETTE.text);
                            Text::new(
                                "The quick brown fox jumps over the lazy dog. \
                                 Pack my box with five dozen liquor jugs.",
                            )
                            .id_salt("paragraph")
                            .style(&style)
                            .text_wrap(TextWrap::WrapWithOverflow)
                            .grid_cell((0, 0))
                            .show(ui);
                            Text::new("right column")
                                .id_salt("label")
                                .style(&style)
                                .grid_cell((0, 1))
                                .show(ui);
                        });
                });
        },
    );
}

/// The frame bench's tree at scale 1, which records every public widget.
#[test]
fn frame_fixture_matches_golden() {
    let mut state = FrameFixture::default();
    assert_scene_matches_golden(GoldenName::FrameFixture, UVec2::new(1280, 800), |ui| {
        state.render(1, ui);
    });
}
