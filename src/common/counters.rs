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
//! [`BenchOnly`] is `cfg(any(test, feature = "internals"))`: for the
//! counters a benchmark reads. Widening one costs a build's worth of
//! increments, so it is done for a real bench rather than a hypothetical
//! one — [`crate::scene::damage::counters`] explains its own case.
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
//! [`counter_snapshot!`] takes the list once and generates all three; it
//! is what `CacheCounters` (the shaped-buffer cache) and `EncodedCounters`
//! (the encoded-run cache) are declared with — both in private modules, so
//! neither is linkable from here. A probe with no snapshot just declares
//! its cells directly.
//!
//! [`LayoutCounters`]: crate::layout::counters::LayoutCounters
//! [`DamageCounters`]: crate::scene::damage::counters::DamageCounters
//! [`CascadeCounters`]: crate::scene::cascade::counters::CascadeCounters

/// Declare a gated cell type: `T` when `$gate` holds, zero-sized
/// otherwise, with unconditional mutators.
///
/// The `u32` and `Vec<T>` conveniences are inherent impls on the
/// concrete instantiations, so a counter reads `c.bump()` and a log
/// reads `l.push(id)` rather than every call site spelling out a
/// closure.
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

        impl<T> $name<Vec<T>> {
            /// Append, retaining the backing capacity across resets so a
            /// gated build doesn't reallocate every pass.
            #[inline]
            pub(crate) fn push(&mut self, item: T) {
                self.edit(move |log| log.push(item));
            }

            #[inline]
            pub(crate) fn clear(&mut self) {
                self.edit(Vec::clear);
            }

            #[cfg($gate)]
            #[inline]
            pub(crate) fn as_slice(&self) -> &[T] {
                self.get()
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
    /// Retained in `cfg(test)` *and* `internals` builds, for the
    /// counters a benchmark asserts on.
    BenchOnly, any(test, feature = "internals")
}

/// Declare a counter set together with the snapshot a test reads off it.
///
/// The field list appears once and generates three things that must agree:
/// the gated cells, the plain-`u32` snapshot, and the `Sub` that turns two
/// readings into what a span did. Written out by hand, adding a counter
/// updated the cells and silently left it out of the snapshot — nothing
/// forces three separate lists to grow together, and the omission reads as
/// a counter that never fires.
///
/// Both visibilities are taken because they differ in practice: a probe is
/// usually as private as the type it measures, while its snapshot travels
/// to wherever the tests live.
macro_rules! counter_snapshot {
    (
        $(#[$counters_meta:meta])*
        $cvis:vis struct $counters:ident;
        $(#[$snapshot_meta:meta])*
        $svis:vis struct $snapshot:ident;
        $($(#[$field_meta:meta])* $field:ident,)+
    ) => {
        $(#[$counters_meta])*
        #[derive(Debug, Default)]
        $cvis struct $counters {
            $($(#[$field_meta])* $cvis $field: $crate::common::counters::TestOnly<u32>,)+
        }

        $(#[$snapshot_meta])*
        #[cfg(test)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        $svis struct $snapshot {
            $($svis $field: u32,)+
        }

        /// Reads are test-only: nothing in a shipping build has a reason
        /// to ask, and gating them is what lets the cells themselves be
        /// absent.
        #[cfg(test)]
        impl $counters {
            $cvis fn counts(&self) -> $snapshot {
                $snapshot { $($field: self.$field.count(),)+ }
            }
        }

        #[cfg(test)]
        impl std::ops::Sub for $snapshot {
            type Output = Self;

            fn sub(self, base: Self) -> Self {
                Self { $($field: self.$field - base.$field,)+ }
            }
        }
    };
}

pub(crate) use counter_snapshot;
