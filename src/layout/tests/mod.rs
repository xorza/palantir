//! Cross-driver tests: layouts × text wrapping, fill propagation, overlap regressions. Driver-local semantics live in each driver's `tests.rs`.
//!
//! The `pub(crate)` reach-ins (`Layout` fields, `internals::paint_capture::PaintCall`, `UiHarness`) are intentional: crate-root `tests/` would force widening items to `pub`.

mod arrange_axis;
mod convergence;
mod fill_propagation;
mod no_overlap;
mod stretch_semantics;
mod support;
mod text_wrap;
