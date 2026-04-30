use crate::primitives::{Size, Sizes, Spacing};
use glam::Vec2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub size: Sizes,
    pub min_size: Size,
    pub max_size: Size,
    pub padding: Spacing,
    pub margin: Spacing,
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
            position: None,
        }
    }
}
