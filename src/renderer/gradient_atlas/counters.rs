//! Observability for the gradient LUT atlas. Built on [`BenchOnly`], whose
//! module doc explains the gated-cell pattern and why the two gates exist.
//!
//! On the wider cell gate rather than test-only because
//! [`gradient_atlas`](crate::renderer::gradient_atlas::bench) benches
//! the register path and asserts each arm actually exercised what its
//! name claims — a "steady-state hit" arm that quietly started baking
//! would otherwise read as a plausible slowdown rather than a broken
//! fixture. Every counter is a plain `u32`; nothing here allocates, so
//! the alloc bench sees nothing from this module.
//!
//! ## Why these seven
//!
//! Every arm of [`CpuGradientAtlas::register_stops`] ends in a
//! `LutRow`, and from the outside a row id says nothing about how it
//! was reached. Resolving from the index, baking into a free row,
//! baking over an evicted one, doubling the table, and giving up to the
//! magenta fallback are five materially different costs — plus the
//! registration total they are all fractions of, and the row count the
//! resulting upload actually moved. Left indistinguishable, they are
//! exactly how an O(capacity) probe walk sits in the hot path
//! unnoticed.
//!
//! They accumulate for the life of the atlas rather than resetting per
//! frame: the atlas is shared across windows and `flush` is a
//! per-submit boundary, so there is no single "pass" to scope them to.
//! Readers take a delta, the same call
//! [`CascadeCounters`](crate::scene::cascade::counters::CascadeCounters) makes.
//!
//! [`BenchOnly`]: crate::common::counters::BenchOnly
//! [`CpuGradientAtlas::register_stops`]:
//!     crate::renderer::gradient_atlas::CpuGradientAtlas::register_stops

use crate::common::counters::counter_snapshot;

counter_snapshot! {
    cells BenchOnly, reads cfg(any(test, feature = "bench"));

    /// What the atlas did, for tests and benches to assert against.
    pub(super) struct GradientAtlasCounters;

    /// One reading of a [`GradientAtlasCounters`]. Subtract two to get
    /// what a span of registrations did.
    pub(super) struct GradientAtlasCounts;

    /// `register_stops` calls, however they resolved.
    registrations: u32,
    /// Calls answered straight from the index — no bake. The
    /// steady-state metric: a frame redrawing unchanged gradients must
    /// be all hits.
    hits: u32,
    /// Rows baked, free-row claims and evictions together. One per
    /// miss, always — more than one would mean the register path is
    /// re-baking content it should have resolved.
    bakes: u32,
    /// Bakes that displaced a resident gradient rather than taking a
    /// never-claimed row. Separated from [`Self::bakes`] because
    /// eviction is what costs a *later* miss: the displaced gradient
    /// re-bakes when it comes back.
    evictions: u32,
    /// Capacity doublings. The one-way ratchet — a test that wants to
    /// prove the atlas held its size needs to see this stay flat.
    growths: u32,
    /// LUT rows handed to the GPU, summed over every flush.
    ///
    /// The number [`Self::bakes`] has to be read against. A flush
    /// uploads the inclusive `min..=max` span of the rows that changed,
    /// because the dirty tracker is a pair of row ids and cannot say
    /// "these two" — so two rows re-baked at opposite ends of a grown
    /// table upload the whole table. Nothing else reports that, and the
    /// ratio of these two is what says whether a workload is paying it.
    rows_uploaded: u32,
    /// Registrations that resolved to
    /// [`LutRow::FALLBACK`](crate::primitives::lut_row::LutRow) because
    /// the table was full, every row was spoken for this epoch, and the
    /// row cap refused to grow.
    fallbacks: u32,
}

impl GradientAtlasCounters {
    /// Record one baked row. `evicted` says whether it was holding
    /// another gradient — a flag rather than two call sites, so the one
    /// caller cannot record a bake and forget the eviction that came
    /// with it. The other five are plain field bumps, as every other
    /// counter set in the crate does it.
    #[inline]
    pub(super) fn bake(&mut self, evicted: bool) {
        self.bakes.bump();
        if evicted {
            self.evictions.bump();
        }
    }
}
