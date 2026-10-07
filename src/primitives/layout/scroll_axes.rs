//! Which axes a scroll viewport pans, which size to content, and the resulting child driver.

use crate::primitives::layout::axis::Axis;
use glam::BVec2;

/// Which driver lays out a scroll viewport's children, derived from the pan axes so measure, arrange and intrinsics agree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScrollChildLayout {
    /// Both axes pan: children stack at the origin.
    Layered,
    /// One axis pans: children flow along it.
    Flow(Axis),
}

/// Which axes a [`Widget::scroll`](crate::widget::Widget::scroll) viewport pans
/// and which of those size to its content. A panned axis measures children
/// unbounded and takes its own extent from its `Sizing`.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ScrollAxes(u16);

impl ScrollAxes {
    const PAN_X: u16 = 0b0001;
    const PAN_Y: u16 = 0b0010;
    const FIT_X: u16 = 0b0100;
    const FIT_Y: u16 = 0b1000;

    /// Pans left and right; children flow along X.
    pub(crate) const HORIZONTAL: Self = Self(Self::PAN_X);
    /// Pans up and down; children flow along Y.
    pub(crate) const VERTICAL: Self = Self(Self::PAN_Y);
    /// Pans on both axes; children stack at the origin.
    pub(crate) const BOTH: Self = Self(Self::PAN_X | Self::PAN_Y);

    /// Sizes to content on each flagged panned axis, as a `Hug`
    /// [`Scroll`](crate::Scroll) does, bounded by `max_size` and the parent's
    /// space; min size stays zero. Replaces earlier flags; a flag on a
    /// non-panning axis is dropped.
    pub(crate) const fn fit_content(self, x: bool, y: bool) -> Self {
        let fit_x = if x && self.0 & Self::PAN_X != 0 {
            Self::FIT_X
        } else {
            0
        };
        let fit_y = if y && self.0 & Self::PAN_Y != 0 {
            Self::FIT_Y
        } else {
            0
        };
        Self(self.pan_bits() | fit_x | fit_y)
    }

    /// Whether the viewport pans along `axis`.
    pub(crate) const fn pans(self, axis: Axis) -> bool {
        let bit = match axis {
            Axis::X => Self::PAN_X,
            Axis::Y => Self::PAN_Y,
        };
        self.0 & bit != 0
    }

    const fn fits(self, axis: Axis) -> bool {
        let bit = match axis {
            Axis::X => Self::FIT_X,
            Axis::Y => Self::FIT_Y,
        };
        self.0 & bit != 0
    }

    #[inline]
    pub(crate) const fn to_bits(self) -> u16 {
        self.0
    }

    #[inline]
    pub(crate) const fn from_bits(bits: u16) -> Self {
        Self(bits)
    }

    /// The pan flags, for a hash that must not see the fit bits.
    #[inline]
    pub(crate) const fn pan_bits(self) -> u16 {
        self.0 & (Self::PAN_X | Self::PAN_Y)
    }

    #[inline]
    pub(crate) const fn pan_mask(self) -> BVec2 {
        BVec2::new(self.pans(Axis::X), self.pans(Axis::Y))
    }

    #[inline]
    pub(crate) const fn contributes_mask(self) -> BVec2 {
        BVec2::new(self.contributes(Axis::X), self.contributes(Axis::Y))
    }

    #[inline]
    pub(crate) const fn child_layout(self) -> ScrollChildLayout {
        match (self.pans(Axis::X), self.pans(Axis::Y)) {
            (true, true) => ScrollChildLayout::Layered,
            (false, true) => ScrollChildLayout::Flow(Axis::Y),
            _ => ScrollChildLayout::Flow(Axis::X),
        }
    }

    /// Whether `axis` folds its measured content into the reported size. This
    /// is the max-content rule only: min-content stays zero, or a `Hug` scroll
    /// would pin itself open past `max_size`.
    #[inline]
    pub(crate) const fn contributes(self, axis: Axis) -> bool {
        !self.pans(axis) || self.fits(axis)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_content_flags_only_the_panned_axes() {
        let cases = [
            (ScrollAxes::VERTICAL, false, false, true, false),
            (ScrollAxes::VERTICAL, false, true, true, true),
            (ScrollAxes::VERTICAL, true, false, true, false),
            (ScrollAxes::HORIZONTAL, true, true, true, true),
            (ScrollAxes::BOTH, true, false, true, false),
            (ScrollAxes::BOTH, false, false, false, false),
        ];
        for (axes, x, y, want_x, want_y) in cases {
            let fitted = axes.fit_content(x, y);
            assert_eq!(fitted.pan_bits(), axes.pan_bits(), "{axes:?} ({x}, {y})");
            assert_eq!(
                (fitted.contributes(Axis::X), fitted.contributes(Axis::Y)),
                (want_x, want_y),
                "{axes:?} ({x}, {y})",
            );
        }
        assert_eq!(
            ScrollAxes::VERTICAL.fit_content(true, false),
            ScrollAxes::VERTICAL,
            "a flag on an axis that does not pan is dropped, not stored",
        );
        assert_eq!(
            ScrollAxes::BOTH
                .fit_content(true, true)
                .fit_content(false, true),
            ScrollAxes::BOTH.fit_content(false, true),
        );
        let pans = |axes: ScrollAxes| (axes.pans(Axis::X), axes.pans(Axis::Y));
        assert_eq!(pans(ScrollAxes::HORIZONTAL), (true, false));
        assert_eq!(pans(ScrollAxes::VERTICAL), (false, true));
        assert_eq!(pans(ScrollAxes::BOTH), (true, true));
    }
}
