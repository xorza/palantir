//! The reel's cursor: an arrow drawn in the pointer layer, white with a dark rim.

use palantir::glam::Vec2;
use palantir::widget::Shape;
use palantir::{Border, RgbaF32, Ui};

/// The arrow's outline from its tip, in logical px: a 12 × 19 px pointer, the size of a
/// desktop's.
const OUTLINE: [Vec2; 7] = [
    Vec2::new(0.0, 0.0),
    Vec2::new(0.0, 16.0),
    Vec2::new(4.2, 12.4),
    Vec2::new(7.2, 19.0),
    Vec2::new(9.6, 18.0),
    Vec2::new(6.8, 11.6),
    Vec2::new(12.0, 11.6),
];

/// The arrow's fill as triangles over [`OUTLINE`]'s points: the head as a fan from the tip, the
/// tail as two.
const FILL: [[usize; 3]; 5] = [[0, 1, 2], [0, 2, 5], [0, 5, 6], [2, 3, 4], [2, 4, 5]];

/// The arrow with its tip at `at`.
pub(crate) fn draw(ui: &mut Ui, at: Vec2) {
    let point = |i: usize| at + OUTLINE[i];
    for [a, b, c] in FILL {
        ui.add_shape(Shape::triangle(point(a), point(b), point(c)).fill(RgbaF32::WHITE));
    }
    let rim: [Vec2; 8] = std::array::from_fn(|i| at + OUTLINE[i % OUTLINE.len()]);
    ui.add_shape(Shape::polyline(
        &rim,
        Border::new(RgbaF32::hex(0x111111), 1.4),
    ));
}
