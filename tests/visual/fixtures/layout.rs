//! Layout-driver fixtures: vstack/grid/zstack at their minimal
//! exercise-everything sizes.

use glam::UVec2;
use palantir::{
    Align, Background, Button, Color, Configure, Corners, Frame, Grid, Panel, Sizing, Stroke, Track,
};

use crate::diff::Tolerance;
use crate::fixtures::DARK_BG;
use crate::golden::assert_matches_golden;
use crate::harness::Harness;

/// Three rows of `Fill(1)` / `Fill(2)` / `Fill(1)` — should split the
/// available height in 25/50/25 ratios.
#[test]
fn vstack_fill_weights_matches_golden() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(160, 200), 1.0, DARK_BG, |ui| {
        Panel::vstack()
            .auto_id()
            .padding(8.0)
            .gap(4.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("a")
                    .size((Sizing::FILL, Sizing::Fill(1.0)))
                    .background(Background {
                        fill: Color::rgb(0.85, 0.30, 0.30).into(),
                        ..Default::default()
                    })
                    .show(ui);
                Frame::new()
                    .id_salt("b")
                    .size((Sizing::FILL, Sizing::Fill(2.0)))
                    .background(Background {
                        fill: Color::rgb(0.30, 0.85, 0.40).into(),
                        ..Default::default()
                    })
                    .show(ui);
                Frame::new()
                    .id_salt("c")
                    .size((Sizing::FILL, Sizing::Fill(1.0)))
                    .background(Background {
                        fill: Color::rgb(0.30, 0.50, 0.95).into(),
                        ..Default::default()
                    })
                    .show(ui);
            });
    });
    assert_matches_golden("vstack_fill_weights", &img, Tolerance::default());
}

/// Grid with mixed track types (fixed / fill), gap, and a spanning
/// header row. Tests the grid layout driver end to end.
#[test]
fn grid_mixed_tracks_matches_golden() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(320, 200), 1.0, DARK_BG, |ui| {
        Grid::new()
            .id_salt("g")
            .cols([Track::fixed(80.0), Track::fill(), Track::fixed(60.0)])
            .rows([Track::fixed(40.0), Track::fill()])
            .gap(6.0)
            .padding(10.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("header")
                    .grid_cell((0, 0))
                    .grid_span((1, 3))
                    .background(Background {
                        fill: Color::rgb(0.25, 0.30, 0.45).into(),
                        radius: Corners::all(4.0),
                        ..Default::default()
                    })
                    .show(ui);
                Frame::new()
                    .id_salt("side")
                    .grid_cell((1, 0))
                    .background(Background {
                        fill: Color::rgb(0.35, 0.45, 0.30).into(),
                        radius: Corners::all(4.0),
                        ..Default::default()
                    })
                    .show(ui);
                Frame::new()
                    .id_salt("body")
                    .grid_cell((1, 1))
                    .background(Background {
                        fill: Color::rgb(0.20, 0.20, 0.28).into(),
                        radius: Corners::all(4.0),
                        ..Default::default()
                    })
                    .show(ui);
                Frame::new()
                    .id_salt("aside")
                    .grid_cell((1, 2))
                    .background(Background {
                        fill: Color::rgb(0.50, 0.30, 0.45).into(),
                        radius: Corners::all(4.0),
                        ..Default::default()
                    })
                    .show(ui);
            });
    });
    assert_matches_golden("grid_mixed_tracks", &img, Tolerance::default());
}

/// ZStack: tinted background frame + centered button on top. Tests
/// paint order (background drawn first, foreground on top) and
/// `Align::CENTER` arrangement.
#[test]
fn zstack_centered_button_matches_golden() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(240, 160), 1.0, DARK_BG, |ui| {
        Panel::zstack()
            .auto_id()
            .padding(12.0)
            .size((Sizing::FILL, Sizing::FILL))
            .background(Background {
                fill: Color::rgb(0.16, 0.20, 0.28).into(),
                stroke: Stroke::solid(Color::rgb(0.30, 0.36, 0.46), 1.0),
                radius: Corners::all(10.0),
                shadow: None,
            })
            .show(ui, |ui| {
                Button::new()
                    .id_salt("btn")
                    .align(Align::CENTER)
                    .label("centered")
                    .show(ui);
            });
    });
    assert_matches_golden("zstack_centered_button", &img, Tolerance::default());
}
