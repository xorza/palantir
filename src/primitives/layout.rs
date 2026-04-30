use crate::primitives::{Align, Size, Sizes, Spacing};
use glam::Vec2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub size: Sizes,
    pub min_size: Size,
    pub max_size: Size,
    pub padding: Spacing,
    pub margin: Spacing,
    /// Logical-px space between children of `HStack`/`VStack`. Ignored by
    /// `Leaf` / `ZStack` / `Canvas`.
    pub gap: f32,
    /// Cross-axis alignment of this node when its parent is a stack.
    pub align: Align,
    /// Absolute position inside a `Canvas` parent (parent-inner coordinates).
    /// Ignored by other layout kinds.
    pub position: Option<Vec2>,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            size: Sizes::default(),
            min_size: Size::ZERO,
            max_size: Size::INF,
            padding: Spacing::ZERO,
            margin: Spacing::ZERO,
            gap: 0.0,
            align: Align::Auto,
            position: None,
        }
    }
}
