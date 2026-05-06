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

/// Pins the rounded-clip stencil path. A `Surface::rounded(...)` panel
/// holds an oversized child that overflows on every side via negative
/// margins; the stencil mask must trim the child at the painted radius
/// so the panel's corners look clean against the dark background.
#[test]
fn surface_rounded_clips_overflow_to_corners() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(220, 220), 1.0, DARK_BG, |ui| {
        Panel::vstack().padding(20.0).show(ui, |ui| {
            Panel::zstack()
                .with_id("card")
                .size((Sizing::FILL, Sizing::FILL))
                .background(Surface::rounded(Background {
                    fill: Color::rgb(0.18, 0.22, 0.30),
                    stroke: Some(Stroke {
                        width: 1.5,
                        color: Color::rgb(0.55, 0.65, 0.78),
                    }),
                    radius: Corners::all(28.0),
                }))
                .show(ui, |ui| {
                    Frame::new()
                        .with_id("spill")
                        .size((Sizing::Fixed(360.0), Sizing::Fixed(360.0)))
                        .margin((-60.0, -60.0, -60.0, -60.0))
                        .background(Background {
                            fill: Color::rgb(0.92, 0.32, 0.36),
                            ..Default::default()
                        })
                        .show(ui);
                });
        });
    });
    assert_matches_golden("surface_rounded_clips_overflow", &img, Tolerance::default());
}
