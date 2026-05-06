//! Per-widget fixtures: smallest possible scene that exercises one
//! widget's render path.

use glam::UVec2;
use palantir::{
    Background, Button, Color, Configure, Corners, Frame, Panel, Sizing, Stroke, Surface,
};

use crate::diff::Tolerance;
use crate::fixtures::DARK_BG;
use crate::golden::assert_matches_golden;
use crate::harness::Harness;

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
            Frame::new()
                .with_id("card")
                .size((Sizing::FILL, Sizing::FILL))
                .background(Background {
                    fill: Color::rgb(0.20, 0.30, 0.55),
                    stroke: Some(Stroke {
                        width: 2.0,
                        color: Color::rgb(0.65, 0.80, 1.00),
                    }),
                    radius: Corners::all(16.0),
                })
                .show(ui);
        });
    });
    assert_matches_golden("frame_filled_with_stroke", &img, Tolerance::default());
}

/// Pins the rounded-clip stencil path. Layered: full-canvas pink, then
/// a smaller rounded panel (per-corner distinct radii, 1px black
/// stroke, rounded clip), then a full-fill black child whose square
/// corners must be trimmed by the stencil mask. Per-corner radii test
/// the SDF's corner mixing — uniform-radius bug would still pass a
/// `Corners::all(...)` fixture.
#[test]
fn surface_rounded_clips_full_fill_child() {
    let mut h = Harness::new();
    let pink = Color::rgb(1.0, 0.42, 0.72);
    let black = Color::rgb(0.0, 0.0, 0.0);
    let img = h.render(UVec2::new(220, 220), 1.0, DARK_BG, |ui| {
        Panel::vstack()
            .size((Sizing::FILL, Sizing::FILL))
            .padding(20.0)
            .background(Background {
                fill: pink,
                ..Default::default()
            })
            .show(ui, |ui| {
                Panel::zstack()
                    .with_id("rounded")
                    .size((Sizing::FILL, Sizing::FILL))
                    .background(Surface::rounded(Background {
                        fill: Color::TRANSPARENT,
                        stroke: Some(Stroke {
                            width: 1.0,
                            color: Color::rgb_u8(0, 255, 0),
                        }),
                        radius: Corners::new(4.0, 12.0, 20.0, 28.0),
                    }))
                    .show(ui, |ui| {
                        Frame::new()
                            .with_id("inner")
                            .size((Sizing::FILL, Sizing::FILL))
                            .background(Background {
                                fill: black,
                                ..Default::default()
                            })
                            .show(ui);
                    });
            });
    });
    assert_matches_golden(
        "surface_rounded_clips_full_fill_child",
        &img,
        Tolerance::default(),
    );
}
