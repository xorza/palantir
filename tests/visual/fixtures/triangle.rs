//! Triangles whose corners do not span an area.

use glam::{UVec2, Vec2};
use palantir::widget::Shape;
use palantir::{Configure, Panel, RgbaF32, Sizing};

use crate::harness::Harness;

/// A triangle with no area has no inside, so a radius grows its edges and
/// corners alone: three collinear corners paint a 4 px stadium along their
/// segment, three coincident ones a 6 px disc. Neither fills the quad the
/// radius inflates.
///
/// The stadium runs from (10, 20) to (90, 20), so its quad is x 6..94,
/// y 16..24. Pixel (6, 16) has its centre at (6.5, 16.5), 4.95 px from the
/// end at (10, 20): outside by almost a pixel. The disc sits at (50, 50)
/// in the quad 44..56; pixel (44, 44)'s centre is 7.8 px from it.
#[test]
fn a_degenerate_rounded_triangle_paints_its_edges_not_its_quad() {
    let mut h = Harness::new();
    let img = h
        .size(UVec2::new(100, 70))
        .clear(RgbaF32::BLACK)
        .frame(|ui| {
            Panel::canvas()
                .id_salt("degenerate")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    ui.add_shape(
                        Shape::triangle(
                            Vec2::new(10.0, 20.0),
                            Vec2::new(50.0, 20.0),
                            Vec2::new(90.0, 20.0),
                        )
                        .fill(RgbaF32::WHITE)
                        .radius(4.0_f32),
                    );
                    ui.add_shape(
                        Shape::triangle(Vec2::splat(50.0), Vec2::splat(50.0), Vec2::splat(50.0))
                            .fill(RgbaF32::WHITE)
                            .radius(6.0_f32),
                    );
                });
        })
        .image;
    let lit = |x: u32, y: u32| img.get_pixel(x, y).0[0] > 200;
    let dark = |x: u32, y: u32| img.get_pixel(x, y).0[0] < 40;

    assert!(lit(50, 20), "the stadium's spine");
    assert!(lit(50, 17), "3.5 px from the spine, inside the 4 px radius");
    assert!(dark(6, 16), "the stadium's quad corner");
    assert!(dark(93, 23), "the stadium's other quad corner");
    assert!(lit(50, 50), "the disc's centre");
    assert!(dark(44, 44), "the disc's quad corner");
    assert!(dark(55, 55), "the disc's other quad corner");
}
