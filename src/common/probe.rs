//! Build-gated observability primitives — the mechanism the per-pass
//! probes ([`LayoutProbe`], [`DamageProbe`], [`CascadeProbe`]) are built
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
//! one — [`crate::scene::damage::probe`] explains its own case.
//!
//! [`LayoutProbe`]: crate::layout::probe::LayoutProbe
//! [`DamageProbe`]: crate::scene::damage::probe::DamageProbe
//! [`CascadeProbe`]: crate::scene::cascade::probe::CascadeProbe

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
