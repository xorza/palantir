use crate::primitives::{Sizes, Spacing};

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Style {
    pub size: Sizes,
    pub padding: Spacing,
    pub margin: Spacing,
}
