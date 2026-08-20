//! The rect a shape reports as painted, where it is not the owner's.

use crate::primitives::color::Color;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::scene::shapes::paint::shadow_paint_rect_local;
use crate::scene::shapes::record::*;
use glam::Vec2;

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
            expected: Rect::new(8.0, 13.0, 58.0, 68.0),
        },
        DropCase {
            offset: Vec2::new(-9.0, -11.0),
            blur: 3.0,
            spread: 5.0,
            expected: Rect::new(-13.0, -5.0, 58.0, 68.0),
        },
        DropCase {
            offset: Vec2::new(4.0, -3.0),
            blur: 2.0,
            spread: -5.0,
            expected: Rect::new(8.0, 11.0, 42.0, 52.0),
        },
    ];

    for case in cases {
        assert_eq!(
            shadow_paint_rect_local(
                Some(source),
                Size::ZERO,
                case.offset,
                case.blur,
                case.spread,
                false,
            ),
            case.expected,
            "{case:?}",
        );
    }

    assert_eq!(
        shadow_paint_rect_local(
            Some(source),
            Size::ZERO,
            Vec2::new(100.0, -100.0),
            20.0,
            8.0,
            true,
        ),
        source,
        "inset paint remains clipped to its source rect",
    );
}

/// A mesh whose vertex hull overflows its owner box (a rotated / scaled
/// glyph) must report that hull as its paint bbox. Returning the owner
/// rect instead makes partial damage too small — the overflow paints with
/// cut vertices and leaves leftover pixels when it changes. Regression for
/// the subscription-glyph triangle.
#[test]
fn mesh_paint_bbox_is_vertex_hull_not_owner_rect() {
    let owner = Size::new(13.0, 13.0);
    // Hull reaches left/up past the owner origin and right/down past its
    // size — i.e. paints outside the owner box on every side.
    let hull = Rect {
        min: Vec2::new(-5.0, -4.0),
        size: Size::new(25.0, 24.0),
    };
    let mesh = |local_rect| ShapeRecord::Mesh {
        local_rect,
        tint: ColorF16::from(Color::WHITE),
        vertices: Span::new(0, 3),
        indices: Span::new(0, 3),
        bbox: hull,
        content_hash: 0,
    };

    assert_eq!(
        mesh(None).bbox_local(owner),
        hull,
        "the paint bbox is the vertex hull, not the owner rect"
    );

    // `local_rect` translates the hull (its size still comes from the
    // vertices, not `local_rect.size`).
    let offset = Rect {
        min: Vec2::new(2.0, 3.0),
        size: Size::new(99.0, 99.0),
    };
    assert_eq!(
        mesh(Some(offset)).bbox_local(owner),
        Rect {
            min: hull.min + offset.min,
            size: hull.size,
        },
        "local_rect offsets the hull; the size is unchanged"
    );
}
