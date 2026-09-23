//! Which axes a scroll viewport pans, which of those size to content, and
//! the child driver that choice implies.

use crate::layout::axis::Axis;
use glam::BVec2;

/// Which driver lays a scroll viewport's children out. Derived from the
/// pan axes by [`ScrollAxes::child_layout`], so measure, arrange, and the
/// intrinsic query cannot pick different ones. Spelling the three-way
/// choice out at each of the three sites is what lets the intrinsic copy
/// drift from the other two.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScrollChildLayout {
    /// Both axes pan, so neither constrains the other and children stack
    /// at the origin.
    Layered,
    /// One axis pans; children flow along it.
    Flow(Axis),
}

/// Which axes a [`Widget::scroll`](crate::widget::Widget::scroll) viewport
/// pans, and which of those size to its content.
///
/// A panned axis measures the children unbounded, so they report their
/// full extent, and the viewport takes its own extent on that axis from
/// its `Sizing` rather than from what it scrolls over. One panned axis
/// flows the children along it, as a stack does. Two stack them at the
/// origin, as a z-stack does.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ScrollAxes(u16);

impl ScrollAxes {
    const PAN_X: u16 = 0b0001;
    const PAN_Y: u16 = 0b0010;
    const FIT_X: u16 = 0b0100;
    const FIT_Y: u16 = 0b1000;

    /// Pans left and right. The children flow along X.
    pub(crate) const HORIZONTAL: Self = Self(Self::PAN_X);
    /// Pans up and down. The children flow along Y.
    pub(crate) const VERTICAL: Self = Self(Self::PAN_Y);
    /// Pans on both axes. The children stack at the origin.
    pub(crate) const BOTH: Self = Self(Self::PAN_X | Self::PAN_Y);

    /// Size to content on each panned axis flagged here, the way a `Hug`
    /// [`Scroll`](crate::Scroll) does: the viewport reports its content's
    /// extent on that axis, bounded by its `max_size` and by the space its
    /// parent has, and pans past that cap.
    ///
    /// Only the reported size changes. The minimum size on a panned axis
    /// stays zero whatever the flag says, because shrinking below the
    /// content is what a viewport is for.
    ///
    /// Replaces the flags of an earlier call. An axis that does not pan
    /// already reports its content, so a flag there changes nothing and
    /// is dropped.
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

    /// The whole word, for the packed layout mode that carries it.
    #[inline]
    pub(crate) const fn to_bits(self) -> u16 {
        self.0
    }

    /// The inverse of [`Self::to_bits`].
    #[inline]
    pub(crate) const fn from_bits(bits: u16) -> Self {
        Self(bits)
    }

    /// The pan flags as stored, for a hash that must not see the fit
    /// bits. The one place these two flags' bit positions are written
    /// down, so a consumer folding them cannot invent a second layout.
    #[inline]
    pub(crate) const fn pan_bits(self) -> u16 {
        self.0 & (Self::PAN_X | Self::PAN_Y)
    }

    #[inline]
    pub(crate) fn pan_mask(self) -> BVec2 {
        BVec2::new(self.pans(Axis::X), self.pans(Axis::Y))
    }

    /// Which axes fold their measured content into the viewport's own
    /// reported size, as a lane mask — see [`Self::contributes`].
    #[inline]
    pub(crate) fn contributes_mask(self) -> BVec2 {
        BVec2::new(self.contributes(Axis::X), self.contributes(Axis::Y))
    }

    /// The driver that lays this viewport's children out.
    #[inline]
    pub(crate) fn child_layout(self) -> ScrollChildLayout {
        match (self.pans(Axis::X), self.pans(Axis::Y)) {
            (true, true) => ScrollChildLayout::Layered,
            (false, true) => ScrollChildLayout::Flow(Axis::Y),
            // An x-only pan flows along X, and so does the degenerate
            // no-pan value no constructor can produce.
            _ => ScrollChildLayout::Flow(Axis::X),
        }
    }

    /// Whether `axis` folds its measured content extent into the size the
    /// viewport reports for itself.
    ///
    /// A panned axis normally reports nothing — the viewport takes that
    /// axis from its own `Sizing`, not from what it scrolls over — but
    /// [`Self::fit_content`] opts back in, which is how a `Hug` scroll
    /// sizes to content.
    ///
    /// This is the **max**-content rule, and only that. A panned axis'
    /// *min*-content stays zero whatever the fit flag says:
    /// `resolve_sizing` floors a node's own size with its min-content
    /// intrinsic, and shrinking below the content is precisely what
    /// scrolling is for — floor a `Hug` scroll at its content and it pins
    /// itself open, ignoring both `max_size` and the space its parent
    /// actually has.
    #[inline]
    pub(crate) fn contributes(self, axis: Axis) -> bool {
        !self.pans(axis) || self.fits(axis)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fit flag lands only on an axis that pans, replaces the flags of
    /// an earlier call, and never changes which axes pan.
    #[test]
    fn fit_content_flags_only_the_panned_axes() {
        // (axes, fit x, fit y, contributes x, contributes y)
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
