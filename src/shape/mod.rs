use crate::primitives::{Color, Corners, Rect, Stroke};
use glam::Vec2;

/// Where a shape sits inside its owner Node.
/// Resolved against `Node.rect` at paint time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShapeRect {
    /// Fill the owner's full arranged rect.
    Full,
    /// Offset relative to the owner's `rect.min`.
    Offset(Rect),
}

#[derive(Clone, Debug)]
pub enum Shape {
    RoundedRect {
        bounds: ShapeRect,
        radius: Corners,
        fill: Color,
        stroke: Option<Stroke>,
    },
    Line {
        a: Vec2,
        b: Vec2,
        width: f32,
        color: Color,
    },
    /// Placeholder until glyphon is wired up. `measured` is the pre-shaped run size
    /// so layout can ask for it.
    Text {
        offset: Vec2,
        text: String,
        color: Color,
        measured: crate::primitives::Size,
    },
}
