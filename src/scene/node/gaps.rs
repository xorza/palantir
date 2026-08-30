//! A panel's two inter-child gaps, as layout reads them.

use half::f16;

/// The within-line and between-line spacing of one panel, packed as two
/// f16 lanes. Every lane is finite and non-negative: an authoring gap
/// the caller never set is folded to `0.0` by
/// [`AuthoredGaps::resolve`](crate::scene::node::authored_gaps::AuthoredGaps::resolve),
/// so nothing downstream carries an unset state or has to fold one.
///
/// That is what lets the bit pattern be the identity — the derived
/// `Eq`/`Hash` are what a cascade key and a
/// [`PanelExtras`](crate::scene::node::panel_extras::PanelExtras) row
/// compare on.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Gaps([u16; 2]);

impl std::fmt::Debug for Gaps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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

    /// Both lanes as one `u32`, low lane first.
    ///
    /// Shifted rather than byte-cast so the number is the same on either
    /// endianness. It never leaves the process, but a layout-dependent
    /// key is a trap worth not setting.
    #[inline]
    pub(crate) fn as_u32(self) -> u32 {
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
