//! Build-gated observability primitives behind the per-pass probes ([`LayoutCounters`], [`DamageCounters`], [`CascadeCounters`]).
//!
//! One cell type owns the gate: it holds its value only when the gate is on, and its mutators are **unconditional** methods with gated bodies, so call sites are plain method calls and the type is zero-sized when off. Only read accessors carry a `#[cfg]`.
//!
//! - [`TestOnly`] (`cfg(test)`): for counters nothing benches and for anything that allocates, which the allocation suite (`tests/alloc`) would otherwise measure instead of the frame.
//! - [`BenchOnly`] (`cfg(any(test, feature = "bench"))`): for counters a benchmark reads, gated on `bench` not `internals`, which the integration suites enable without reading counters.
//!
//! `TestOnly` unless a benchmark reads the counter; a counter module documents what it measures, not which gate it picked.
//!
//! **A probe accumulates for the life of its owner and readers subtract two readings**, which survives a pass that may not run. A reading meaningless outside one pass (a `Vec` log, a phase timing) clears at the top of it via **`begin_pass`**, as the layout and damage probes do.
//!
//! [`counter_snapshot!`] generates the cells, snapshot and subtraction from one field list, so a new counter can't miss one of them.
//!
//! A store that can *refuse* work at its ceiling reports it under its own name (`AtlasCounters::oversized`, `GradientAtlasCounters::fallbacks`, `BlockArenaCounters::allocs`); a workload test should assert these stay zero, since waiting won't clear them. They are gated out of shipping builds.
//!
//! [`LayoutCounters`]: crate::layout::counters::LayoutCounters
//! [`DamageCounters`]: crate::damage::counters::DamageCounters
//! [`CascadeCounters`]: crate::cascade::counters::CascadeCounters

use std::cell::Cell;

/// Declares a gated cell type: `T` when `$gate` holds, zero-sized otherwise, with unconditional mutators.
macro_rules! gated_cell {
    ($(#[$meta:meta])* $name:ident, $gate:meta) => {
        $(#[$meta])*
        #[derive(Debug, Default)]
        pub(crate) struct $name<T> {
            #[cfg($gate)]
            value: T,
            #[cfg(not($gate))]
            _unused: std::marker::PhantomData<T>,
        }

        impl<T> $name<T> {
            #[inline]
            pub(crate) fn edit(&mut self, f: impl FnOnce(&mut T)) {
                #[cfg($gate)]
                f(&mut self.value);
                #[cfg(not($gate))]
                drop(f);
            }

            #[inline]
            pub(crate) fn reset(&mut self)
            where
                T: Default,
            {
                self.edit(|v| *v = T::default());
            }

            #[cfg($gate)]
            #[inline]
            pub(crate) const fn get(&self) -> &T {
                &self.value
            }
        }

        impl $name<u32> {
            #[inline]
            pub(crate) fn bump(&mut self) {
                self.edit(|c| *c = c.saturating_add(1));
            }

            #[cfg($gate)]
            #[inline]
            pub(crate) const fn count(&self) -> u32 {
                *self.get()
            }
        }

    };
}

gated_cell! {
    /// Retained only in `cfg(test)` builds; the default, and required for anything that allocates.
    TestOnly, test
}

gated_cell! {
    /// Retained in `cfg(test)` and `bench` builds, for counters a benchmark asserts on.
    BenchOnly, any(test, feature = "bench")
}

/// Log conveniences for [`TestOnly`] alone: a cell that allocates must not be live in a non-test build.
impl<T> TestOnly<Vec<T>> {
    #[inline]
    pub(crate) fn push(&mut self, item: T) {
        self.edit(move |log| log.push(item));
    }

    #[inline]
    pub(crate) fn clear(&mut self) {
        self.edit(Vec::clear);
    }

    #[cfg(test)]
    #[inline]
    pub(crate) fn as_slice(&self) -> &[T] {
        self.get()
    }
}

impl TestOnly<Cell<u32>> {
    #[inline]
    #[cfg_attr(
        not(test),
        expect(
            clippy::missing_const_for_fn,
            reason = "the test build's body calls `Cell::set`, which is not const"
        )
    )]
    pub(crate) fn bump_shared(&self) {
        #[cfg(test)]
        self.value.set(self.value.get().saturating_add(1));
    }

    #[cfg(test)]
    #[inline]
    pub(crate) const fn count(&self) -> u32 {
        self.value.get()
    }
}

/// A counter set and the reading its readers subtract; [`counter_snapshot!`] generates the snapshot beside the cells. Gated with the union of the sets' `reads` gates.
#[cfg(any(test, feature = "bench"))]
pub(crate) trait CounterSet {
    type Counts: Copy + std::fmt::Debug + PartialEq + std::ops::Sub<Output = Self::Counts>;

    fn counts(&self) -> Self::Counts;
}

/// Declares a counter set with the snapshot its readers take deltas off.
///
/// The header names both gates:
///
/// - `cells` picks [`TestOnly`] or [`BenchOnly`], which builds retain values.
/// - `reads` is the `cfg` the snapshot and `CounterSet` impl compile under; it must name **exactly** the builds that call `CounterSet::counts` (wider leaves dead code, narrower fails to compile) and imply `cells`.
///
/// Field types are spelled out since a scan total wants a `u64`.
macro_rules! counter_snapshot {
    (
        cells $cell:ident, reads $reads:meta;

        $(#[$counters_meta:meta])*
        $cvis:vis struct $counters:ident;
        $(#[$snapshot_meta:meta])*
        $svis:vis struct $snapshot:ident;
        $($(#[$field_meta:meta])* $field:ident: $fty:ty,)+
    ) => {
        $(#[$counters_meta])*
        #[derive(Debug, Default)]
        $cvis struct $counters {
            $($(#[$field_meta])* $cvis $field: $crate::common::counters::$cell<$fty>,)+
        }

        $(#[$snapshot_meta])*
        #[$reads]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        $svis struct $snapshot {
            $($svis $field: $fty,)+
        }

        #[$reads]
        impl $crate::common::counters::CounterSet for $counters {
            type Counts = $snapshot;

            fn counts(&self) -> $snapshot {
                $snapshot { $($field: *self.$field.get(),)+ }
            }
        }

        #[$reads]
        impl std::ops::Sub for $snapshot {
            type Output = Self;

            fn sub(self, base: Self) -> Self {
                Self { $($field: self.$field - base.$field,)+ }
            }
        }
    };
}

pub(crate) use counter_snapshot;
