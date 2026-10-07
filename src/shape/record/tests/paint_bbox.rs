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
        LoweredShadow::from(Shadow {
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

/// A mesh whose vertex hull overflows its owner box reports the hull as its paint bbox; the owner rect would under-size partial damage (regression: the subscription-glyph triangle).
#[test]
fn mesh_paint_bbox_is_vertex_hull_not_owner_rect() {
    let hull = Rect {
        min: Vec2::new(-5.0, -4.0),
        size: Size::new(25.0, 24.0),
    };
    assert_eq!(
        mesh_paint_bbox_local(hull, None),
        hull,
        "the paint bbox is the vertex hull, not the owner rect"
    );

    let offset = Rect {
        min: Vec2::new(2.0, 3.0),
        size: Size::new(99.0, 99.0),
    };
    assert_eq!(
        mesh_paint_bbox_local(hull, Some(offset)),
        Rect {
            min: hull.min + offset.min,
            size: hull.size,
        },
        "local_rect offsets the hull; the size is unchanged"
    );
}
