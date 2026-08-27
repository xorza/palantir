//! Build-gated observability primitives — the mechanism the per-pass
//! probes ([`LayoutCounters`], [`DamageCounters`], [`CascadeCounters`]) are built
//! from.
//!
//! ## The pattern
//!
//! Counters that exist only in some builds want to be `#[cfg]`-gated
//! fields, and a gated field forces a gate at every write site too — a
//! `#[cfg]` block per increment, sometimes an extra gated local just to
//! split a borrow. Three passes carried that shape independently.
//!
//! Instead, one cell type owns the gate: it holds its value only when
//! the gate is on, and its mutators are **unconditional** methods whose
//! bodies are gated. Production call sites are plain method calls; with
//! the gate off the type is zero-sized and the calls compile away. The
//! only `#[cfg]`s left in a probe are on its read accessors, which is
//! the one placement that has nowhere else to go.
//!
//! ## Two gates, because the passes have two audiences
//!
//! [`TestOnly`] is `cfg(test)`: for counters nothing benches, and for
//! anything that allocates. A probe that pushes to a `Vec` must not be
//! live in an `internals` build — the `record-only` alloc step asserts
//! steady-state frames allocate nothing and would measure the probe
//! instead of the frame.
//!
//! [`BenchOnly`] is `cfg(any(test, feature = "bench"))`: for the
//! counters a benchmark reads. Widening one costs a build's worth of
//! increments, so it is done for a real bench rather than a hypothetical
//! one — [`crate::scene::damage::counters`] explains its own case.
//!
//! `bench` and not `internals`, though `bench` implies it: the two
//! integration suites turn `internals` on to reach past the published
//! surface and never ask a counter anything, so gating on it retained
//! every cell — and left every reader dead — in exactly the build that
//! has no use for either.
//!
//! **This is the whole rule; a counter module does not restate it.**
//! `TestOnly` unless a benchmark actually reads the counter. What each
//! module documents locally is what it measures and why that is worth
//! separating — not which gate it picked.
//!
//! ## Declaring a set that a test reads deltas off
//!
//! A probe whose tallies accumulate wants a snapshot type beside it, so a
//! test can subtract two readings. That is three lists of the same fields
//! — the cells, the snapshot, and the subtraction — and writing them out
//! let a new counter reach the cells and silently miss the other two.
//! [`counter_snapshot!`] takes the list once and generates all three, and
//! takes both gates on its header line so a set that a *benchmark* reads
//! is declared the same way as one only tests read. Every cache and atlas
//! set in the crate goes through it — the shaped-buffer cache, the
//! encoded-run cache, the block arena, both raster atlases, and the
//! gradient atlas. A probe with no snapshot just declares its cells
//! directly.
//!
//! ## The counter every bounded store owes
//!
//! Each store that can *refuse* work at its ceiling reports that refusal
//! under its own name: `AtlasCounters::oversized` (a raster bigger than
//! the byte budget will ever hold), `GradientAtlasCounters::fallbacks` (a
//! registration that got the magenta row), and `BlockArenaCounters::allocs`
//! (an arena still growing, so the working set has not settled).
//!
//! These are the three a workload test or bench should assert stay at
//! zero, because a non-zero steady-state reading is a configuration
//! problem rather than a load one — no amount of waiting clears them, and
//! nothing else in the pipeline says so. They are *not* a production
//! signal: like every cell here they are gated out of a shipping build,
//! so the assertion has to be written while the workload is still one a
//! test can drive.
//!
//! [`LayoutCounters`]: crate::layout::counters::LayoutCounters
//! [`DamageCounters`]: crate::scene::damage::counters::DamageCounters
//! [`CascadeCounters`]: crate::scene::cascade::counters::CascadeCounters

