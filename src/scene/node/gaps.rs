//! A panel's two inter-child gaps, as layout reads them.

use half::f16;
use std::fmt;

/// The within-line and between-line spacing of one panel, packed as two f16 lanes, each finite and non-negative (an unset gap is folded to `0.0` by [`AuthoredGaps::resolve`](crate::scene::node::authored_gaps::AuthoredGaps::resolve)), so the bit pattern is the identity for `Eq`/`Hash` in cascade keys and [`PanelExtras`](crate::scene::node::panel_extras::PanelExtras) rows.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Gaps([u16; 2]);

impl fmt::Debug for Gaps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Gaps")
            .field("gap", &self.gap())
            .field("line_gap", &self.line_gap())
            .finish()
    }
}

impl Gaps {
    pub(crate) const ZERO: Self = Self([0; 2]);

    #[inline]
    pub(crate) fn new(gap: f32, line_gap: f32) -> Self {
        Self([
            f16::from_f32(gap).to_bits(),
            f16::from_f32(line_gap).to_bits(),
        ])
    }

    #[inline]
    pub(crate) const fn as_u32(self) -> u32 {
        self.0[0] as u32 | ((self.0[1] as u32) << 16)
    }

    #[inline]
    pub(crate) fn gap(self) -> f32 {
        f16::from_bits(self.0[0]).to_f32()
    }

    #[inline]
    pub(crate) fn line_gap(self) -> f32 {
        f16::from_bits(self.0[1]).to_f32()
    }
}
