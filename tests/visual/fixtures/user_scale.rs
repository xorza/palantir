//! The app's scale against the platform's. The render path sees only
//! their product, and a larger product paints larger.
//!
//! Neither fixture holds a golden. Both assert a relation between two
//! renders of one scene, which is what the property actually is — a
//! stored PNG would pin the scene's pixels and say nothing about the
//! relation.

use glam::UVec2;
use image::{Rgba, RgbaImage};
use palantir::{Background, Configure, Frame, Panel, RgbaF32, Sizing, Ui, UserScale};

use crate::fixtures::DARK_BG;
use crate::harness::Harness;

const SURFACE: UVec2 = UVec2::new(200, 160);

/// A 40×24 logical block against the top-left corner, with nothing
/// between it and the surface edge — so its painted rows and columns are
/// the logical size times whatever the frame's scale factor is.
fn block(ui: &mut Ui) {
    Panel::vstack()
        .id_salt("root")
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Frame::new()
                .id_salt("block")
                .size((Sizing::fixed(40.0), Sizing::fixed(24.0)))
                .background(Background::fill(RgbaF32::WHITE))
                .show(ui);
        });
}

/// How wide and how tall the painted region reaches, measured from the
/// corner the block is flush against.
///
/// The tolerance absorbs the linear→sRGB round trip the pipeline makes of
/// the clear colour, which lands the background a code point or two off
/// the value that went in. It is far below the white block's own
/// distance from it, so no edge pixel is miscounted either way.
const BACKGROUND_TOLERANCE: u8 = 8;

fn painted_extent(img: &RgbaImage, background: Rgba<u8>) -> UVec2 {
    let painted =
        |p: &Rgba<u8>| (0..3).any(|c| p.0[c].abs_diff(background.0[c]) > BACKGROUND_TOLERANCE);
    let mut extent = UVec2::ZERO;
    for (x, y, pixel) in img.enumerate_pixels() {
        if painted(pixel) {
            extent = extent.max(UVec2::new(x + 1, y + 1));
        }
    }
    extent
}

/// The two halves of the scale factor are interchangeable: the render
/// path multiplies them and never sees either alone, so dpr 2 with no
/// user scale must paint the same pixels as dpr 1 at 200%.
///
/// This is the property that lets the user scale reuse the whole hi-dpi
/// path rather than needing one of its own.
#[test]
fn the_two_halves_of_the_scale_factor_are_interchangeable() {
    let mut h = Harness::new();

    let system = h.render(SURFACE, 2.0, DARK_BG, block);
    h.host.ui().set_user_scale(UserScale::new(2.0));
    let user = h.render(SURFACE, 1.0, DARK_BG, block);

    assert_eq!(system, user);
}

/// Scaling up paints up. The 40×24 logical block covers 40×24 physical
/// pixels at 100% and 80×48 at 200%, measured from the surface corner it
/// is flush against.
#[test]
fn a_larger_user_scale_paints_a_larger_block() {
    let mut h = Harness::new();
    // Read back rather than converted: `DARK_BG` is linear and the target
    // is sRGB, so what the background *is* in these bytes is what an empty
    // render says it is.
    let background = {
        let empty = h.render(SURFACE, 1.0, DARK_BG, |_: &mut Ui| {});
        *empty.get_pixel(SURFACE.x - 1, SURFACE.y - 1)
    };

    let plain = h.render(SURFACE, 1.0, DARK_BG, block);
    assert_eq!(painted_extent(&plain, background), UVec2::new(40, 24));

    h.host.ui().set_user_scale(UserScale::new(2.0));
    let doubled = h.render(SURFACE, 1.0, DARK_BG, block);
    assert_eq!(painted_extent(&doubled, background), UVec2::new(80, 48));
}
