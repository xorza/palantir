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
    /// Horizontal alignment inside the parent's inner rect. Read by `VStack`
    /// (cross axis) and `ZStack` (both axes); ignored by `HStack` (main axis)
    /// and `Canvas` (absolute placement).
    pub align_x: Align,
    /// Vertical alignment inside the parent's inner rect. Read by `HStack`
    /// (cross axis) and `ZStack` (both axes); ignored by `VStack` (main axis)
    /// and `Canvas` (absolute placement).
    pub align_y: Align,
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
            align_x: Align::Auto,
            align_y: Align::Auto,
            position: None,
        }
    }
}
