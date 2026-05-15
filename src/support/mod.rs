//! Test + bench scaffolding. `internals` is the bench/test reach-in surface
//! (gated on `cfg(test)` or `feature = "internals"`); `testing` is `cfg(test)`-only
//! fixtures. Production builds compile out everything here.

#[cfg(any(test, feature = "internals"))]
pub mod internals;

#[cfg(test)]
pub(crate) mod testing;
