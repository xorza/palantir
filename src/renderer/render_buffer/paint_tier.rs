//! The fixed above-text replay order every non-text draw kind sorts into.

/// Above-text replay tiers in the backend's fixed intra-group order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PaintTier {
    Mesh,
    Image,
    /// Above `Image`, so an icon over an image backdrop lands on top without a group flush.
    Icon,
    Curve,
    /// App-written effects, last: rare, and each run binds a pipeline of its own.
    Effect,
}

impl PaintTier {
    /// Every tier in paint order (`Ord` order): the single source of the order a group lists its tiers in, which `HigherKindRects::conflicts` relies on.
    pub(crate) const ALL: [Self; Self::COUNT] = [
        Self::Mesh,
        Self::Image,
        Self::Icon,
        Self::Curve,
        Self::Effect,
    ];

    pub(crate) const COUNT: usize = 5;

    #[inline]
    pub(crate) const fn idx(self) -> usize {
        self as usize
    }
}

// `ALL` must stay in ascending `Ord` order: `conflicts` compares tiers with `<`.
const _: () = {
    let mut i = 1;
    while i < PaintTier::COUNT {
        assert!(
            PaintTier::ALL[i - 1] as u8 <= PaintTier::ALL[i] as u8,
            "PaintTier::ALL must be in ascending Ord order",
        );
        i += 1;
    }
};
