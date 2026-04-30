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
    /// Alignment of this node inside its parent's inner rect. Each axis is
    /// honored only by parent layout modes that own that axis as a cross or
    /// placement axis: HStack reads `align.v` (cross), VStack reads `align.h`
    /// (cross), ZStack reads both, HStack/VStack ignore their main axis,
    /// Canvas ignores both (absolute placement).
    pub align: Align,
    /// Default `align` applied to children when the child's own axis is
    /// `Auto`. Mirrors CSS `align-items` (parent) + `align-self` (child).
    /// Read by the same parents as `align`, on the same axes.
    pub child_align: Align,
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
            align: Align::default(),
            child_align: Align::default(),
            position: None,
        }
    }
}
