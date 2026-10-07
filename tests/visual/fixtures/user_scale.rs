//! The app's scale against the platform's: the render path sees only their product, and a larger product paints larger. Neither fixture holds a golden; each asserts a relation between two renders.

use glam::UVec2;
use palantir::golden::image::{Rgba, RgbaImage};
use palantir::{Background, Block, Configure, Panel, RgbaF32, Sizing, Ui, UserScale};

use crate::goldens::assert_same;
use crate::harness::Harness;

const SURFACE: UVec2 = UVec2::new(200, 160);

/// A 40×24 logical block flush against the top-left corner, so painted rows and columns are the logical size times the scale factor.
fn block(ui: &mut Ui) {
    Panel::vstack()
        .id_salt("root")
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Block::new()
                .id_salt("block")
                .size((Sizing::fixed(40.0), Sizing::fixed(24.0)))
                .background(Background::fill(RgbaF32::WHITE))
                .show(ui);
        });
}

/// How far the painted region reaches from the corner. `background` is read from an empty render on the same pipeline, so it is the exact bytes of an unpainted pixel.
fn painted_extent(img: &RgbaImage, background: Rgba<u8>) -> UVec2 {
    let painted = |p: &Rgba<u8>| *p != background;
    let mut extent = UVec2::ZERO;
    for (x, y, pixel) in img.enumerate_pixels() {
        if painted(pixel) {
            extent = extent.max(UVec2::new(x + 1, y + 1));
        }
    }
    extent
}

/// The two scale halves are interchangeable: dpr 2 with no user scale paints the same pixels as dpr 1 at 200%, which lets user scale reuse the hi-dpi path.
#[test]
fn the_two_halves_of_the_scale_factor_are_interchangeable() {
    let mut h = Harness::new();

    let system = h.size(SURFACE).scale(2.0).frame(block).image;
    h.host.ui().set_user_scale(UserScale::new(2.0).unwrap());
    let user = h.scale(1.0).frame(block).image;

    assert_same("user_scale_halves", &user, &system);
}

/// Scaling up paints up: 40×24 physical at 100%, 80×48 at 200%.
#[test]
fn a_larger_user_scale_paints_a_larger_block() {
    let mut h = Harness::new();
    // Read back, not converted: `DARK_BG` is linear and the target sRGB, so use what an empty render says.
    let background = {
        let empty = h.size(SURFACE).frame(|_: &mut Ui| {}).image;
        *empty.get_pixel(SURFACE.x - 1, SURFACE.y - 1)
    };

    let plain = h.size(SURFACE).frame(block).image;
    assert_eq!(painted_extent(&plain, background), UVec2::new(40, 24));

    h.host.ui().set_user_scale(UserScale::new(2.0).unwrap());
    let doubled = h.size(SURFACE).frame(block).image;
    assert_eq!(painted_extent(&doubled, background), UVec2::new(80, 48));
}