/// Declare a gated cell type: `T` when `$gate` holds, zero-sized
/// otherwise, with unconditional mutators.
///
/// The `u32` convenience is an inherent impl on the concrete
/// instantiation, so a counter reads `c.bump()` rather than every call
/// site spelling out a closure. The `Vec<T>` one is written out below
/// for [`TestOnly`] alone — see there.
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
            /// Mutate the retained value. Compiles to nothing — `f` is
            /// dropped unrun — when the gate is off, which is what lets
            /// the call sites carry no `#[cfg]` of their own.
            #[inline]
            pub(crate) fn edit(&mut self, f: impl FnOnce(&mut T)) {
                #[cfg($gate)]
                f(&mut self.value);
                #[cfg(not($gate))]
                drop(f);
            }

            /// Restore the gate-on value to its default. No-op otherwise.
            #[inline]
            pub(crate) fn reset(&mut self)
            where
                T: Default,
            {
                self.edit(|v| *v = T::default());
            }

            #[cfg($gate)]
            #[inline]
            pub(crate) fn get(&self) -> &T {
                &self.value
            }
        }

        impl $name<u32> {
            /// Saturating increment, so a runaway pass can't wrap a
            /// count back past zero and read as quiet.
            #[inline]
            pub(crate) fn bump(&mut self) {
                self.edit(|c| *c = c.saturating_add(1));
            }

            #[cfg($gate)]
            #[inline]
            pub(crate) fn count(&self) -> u32 {
                *self.get()
            }
        }

    };
}

gated_cell! {
    /// Retained only in `cfg(test)` builds. The default choice, and the
    /// required one for anything that allocates — see the module doc.
    TestOnly, test
}

gated_cell! {
    /// Retained in `cfg(test)` *and* `bench` builds, for the counters a
    /// benchmark asserts on.
    BenchOnly, any(test, feature = "bench")
}

/// Log conveniences, so a probe reads `l.push(id)` rather than spelling
/// out a closure.
///
/// [`TestOnly`] alone, which is what makes the module rule above a
/// property of the types rather than a line of prose: a cell that
/// allocates must not be live in a non-test build, and a `BenchOnly` one
/// has no way to append to begin with.
impl<T> TestOnly<Vec<T>> {
    /// Append, retaining the backing capacity across resets so a gated
    /// build doesn't reallocate every pass.
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

/// A counter set and the reading its readers subtract.
///
/// [`counter_snapshot!`] generates the snapshot type beside the cells, so
/// the name a set declares for it appears exactly once and leads nowhere.
/// This trait is where the pairing lives: `Self::Counts` resolves from the
/// probe, [`Self::counts`] has a declaration to jump to, and every set a
/// test or a benchmark reads deltas off is an implementor.
///
/// Gated with the union of the `reads` gates the sets declare, because a
/// shipping build implements it for nothing.
#[cfg(any(test, feature = "bench"))]
pub(crate) trait CounterSet {
    /// One reading of every cell. Subtract two to get what a span did.
    type Counts: Copy + std::fmt::Debug + PartialEq + std::ops::Sub<Output = Self::Counts>;

    /// Read every cell. Gated with its callers: nothing in a shipping
    /// build has a reason to ask, and gating the reads is what lets the
    /// cells themselves be absent.
    fn counts(&self) -> Self::Counts;
}

/// Declare a counter set together with the snapshot its readers take
/// deltas off.
///
/// The field list appears once and generates three things that must agree:
/// the gated cells, the plain snapshot, and the `Sub` that turns two
/// readings into what a span did. Written out by hand, adding a counter
/// updated the cells and silently left it out of the snapshot — nothing
/// forces three separate lists to grow together, and the omission reads as
/// a counter that never fires.
///
/// The set reaches its snapshot through [`CounterSet`], which is the one
/// declaration a reader has to follow — the snapshot's own name is written
/// here and nowhere else.
///
/// The header line names both gates, because they are separate questions
/// and every set answers them differently:
///
/// - `cells` picks [`TestOnly`] or [`BenchOnly`] — which builds retain the
///   values at all. The rule for choosing is in this module's doc.
/// - `reads` is the `cfg` the snapshot and the [`CounterSet`] impl are
///   compiled under, and it must name **exactly** the builds that call
///   [`CounterSet::counts`].
///   Wider and the accessor is dead in some build combination, which is
///   how a set ends up carrying a blanket `allow(dead_code)`; narrower and
///   it does not compile. It must also imply `cells`, since `counts()`
///   reads values only the cell gate retains.
///
/// Both visibilities are taken because they differ in practice: a probe is
/// usually as private as the type it measures, while its snapshot travels
/// to wherever the tests live. Field types are spelled out because not
/// every tally is a count — a scan total wants a `u64`.
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
