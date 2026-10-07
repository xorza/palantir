//! Observability for the gradient LUT atlas, built on [`BenchOnly`]. A row id
//! does not say how [`CpuGradientAtlas::register`] reached it (index hit,
//! free-row bake, eviction bake, doubling, magenta fallback), which cost very
//! differently. Counters accumulate rather than reset, as the atlas is shared
//! across windows with no single pass to scope a reset to.
//!
//! [`BenchOnly`]: crate::common::counters::BenchOnly
//! [`CpuGradientAtlas::register`]:
//!     crate::renderer::gradient_atlas::CpuGradientAtlas::register

use crate::common::counters::counter_snapshot;

counter_snapshot! {
    cells BenchOnly, reads cfg(any(test, feature = "bench"));

    pub(super) struct GradientAtlasCounters;

    /// One reading; subtract two for a span's activity.
    pub(super) struct GradientAtlasCounts;

    /// `register` calls, however they resolved.
    registrations: u32,
    /// Calls answered from the index without a bake.
    hits: u32,
    /// Rows baked (free-row claims and evictions): one per miss.
    bakes: u32,
    /// Bakes that displaced a resident gradient, which re-bakes if it returns.
    evictions: u32,
    /// Capacity doublings, a one-way ratchet.
    growths: u32,
    /// LUT rows handed to the GPU, summed over flushes. A flush uploads the
    /// whole `min..=max` span, so compare with [`Self::bakes`].
    rows_uploaded: u32,
    /// Registrations that resolved to
    /// [`LutRow::FALLBACK`](crate::primitives::paint::lut_row::LutRow): table full and capped.
    fallbacks: u32,
}

impl GradientAtlasCounters {
    /// Records one baked row; `evicted` says whether it displaced a gradient.
    #[inline]
    pub(super) fn bake(&mut self, evicted: bool) {
        self.bakes.bump();
        if evicted {
            self.evictions.bump();
        }
    }
}
