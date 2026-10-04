//! Pixel-level shadow fixtures.

use glam::{IVec2, UVec2, Vec2};
use palantir::golden::image::RgbaImage;
use palantir::widget::Shape;
use palantir::{Configure, Panel, Rect, RgbaF32, Shadow, Sizing};

use crate::goldens::{assert_same, assert_same_in, crop};
use crate::harness::Harness;

const VIEWPORT: UVec2 = UVec2::new(220, 180);
const CLEAR: RgbaF32 = RgbaF32::WHITE;

fn render_shadow(
    source: Rect,
    corners: f32,
    offset: Vec2,
    blur: f32,
    spread: f32,
    inset: bool,
) -> RgbaImage {
    let mut harness = Harness::new();
    harness
        .size(VIEWPORT)
        .clear(CLEAR)
        .frame(|ui| {
            Panel::canvas()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    ui.add_shape(
                        Shape::shadow(Shadow {
                            color: RgbaF32::srgba(0.0, 0.0, 0.0, 0.85),
                            offset,
                            blur,
                            spread,
                            inset,
                        })
                        .at(source)
                        .corners(corners),
                    );
                });
        })
        .image
}

#[test]
fn shifted_drop_bbox_preserves_positive_and_negative_offset_pixels() {
    let source = Rect::new(64.0, 60.0, 72.0, 54.0);

    for offset in [Vec2::new(17.0, 13.0), Vec2::new(-19.0, -11.0)] {
        let shifted = render_shadow(source, 11.0, offset, 6.0, 4.0, false);
        let reference = render_shadow(
            Rect {
                min: source.min + offset,
                size: source.size,
            },
            11.0,
            Vec2::ZERO,
            6.0,
            4.0,
            false,
        );
        let name = format!("shadow_offset_{}_{}", offset.x, offset.y);
        assert_same(&name, &shifted, &reference);
    }
}

#[test]
fn inset_offset_matches_translated_zero_offset_pixels_inside_source() {
    let source = Rect::new(40.0, 35.0, 120.0, 100.0);
    let pixel_offset = IVec2::new(9, -7);
    let shifted = render_shadow(source, 11.0, pixel_offset.as_vec2(), 6.0, 8.0, true);
    let reference = render_shadow(source, 11.0, Vec2::ZERO, 6.0, 8.0, true);

    // The shifted shadow at a region, against the unshifted one at the
    // same region moved back by the offset.
    let region = Rect::new(60.0, 50.0, 85.0, 70.0);
    let unshifted = Rect {
        min: region.min - pixel_offset.as_vec2(),
        size: region.size,
    };
    assert_same(
        "shadow_inset_translated",
        &crop(&shifted, region),
        &crop(&reference, unshifted),
    );
}

/// A spread is the same shadow as a source grown or shrunk by it, with
/// its radii moved by the CSS spread rule: a drop shadow's 11 px corners
/// at spread −4 become `max(11 − 4, 0) = 7`, and an inset hole grown by 4
/// rounds at `11 + 4 = 15`, since 11 ≥ 4.
#[test]
fn negative_spread_deflates_drop_and_inset_shadow_geometry() {
    let blur = 6.0;
    let spread = -4.0;

    let drop_source = Rect::new(64.0, 60.0, 72.0, 54.0);
    let drop = render_shadow(drop_source, 11.0, Vec2::ZERO, blur, spread, false);
    let deflated_source = drop_source.inflated(spread);
    let drop_reference =
        render_shadow(deflated_source, 11.0 + spread, Vec2::ZERO, blur, 0.0, false);
    assert_same_in(
        "shadow_drop_negative_spread",
        &drop,
        &drop_reference,
        deflated_source.inflated(3.0 * blur),
    );

    let inset_source = Rect::new(40.0, 35.0, 120.0, 100.0);
    let inset = render_shadow(inset_source, 11.0, Vec2::ZERO, blur, spread, true);
    let inset_reference = render_shadow(
        inset_source.inflated(-spread),
        11.0 - spread,
        Vec2::ZERO,
        blur,
        0.0,
        true,
    );
    assert_same_in(
        "shadow_inset_negative_spread",
        &inset,
        &inset_reference,
        inset_source.inflated(-12.0),
    );
}

/// Drop shadows with no blur, so each edge is a hard one a probe can
/// read: the radius grows or shrinks with the spread by the CSS rule,
/// then fits the shadow box.
///
/// A 40 px circle (corners 20) centred at (110, 90):
/// - spread +6 → a 52 px box with radius 26, a circle. The probe 27.6 px
///   out along the diagonal is past it; with the radius left at 20 the
///   corner arc reached 6√2 + 20 = 28.5 there.
/// - spread −6 → a 28 px box with radius `max(20 − 6, 0) = 14`, a circle.
///   The probe 13.5 px straight down is inside it; with the radius left at
///   20 the shape was a squircle 13.6 px from the centre on that axis.
///
/// A sharp 40 px box with spread +6 stays sharp: its 52 px shadow fills
/// the corner pixel at (84.5, 64.5).
#[test]
fn a_spread_moves_the_shadow_radius_by_the_css_rule() {
    let circle = Rect::new(90.0, 70.0, 40.0, 40.0);
    // 85 % black over white leaves 0.15 linear, which encodes to sRGB 107.
    let dark = |img: &RgbaImage, x: u32, y: u32| img.get_pixel(x, y).0[0] < 120;
    let clear = |img: &RgbaImage, x: u32, y: u32| img.get_pixel(x, y).0[0] > 245;

    let grown = render_shadow(circle, 20.0, Vec2::ZERO, 0.0, 6.0, false);
    assert!(dark(&grown, 127, 107), "24.7 px out on the diagonal");
    assert!(clear(&grown, 129, 109), "27.6 px out on the diagonal");

    let shrunk = render_shadow(circle, 20.0, Vec2::ZERO, 0.0, -6.0, false);
    assert!(dark(&shrunk, 110, 103), "13.5 px below the centre");
    assert!(clear(&shrunk, 110, 105), "15.5 px below the centre");
    assert!(clear(&shrunk, 120, 100), "14.1 px out on the diagonal");

    let sharp = render_shadow(circle, 0.0, Vec2::ZERO, 0.0, 6.0, false);
    assert!(dark(&sharp, 84, 64), "the sharp shadow's corner pixel");
}
