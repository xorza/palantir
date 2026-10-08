//! Presenting onto a target that cannot be copied into.
//!
//! A GLES swapchain image is the default framebuffer, so EGL offers `RENDER_ATTACHMENT` alone and the renderer draws the retained backbuffer instead of copying it; the draw has to stand in for a copy exactly.

use crate::fixtures::expander;
use crate::goldens::assert_same;
use crate::harness::Harness;

/// The expander scene's text and disclosure arrows: antialiased edges expose a nearly-right blit (sampling through the linear group-0 sampler once lifted dark background from 16 to 26).
#[test]
fn a_target_that_takes_no_copy_presents_the_same_pixels() {
    let copied = Harness::new()
        .size(expander::SURFACE)
        .settled_frame(2, expander::scene)
        .image;
    let drawn = Harness::new()
        .without_copy_dst()
        .size(expander::SURFACE)
        .settled_frame(2, expander::scene)
        .image;
    assert_same("blit_drawn_not_copied", &drawn, &copied);
}
