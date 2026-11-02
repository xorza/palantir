//! The caps a curve instance draws, as the shader reads them.

use crate::shape::style::LineCap;
use bytemuck::{Pod, Zeroable};

/// The ends of a stroke a curve instance draws, each a bit of the shader's word: a polyline
/// segment's joint ends take no cap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CapEnds {
    Neither = 0,
    Start = 1,
    End = 2,
    Both = 3,
}

impl CapEnds {
    /// The stroke ends segment `idx` of `count` reaches.
    pub(crate) const fn of_segment(idx: usize, count: usize) -> Self {
        match (idx == 0, idx + 1 == count) {
            (false, false) => Self::Neither,
            (true, false) => Self::Start,
            (false, true) => Self::End,
            (true, true) => Self::Both,
        }
    }
}

/// One stroke's cap and which ends of this instance get it.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
pub(crate) struct CurveCaps(u32);

impl CurveCaps {
    /// The bits holding the [`LineCap`] discriminant.
    pub(crate) const CAP_MASK: u32 = 0xFF;
    /// Set when the instance starts the stroke.
    pub(crate) const AT_START: u32 = (CapEnds::Start as u32) << 8;
    /// Set when the instance ends the stroke.
    pub(crate) const AT_END: u32 = (CapEnds::End as u32) << 8;

    pub(crate) const fn new(cap: LineCap, ends: CapEnds) -> Self {
        Self(cap as u32 | ((ends as u32) << 8))
    }
}

const _: () = assert!(LineCap::Round as u32 <= CurveCaps::CAP_MASK);
const _: () = assert!(CurveCaps::CAP_MASK & (CurveCaps::AT_START | CurveCaps::AT_END) == 0);

#[cfg(test)]
mod tests {
    use crate::renderer::render_buffer::curve_caps::{CapEnds, CurveCaps};
    use crate::shape::style::LineCap;

    /// The word as the shader masks it: cap in the low bits, bit 8 start, bit 9 end.
    #[test]
    fn caps_pack_one_cap_and_its_ends() {
        assert_eq!(
            CurveCaps::new(LineCap::Round, CapEnds::Start).0,
            2 | (1 << 8)
        );
        assert_eq!(
            CurveCaps::new(LineCap::Square, CapEnds::End).0,
            1 | (1 << 9)
        );
        assert_eq!(
            CurveCaps::new(LineCap::Round, CapEnds::Both).0,
            2 | (3 << 8)
        );
        assert_eq!(CurveCaps::new(LineCap::Butt, CapEnds::Neither).0, 0);
        // Segments 0, 1 and 2 of 3, and the one segment of 1.
        assert_eq!(
            [0, 1, 2].map(|idx| CapEnds::of_segment(idx, 3)),
            [CapEnds::Start, CapEnds::Neither, CapEnds::End]
        );
        assert_eq!(CapEnds::of_segment(0, 1), CapEnds::Both);
    }
}
