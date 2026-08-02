//! Observability for the cascade pass.
//!
//! Same pattern as [`crate::layout::probe::LayoutProbe`], whose module
//! doc explains it, and [`crate::scene::damage::probe::DamageProbe`]:
//! gated fields, unconditional mutators with gated bodies, so
//! `CascadeEngine::run` carries no `#[cfg]` of its own and the struct
//! compiles away entirely in a build without the counters.
//!
//! ## Why these accumulate instead of resetting per pass
//!
//! Both peers clear their counters at the top of the pass they measure.
//! This one deliberately doesn't. `Ui::post_record`'s fingerprint gate
//! skips [`CascadeEngine::run`] outright on an unchanged frame, so a
//! per-run reset would simply not fire on those frames and every skipped
//! frame would report the *previous* run's numbers as though they were
//! its own — the exact reading error the reset is supposed to prevent.
//! Accumulating for the life of the engine and letting readers take a
//! delta is the shape that survives a pass that may not run, the same
//! call [`PaintSnapArena::compactions_run`] makes.
//!
//! ## Why `cfg(test)` and not `internals`
//!
//! [`DamageProbe`] widens to `internals` because the damage bench reads
//! it. Nothing benches these two yet, and the crate escalates visibility
//! for a real caller rather than a hypothetical one — widening now would
//! only buy an `allow(dead_code)` on accessors no bench calls. If a
//! cascade bench wants them, widen then.
//!
//! [`CascadeEngine::run`]: crate::scene::cascade::engine::CascadeEngine
//! [`DamageProbe`]: crate::scene::damage::probe::DamageProbe
//! [`PaintSnapArena::compactions_run`]:
//!     crate::scene::damage::snapshot::PaintSnapArena

/// What the cascade did, for tests to assert against.
///
/// The two counters exist as a pair: both paths end in the same correct
/// cascade, so from the outside there is nothing to tell them apart, and
/// separating "`can_update` said no" from "the incremental walk gave up
/// halfway" is the whole point.
#[derive(Debug, Default)]
pub(crate) struct CascadeProbe {
    /// Full rebuilds performed.
    #[cfg(test)]
    full_rebuilds: u32,
    /// Incremental walks that got partway and gave up, forcing the full
    /// rebuild they had already started duplicating.
    #[cfg(test)]
    abandoned_incrementals: u32,
}

impl CascadeProbe {
    #[inline]
    pub(crate) fn full_rebuild(&mut self) {
        #[cfg(test)]
        {
            self.full_rebuilds = self.full_rebuilds.saturating_add(1);
        }
    }

    #[inline]
    pub(crate) fn abandoned_incremental(&mut self) {
        #[cfg(test)]
        {
            self.abandoned_incrementals = self.abandoned_incrementals.saturating_add(1);
        }
    }
}

/// Reads are gated: only tests ask, and gating them here is what lets
/// the fields themselves be absent.
#[cfg(test)]
impl CascadeProbe {
    pub(crate) fn full_rebuilds(&self) -> u32 {
        self.full_rebuilds
    }

    pub(crate) fn abandoned_incrementals(&self) -> u32 {
        self.abandoned_incrementals
    }
}
