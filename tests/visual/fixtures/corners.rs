//! Corner radii larger than their box holds.

use glam::UVec2;
use palantir::widget::Shape;
use palantir::{Background, Block, Configure, Corners, Panel, Rect, RgbaF32, Sizing};

use crate::harness::Harness;

/// Radii past half the shorter side shrink until adjacent corners meet,
/// the CSS "overlapping curves" rule: on a 100×40 box both `corners(30)`
/// and `corners(9999)` become 20, a pill exactly 100 px wide. Unfitted,
/// 30 drew a shape 96.6 px wide and 9999 drew nothing.
///
/// A rounded clip's mask follows the same rule: a white block under a
/// `corners(9999)` clip shows as the same pill.
///
/// Each pill sits at x 10..110. Pixel (11, y) has its centre 18.5 px from
/// the left arc's centre at (30, y + 0.5) on the mid-line: inside.
#[test]
fn oversized_radii_fit_their_box() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(120, 160), 1.0, RgbaF32::BLACK, |ui| {
        Panel::canvas()
            .id_salt("radii")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for (y, r) in [(10.0, 30.0), (60.0, 9999.0)] {
                    ui.add_shape(
                        Shape::rect(Rect::new(10.0, y, 100.0, 40.0))
                            .fill(RgbaF32::WHITE)
                            .corners(r),
                    );
                }
                Panel::zstack()
                    .id_salt("clip")
                    .position((10.0, 110.0))
                    .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                    .background(Background {
                        corners: Corners::all(9999.0),
                        ..Default::default()
                    })
                    .clip_rounded()
                    .show(ui, |ui| {
                        Block::new()
                            .id_salt("clipped")
                            .size((Sizing::FILL, Sizing::FILL))
                            .background(Background::fill(RgbaF32::WHITE))
                            .show(ui);
                    });
            });
    });
    let lit = |x: u32, y: u32| img.get_pixel(x, y).0[0] > 200;
    let dark = |x: u32, y: u32| img.get_pixel(x, y).0[0] < 40;
    for (label, mid) in [
        ("corners(30)", 29),
        ("corners(9999)", 79),
        ("clip corners(9999)", 129),
    ] {
        assert!(lit(11, mid), "{label}: the pill's left end");
        assert!(lit(108, mid), "{label}: the pill's right end");
        assert!(lit(60, mid), "{label}: the middle");
        assert!(dark(11, mid - 18), "{label}: outside the top-left arc");
    }
}
