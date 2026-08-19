//! Where a justified row starts, and what gap it uses.

use crate::layout::types::justify::Justify;

/// Main-axis offset + effective inter-child gap for one row of
/// `justify`-distributed children. Single source of truth for Stack and
/// WrapStack — keeps SpaceBetween / SpaceAround degeneracy rules
/// (count < 2 / count < 1) in one place.
#[derive(Debug)]
pub(super) struct JustifyOffsets {
    pub(super) start: f32,
    pub(super) gap: f32,
}

impl JustifyOffsets {
    /// The offsets `justify` asks for, given `leftover` free main-axis
    /// space across `count` children at a base `gap`.
    pub(super) fn new(justify: Justify, leftover: f32, gap: f32, count: usize) -> Self {
        match justify {
            Justify::Start => Self { start: 0.0, gap },
            Justify::Center => Self {
                start: leftover * 0.5,
                gap,
            },
            Justify::End => Self {
                start: leftover,
                gap,
            },
            Justify::SpaceBetween if count > 1 => Self {
                start: 0.0,
                gap: gap + leftover / (count - 1) as f32,
            },
            Justify::SpaceAround if count > 0 => {
                let extra = leftover / count as f32;
                Self {
                    start: extra * 0.5,
                    gap: gap + extra,
                }
            }
            // Fewer than 2 / 1 children → fallback to Start.
            Justify::SpaceBetween | Justify::SpaceAround => Self { start: 0.0, gap },
        }
    }
}
