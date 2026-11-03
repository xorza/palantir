//! Cross-driver tests: layouts × text wrapping, fill propagation, overlap regressions. Driver-local semantics live in each driver's `tests.rs`.
//!
//! Layout cases run on `internals::layout_rig`, so no widget default moves an expectation; the cases that read paint calls or the shaper's cache across frames run a `Harness`. The `pub(crate)` reach-ins are intentional: crate-root `tests/` would force widening items to `pub`.

mod arrange_axis;
mod convergence;
mod fill_propagation;
mod no_overlap;
mod stretch_semantics;
mod support;
mod text_wrap;
