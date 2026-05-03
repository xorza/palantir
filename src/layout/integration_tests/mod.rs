//! Cross-driver integration tests: layouts × text wrapping, fill
//! propagation, and overlap regressions. Driver-local semantics live
//! under each driver's own `tests.rs`; tests here exercise multiple
//! drivers together.

mod fill_propagation;
mod no_overlap;
mod scaffold;
mod text_wrap;
