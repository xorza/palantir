//! The gradient identity an encode pass resolved a brush down to.

use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::lut_row::LutRow;

/// Physical gradient identity resolved for this encode pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ResolvedGradient {
    pub(crate) axis: FillAxis,
    pub(crate) lut_row: LutRow,
    pub(crate) kind: FillKind,
}
