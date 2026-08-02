//! Observability for the gradient LUT atlas. Built on
//! [`BenchOnly`](crate::common::probe::BenchOnly), whose module doc
//! explains the gated-cell pattern and why the two gates exist.
//!
//! On the wider gate rather than test-only because
//! [`gradient_atlas`](crate::renderer::gradient_atlas::bench) benches
//! the register path and asserts each arm actually exercised what its
//! name claims — a "steady-state hit" arm that quietly started baking
//! would otherwise read as a plausible slowdown rather than a broken
//! fixture. Every counter is a plain `u32`; nothing here allocates, so
//! the `alloc_free` benches see nothing from this module.
//!
//! ## Why these five
//!
//! Every arm of [`CpuGradientAtlas::register_stops`] ends in a
//! `LutRow`, and from the outside a row id says nothing about how it
//! was reached. Resolving from the index, baking into a free row,
//! baking over an evicted one, doubling the table, and giving up to the
//! magenta fallback are five materially different costs that were
//! previously indistinguishable — which is exactly how an O(capacity)
//! probe walk sat in the hot path unnoticed.
//!
//! They accumulate for the life of the atlas rather than resetting per
//! frame: the atlas is shared across windows and `flush` is a
//! per-submit boundary, so there is no single "pass" to scope them to.
//! Readers take a delta, the same call
//! [`CascadeProbe`](crate::scene::cascade::probe::CascadeProbe) makes.
//!
//! [`CpuGradientAtlas::register_stops`]:
//!     crate::renderer::gradient_atlas::CpuGradientAtlas::register_stops

use crate::common::probe::BenchOnly;

/// What the atlas did, for tests and benches to assert against.
#[derive(Debug, Default)]
pub(super) struct GradientAtlasProbe {
    /// `register_stops` calls, however they resolved.
    registrations: BenchOnly<u32>,
    /// Calls answered straight from the index — no bake. The
    /// steady-state metric: a frame redrawing unchanged gradients must
    /// be all hits.
    hits: BenchOnly<u32>,
    /// Rows baked, free-row claims and evictions together. One per
    /// miss, always — more than one would mean the register path is
    /// re-baking content it should have resolved.
    bakes: BenchOnly<u32>,
    /// Bakes that displaced a resident gradient rather than taking a
    /// never-claimed row. Separated from `bakes` because eviction is
    /// what costs a *later* miss: the displaced gradient re-bakes when
    /// it comes back.
    evictions: BenchOnly<u32>,
    /// Capacity doublings. The one-way ratchet — a test that wants to
    /// prove the atlas held its size needs to see this stay flat.
    growths: BenchOnly<u32>,
    /// Registrations that resolved to [`LutRow::FALLBACK`] because the
    /// table was full, every row was spoken for this epoch, and the row
    /// cap refused to grow.
    ///
    /// [`LutRow::FALLBACK`]: crate::primitives::fill_wire::LutRow
    fallbacks: BenchOnly<u32>,
}

impl GradientAtlasProbe {
    #[inline]
    pub(super) fn registration(&mut self) {
        self.registrations.bump();
    }

    #[inline]
    pub(super) fn hit(&mut self) {
        self.hits.bump();
    }

    /// Record one baked row. `evicted` says whether it was holding
    /// another gradient — a flag rather than a second method so the one
    /// call site can't record a bake and forget the eviction that came
    /// with it.
    #[inline]
    pub(super) fn bake(&mut self, evicted: bool) {
        self.bakes.bump();
        if evicted {
            self.evictions.bump();
        }
    }

    #[inline]
    pub(super) fn growth(&mut self) {
        self.growths.bump();
    }

    #[inline]
    pub(super) fn fallback(&mut self) {
        self.fallbacks.bump();
    }
}

/// Reads are gated: only tests and benches ask. Not every accessor has
/// both consumers, so an `internals`-without-`test` build legitimately
/// leaves some unused.
#[cfg(any(test, feature = "internals"))]
#[allow(dead_code)]
impl GradientAtlasProbe {
    pub(super) fn registrations(&self) -> u32 {
        self.registrations.count()
    }

    pub(super) fn hits(&self) -> u32 {
        self.hits.count()
    }

    pub(super) fn bakes(&self) -> u32 {
        self.bakes.count()
    }

    pub(super) fn evictions(&self) -> u32 {
        self.evictions.count()
    }

    pub(super) fn growths(&self) -> u32 {
        self.growths.count()
    }

    pub(super) fn fallbacks(&self) -> u32 {
        self.fallbacks.count()
    }
}
