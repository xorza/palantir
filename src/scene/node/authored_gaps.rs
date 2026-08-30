//! A panel's two inter-child gaps while a builder still owns them.

use crate::layout::types::limits::valid_packed_gap;
use crate::scene::node::gaps::Gaps;
use half::f16;

/// The authoring half of [`Gaps`]: each lane is either a caller's value
/// or still untouched, so a widget can lay a themed default under user
/// intent the way the six `Option` fields beside it on
/// [`Node`](crate::scene::node::Node) do.
///
/// The two readings are `Option<f32>`, and the unset state has no
/// spelling outside this file — [`Self::resolve`] is the only way out,
/// and what it produces is a plain [`Gaps`].
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct AuthoredGaps([u16; 2]);

impl std::fmt::Debug for AuthoredGaps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthoredGaps")
            .field("gap", &self.gap())
            .field("line_gap", &self.line_gap())
            .finish()
    }
}

impl AuthoredGaps {
    /// The bit pattern no lane can otherwise hold: an f16 quiet NaN.
    ///
    /// A gap is finite and non-negative ([`valid_packed_gap`]), so no
    /// value a caller can store lands here. That makes NaN free to carry
    /// the untouched state without widening the packed pair.
    const UNSET: u16 = 0x7E00;

    /// A pair with neither lane set — every `Node` starts here.
    pub(crate) const UNSET_PAIR: Self = Self([Self::UNSET; 2]);

    #[inline]
    pub(crate) fn gap(self) -> Option<f32> {
        Self::lane(self.0[0])
    }

    #[inline]
    pub(crate) fn line_gap(self) -> Option<f32> {
        Self::lane(self.0[1])
    }

    /// What layout reads: an untouched lane becomes `0.0`, an explicit
    /// one keeps the value the caller gave.
    #[inline]
    pub(crate) fn resolve(self) -> Gaps {
        Gaps::new(
            self.gap().unwrap_or_default(),
            self.line_gap().unwrap_or_default(),
        )
    }

    /// # Panics
    ///
    /// Panics unless `v` is finite, non-negative, and within the f16 range
    /// this packs into.
    #[inline]
    pub(crate) fn set_gap(&mut self, v: f32) {
        assert!(
            valid_packed_gap(v),
            "gap must be finite, non-negative, and no greater than the f16 maximum, got {v}",
        );
        self.0[0] = f16::from_f32(v).to_bits();
    }

    /// # Panics
    ///
    /// Panics on the same range [`Self::set_gap`] does.
    #[inline]
    pub(crate) fn set_line_gap(&mut self, v: f32) {
        assert!(
            valid_packed_gap(v),
            "line gap must be finite, non-negative, and no greater than the f16 maximum, got {v}",
        );
        self.0[1] = f16::from_f32(v).to_bits();
    }

    #[inline]
    fn lane(bits: u16) -> Option<f32> {
        (bits != Self::UNSET).then(|| f16::from_bits(bits).to_f32())
    }
}
