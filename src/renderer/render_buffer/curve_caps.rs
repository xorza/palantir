//! The caps a curve instance draws, as the shader reads them.

use crate::shape::style::LineCap;
use bytemuck::{Pod, Zeroable};

/// One stroke's cap and the ends of this instance that get it.
///
/// A stroke has one cap; what varies per instance is which of its ends
/// are the stroke's true ends — a polyline segment's joint ends take
/// none. So the word holds one [`LineCap`] and two end bits, and a pair
/// of different caps cannot be spelled. `repr(transparent)` over the `u32`
/// vertex attribute the shader reads, which takes every number here as a
/// substituted constant.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
pub(crate) struct CurveCaps(u32);

impl CurveCaps {
    /// The bits holding the [`LineCap`] discriminant.
    pub(crate) const CAP_MASK: u32 = 0xFF;
    /// Set when the instance's start is the stroke's start.
    pub(crate) const AT_START: u32 = 1 << 8;
    /// Set when the instance's end is the stroke's end.
    pub(crate) const AT_END: u32 = 1 << 9;

    /// `cap` on the ends the flags name.
    pub(crate) const fn new(cap: LineCap, at_start: bool, at_end: bool) -> Self {
        let mut bits = cap as u32;
        if at_start {
            bits |= Self::AT_START;
        }
        if at_end {
            bits |= Self::AT_END;
        }
        Self(bits)
    }
}

const _: () = assert!(LineCap::Round as u32 <= CurveCaps::CAP_MASK);
const _: () = assert!(CurveCaps::CAP_MASK & (CurveCaps::AT_START | CurveCaps::AT_END) == 0);

#[cfg(test)]
mod tests {
    use crate::renderer::render_buffer::curve_caps::CurveCaps;
    use crate::shape::style::LineCap;

    /// The word as the shader masks it: the cap discriminant (Round = 2)
    /// low, then bit 8 for the start and bit 9 for the end.
    #[test]
    fn caps_pack_one_cap_and_its_ends() {
        assert_eq!(CurveCaps::new(LineCap::Round, true, false).0, 2 | 1 << 8);
        assert_eq!(CurveCaps::new(LineCap::Square, false, true).0, 1 | 1 << 9);
        assert_eq!(CurveCaps::new(LineCap::Round, true, true).0, 2 | 3 << 8);
        assert_eq!(CurveCaps::new(LineCap::Butt, false, false).0, 0);
    }
}
