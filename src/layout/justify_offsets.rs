//! Where a justified row starts, and what gap it uses.

use crate::primitives::layout::justify::Justify;

/// Main-axis offset and effective gap for one row of `justify`-distributed children; the single source for Stack and WrapStack.
#[derive(Debug)]
pub(super) struct JustifyOffsets {
    pub(super) start: f32,
    pub(super) gap: f32,
}

/// One row a justify distributes over: `count` children with `leftover` free space, at base `gap`.
#[derive(Clone, Copy, Debug)]
pub(super) struct JustifyRow {
    pub(super) leftover: f32,
    pub(super) gap: f32,
    pub(super) count: usize,
}

impl JustifyOffsets {
    /// The offsets `justify` asks for across `row`.
    pub(super) const fn new(justify: Justify, row: JustifyRow) -> Self {
        let JustifyRow {
            leftover,
            gap,
            count,
        } = row;
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
