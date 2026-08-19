//! The fixed above-text replay order every non-text draw kind sorts into.

/// Above-text replay tiers in the backend's fixed intra-group order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PaintTier {
    Mesh,
    Image,
    /// Above `Image`, so an icon drawn over an image backdrop lands on top of
    /// it without forcing a group flush — the common toolbar-button shape.
    Icon,
    Curve,
}

impl PaintTier {
    /// Every tier in paint order, which is also `Ord` order.
    ///
    /// **The single source of the replay sequence.** The composer's
    /// group-flush arbitration (`HigherKindRects::conflicts`) is sound
    /// only while the backend replays tiers in this order, so every
    /// consumer that walks all tiers iterates this rather than spelling
    /// the order out: the drain block, the cursor struct, the emptiness
    /// test, and the stale-cursor advance. With
    /// [`RenderBuffer::batches`](crate::renderer::render_buffer::RenderBuffer)
    /// sized by [`Self::COUNT`], adding a tier is a variant plus the arms
    /// the compiler names.
    pub(crate) const ALL: [Self; Self::COUNT] = [Self::Mesh, Self::Image, Self::Icon, Self::Curve];

    pub(crate) const COUNT: usize = 4;

    #[inline]
    pub(crate) fn idx(self) -> usize {
        self as usize
    }
}

// `ALL` must stay in ascending `Ord` order: `conflicts` compares tiers
// with `<`, so a table out of step with the derive would silently
// reorder which tier paints on top.
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
