//! Per-widget fixtures: smallest possible scene that exercises one
//! widget's render path.

use glam::UVec2;
use palantir::{Button, Color, Configure, Frame, Panel, Sizing, Stroke, Styled};

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
