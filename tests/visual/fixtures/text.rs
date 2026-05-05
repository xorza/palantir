//! Text rendering fixtures.

use glam::UVec2;
use palantir::{Color, Configure, Panel, Text, TextStyle};

use crate::diff::Tolerance;
use crate::fixtures::DARK_BG;
use crate::golden::assert_matches_golden;
use crate::harness::Harness;

/// Multi-line paragraph with mixed sizes/colors. Slightly looser
/// tolerance — glyph AA varies more across drivers than rect-only
/// scenes.
#[test]
fn text_paragraph_matches_golden() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(360, 140), 1.0, DARK_BG, |ui| {
        Panel::vstack().padding(16.0).gap(6.0).show(ui, |ui| {
            Text::new("Palantir")
                .with_id("title")
                .style(
                    TextStyle::default()
                        .with_font_size(20.0)
                        .with_color(Color::rgb(0.92, 0.94, 1.00)),
                )
                .show(ui);
            Text::new("Immediate-mode UI with WPF-style layout.")
                .with_id("body")
                .style(
                    TextStyle::default()
                        .with_font_size(13.0)
                        .with_color(Color::rgb(0.72, 0.76, 0.84)),
                )
                .show(ui);
            Text::new("Rendered headlessly through wgpu.")
                .with_id("body2")
                .style(
                    TextStyle::default()
                        .with_font_size(13.0)
                        .with_color(Color::rgb(0.72, 0.76, 0.84)),
                )
                .show(ui);
        });
    });
    let tol = Tolerance {
        per_channel: 4,
        max_ratio: 0.005,
    };
    assert_matches_golden("text_paragraph", &img, tol);
}
