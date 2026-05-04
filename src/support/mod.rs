//! Test + bench infrastructure. `internals` exposes cross-frame cache
//! resets to benches and tests behind the `internals` feature;
//! `testing` holds shared `cfg(test)`-only helpers used by in-tree
//! tests. Production builds compile out everything here.

#[cfg(any(test, feature = "internals"))]
pub mod internals;

#[cfg(test)]
pub(crate) mod testing;
