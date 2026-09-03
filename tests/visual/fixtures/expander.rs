//! Disclosure fixtures: the header's arrow at both ends of its turn, and
//! the body one of them reveals.

use glam::UVec2;
use palantir::golden::Tolerance;
use palantir::{Configure, Expander, Panel, Sizing, Text, TextWrap, Ui};

use crate::fixtures::DARK_BG;
use crate::goldens::assert_matches_golden;
use crate::harness::Harness;

/// Two sections, one open and one closed, so the arrow is captured at
/// both ends of its turn and the body's indent is measurable against the
/// header above it.
///
/// No settle loop past the second frame: the reveal snaps on a first
/// open, so the golden would capture the same pixels either way — but a
/// fixture that later gives its theme an `AnimSpec` would need one.
#[test]
fn expander_open_and_closed_matches_golden() {
    let mut h = Harness::new();
    fn scene(ui: &mut Ui) {
        Panel::vstack()
            .id_salt("well")
            .size((Sizing::FILL, Sizing::HUG))
            .padding(10.0)
            .gap(6.0)
            .show(ui, |ui| {
                Expander::new("Revealed")
                    .id_salt("open")
                    .default_open(true)
                    .show(ui, |ui| {
                        Text::new("the body an open header shows")
                            .id_salt("body")
                            .text_wrap(TextWrap::WrapWithOverflow)
                            .size((Sizing::FILL, Sizing::HUG))
                            .show(ui);
                    });
                Expander::new("Hidden").id_salt("closed").show(ui, |ui| {
                    Text::new("never recorded").id_salt("body").show(ui);
                });
            });
    }
    let img = h.render_after_settle(2, UVec2::new(280, 124), 1.0, DARK_BG, scene);
    assert_matches_golden("expander_open_and_closed", &img, Tolerance::default());
}
