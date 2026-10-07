//! Presenting onto a target that cannot be copied into.
//!
//! A GLES swapchain image is the default framebuffer, so EGL offers `RENDER_ATTACHMENT` alone and the renderer draws the retained backbuffer instead of copying it; the draw has to stand in for a copy exactly.

use glam::UVec2;
use palantir::{Configure, Expander, Panel, Sizing, Text, TextWrap, Ui};

use crate::harness::Harness;

const SURFACE: UVec2 = UVec2::new(220, 110);

/// Text and a disclosure arrow: antialiased edges expose a nearly-right blit (sampling through the linear group-0 sampler once lifted dark background from 16 to 26).
fn scene(ui: &mut Ui) {
    Panel::vstack()
        .id_salt("well")
        .size((Sizing::FILL, Sizing::HUG))
        .padding(10.0)
        .gap(6.0)
        .show(ui, |ui| {
            Expander::new("Presented")
                .id_salt("open")
                .start_open(true)
                .show(ui, |ui| {
                    Text::new("drawn, not copied")
                        .id_salt("body")
                        .text_wrap(TextWrap::WrapWithOverflow)
                        .size((Sizing::FILL, Sizing::HUG))
                        .show(ui);
                });
        });
}

#[test]
fn a_target_that_takes_no_copy_presents_the_same_pixels() {
    let copied = Harness::new().size(SURFACE).settled_frame(2, scene).image;
    let drawn = Harness::new()
        .without_copy_dst()
        .size(SURFACE)
        .settled_frame(2, scene)
        .image;

    assert_eq!(copied.dimensions(), drawn.dimensions());
    let differing = copied
        .pixels()
        .zip(drawn.pixels())
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        differing, 0,
        "drawing the backbuffer must land the same bytes as copying it"
    );
}
