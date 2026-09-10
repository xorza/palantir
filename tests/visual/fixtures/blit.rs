//! Presenting onto a target that cannot be copied into.
//!
//! A GLES swapchain image *is* the default framebuffer, so EGL offers
//! `RENDER_ATTACHMENT` alone and the renderer cannot copy its retained
//! backbuffer onto it. It draws the backbuffer instead. Measured on a
//! Raspberry Pi: `WGPU_BACKEND=gl` gives a V3D surface with exactly that one
//! usage, and every frame the showcase paints goes through the draw.
//!
//! The draw stands in for a copy, so it has to *be* one.

use glam::UVec2;
use palantir::{Configure, Expander, Panel, Sizing, Text, TextWrap, Ui};

use crate::fixtures::DARK_BG;
use crate::harness::Harness;

const SURFACE: UVec2 = UVec2::new(220, 110);

/// Text and a disclosure arrow, because antialiased edges are where a blit
/// that is nearly right goes wrong. Sampling the backbuffer through the
/// group-0 sampler — which is *linear*, shared with the image draws that snap
/// their own UVs — put a fraction of each neighbour into every pixel beside an
/// edge, lifting dark background from 16 to 26. Only a pixel comparison showed
/// it, which is why this fixture exists rather than a unit test on the path
/// selection.
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
    let copied = Harness::new().render_after_settle(2, SURFACE, 1.0, DARK_BG, scene);
    let drawn = Harness::new()
        .without_copy_dst()
        .render_after_settle(2, SURFACE, 1.0, DARK_BG, scene);

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
