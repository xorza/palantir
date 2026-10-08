//! Disclosure fixtures: the header's arrow at both ends of its turn, and
//! the body one of them reveals.

use glam::UVec2;
use palantir::{Configure, Expander, Panel, Sizing, Text, TextWrap, Ui};

use crate::golden_name::GoldenName;
use crate::goldens::assert_settled_scene_matches_golden;

pub(crate) const SURFACE: UVec2 = UVec2::new(280, 124);

/// Two sections, one open, one closed, capturing the arrow at both ends of its turn and the body's indent against the header.
///
/// No settle loop past the second frame: the reveal snaps on first open; a theme with an `AnimationSpec` would need one.
pub(crate) fn scene(ui: &mut Ui) {
    Panel::vstack()
        .id_salt("well")
        .size((Sizing::FILL, Sizing::HUG))
        .padding(10.0)
        .gap(6.0)
        .show(ui, |ui| {
            Expander::new("Revealed")
                .id_salt("open")
                .start_open(true)
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

#[test]
fn expander_open_and_closed_matches_golden() {
    assert_settled_scene_matches_golden(GoldenName::ExpanderOpenAndClosed, SURFACE, 2, scene);
}
