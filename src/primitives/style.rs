use crate::primitives::{Size, Sizes, Spacing};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub size: Sizes,
    pub min_size: Size,
    pub max_size: Size,
    pub padding: Spacing,
    pub margin: Spacing,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            size: Sizes::default(),
            min_size: Size::ZERO,
            max_size: Size::INF,
            padding: Spacing::ZERO,
            margin: Spacing::ZERO,
        }
    }
}
