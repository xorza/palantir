//! The rect a shape reports as painted, where it is not the owner's.

use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::shadow::Shadow;
use crate::shape::paint::lowered_shadow::LoweredShadow;
use crate::shape::record::*;

/// A drop shadow is its source moved by the offset and grown by the halo `4σ + max(spread, 0)`. Source (10, 20, 30, 40): offset (12, 7) grown 18 gives (4, 9, 66, 76); (−9, −11) grown 17 gives (−16, −8, 64, 74); (4, −3) grown 8 (negative spread adds nothing) gives (6, 9, 46, 56).
#[test]
fn shadow_paint_bbox_tracks_shifted_drop_and_source_bounded_inset() {
    #[derive(Debug)]
    struct DropCase {
        offset: Vec2,
        blur: f32,
        spread: f32,
        expected: Rect,
    }

    let source = Rect::new(10.0, 20.0, 30.0, 40.0);
    let cases = [
        DropCase {
            offset: Vec2::new(12.0, 7.0),
            blur: 4.0,
            spread: 2.0,
            expected: Rect::new(4.0, 9.0, 66.0, 76.0),
        },
        DropCase {
            offset: Vec2::new(-9.0, -11.0),
            blur: 3.0,
            spread: 5.0,
            expected: Rect::new(-16.0, -8.0, 64.0, 74.0),
        },
        DropCase {
            offset: Vec2::new(4.0, -3.0),
            blur: 2.0,
            spread: -5.0,
            expected: Rect::new(6.0, 9.0, 46.0, 56.0),
        },
    ];

    let lowered = |offset: Vec2, blur: f32, spread: f32, inset: bool| {
        LoweredShadow::lower(Shadow {
            color: RgbaF32::BLACK,
            offset,
            blur,
            spread,
            inset,
        })
    };

    for case in cases {
        assert_eq!(
            lowered(case.offset, case.blur, case.spread, false)
                .paint_rect_local(Some(source), Size::ZERO),
            case.expected,
            "{case:?}",
        );
    }

    assert_eq!(
        lowered(Vec2::new(100.0, -100.0), 20.0, 8.0, true)
            .paint_rect_local(Some(source), Size::ZERO),
        source,
        "inset paint remains clipped to its source rect",
    );
}

/// A mesh's paint bbox is its vertex hull fitted, which can overflow its owner box; the owner rect
/// would under-size partial damage (regression: the subscription-glyph triangle). `Natural` moves
/// the hull by its rect's corner and keeps its size; `Fill` maps it onto the rect.
#[test]
fn mesh_paint_bbox_is_the_fitted_vertex_hull() {
    let hull = Rect::new(-5.0, -4.0, 25.0, 24.0);
    let owner = Size::new(10.0, 10.0);
    let bbox = |placement: PointPlacement| placement.map(hull, owner).apply_rect(hull);
    assert_eq!(
        bbox(PointPlacement::NATURAL),
        hull,
        "the hull, not the owner rect"
    );
    assert_eq!(
        bbox(PointPlacement {
            local_rect: Some(Rect::new(2.0, 3.0, 99.0, 99.0)),
            fit: Fit::Natural,
        }),
        Rect::new(-3.0, -1.0, 25.0, 24.0),
    );
    assert_eq!(
        bbox(PointPlacement {
            local_rect: None,
            fit: Fit::Fill,
        }),
        Rect::new(0.0, 0.0, 10.0, 10.0),
    );
}
