//! A panel's two packed inter-child gaps.

use crate::layout::types::limits::valid_packed_gap;
use half::f16;
use std::hash::Hash;

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Gaps([u16; 2]);

impl std::fmt::Debug for Gaps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gaps")
            .field("gap", &self.gap())
            .field("line_gap", &self.line_gap())
            .finish()
    }
}

impl Hash for Gaps {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u32(self.resolved());
    }
}

impl Gaps {
    pub(crate) const ZERO: Self = Self([0; 2]);

    /// The "caller never set this" bit pattern: an f16 quiet NaN.
    ///
    /// A gap is finite and non-negative ([`valid_packed_gap`]), so no
    /// value a caller can store lands here — which makes NaN free to
    /// carry the unset flag without widening the packed pair. Both
    /// readers below fold it back to `0.0` via `f32::max`, which returns
    /// the non-NaN operand and so costs a `maxss`, not a branch.
    const UNSET: u16 = 0x7E00;

    /// A pair with neither axis set. [`Node`]'s starting value, so a
    /// widget can tell an untouched gap from a caller's explicit `0.0`
    /// and fill in a themed default only for the former.
    pub(crate) const UNSET_PAIR: Self = Self([Self::UNSET; 2]);

    #[inline]
    pub(crate) fn gap(self) -> f32 {
        f16::from_bits(self.0[0]).to_f32().max(0.0)
    }

    #[inline]
    pub(crate) fn line_gap(self) -> f32 {
        f16::from_bits(self.0[1]).to_f32().max(0.0)
    }

    /// Both lanes as one `u32`, with unset axes folded to `0.0` — what
    /// layout actually sees. Equality and hashing downstream go through
    /// this, so an untouched gap and an explicit `0.0` can't split a
    /// cache key or an extras row when they render identically.
    ///
    /// Shifted rather than byte-cast so the key is the same number on
    /// either endianness; it never leaves the process, but a
    /// layout-dependent hash is a trap worth not setting.
    #[inline]
    pub(crate) fn resolved(self) -> u32 {
        let gap = if self.gap_is_set() { self.0[0] } else { 0 };
        let line_gap = if self.line_gap_is_set() { self.0[1] } else { 0 };
        gap as u32 | ((line_gap as u32) << 16)
    }

    #[inline]
    pub(crate) fn gap_is_set(self) -> bool {
        self.0[0] != Self::UNSET
    }

    #[inline]
    pub(crate) fn line_gap_is_set(self) -> bool {
        self.0[1] != Self::UNSET
    }

    #[inline]
    pub(crate) fn set_gap(&mut self, v: f32) {
        debug_assert!(
            valid_packed_gap(v),
            "gap must be finite, non-negative, and no greater than the f16 maximum, got {v}",
        );
        self.0[0] = f16::from_f32(v).to_bits();
    }

    #[inline]
    pub(crate) fn set_line_gap(&mut self, v: f32) {
        debug_assert!(
            valid_packed_gap(v),
            "line gap must be finite, non-negative, and no greater than the f16 maximum, got {v}",
        );
        self.0[1] = f16::from_f32(v).to_bits();
    }
}
