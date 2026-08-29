//! The gradient identity an encode pass resolved a brush down to.

use crate::primitives::brush::gradient::FillAxis;
use crate::primitives::fill_kind::FillKind;
use crate::primitives::lut_row::LutRow;

/// Physical gradient identity resolved for this encode pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ResolvedGradient {
    pub(crate) axis: FillAxis,
    pub(crate) lut_row: LutRow,
    pub(crate) kind: FillKind,
}
