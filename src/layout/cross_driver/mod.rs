//! Cross-driver tests: layouts × text wrapping, fill propagation, and
//! overlap regressions. Driver-local semantics live under each driver's
//! own `tests.rs`; tests here exercise multiple drivers together.
//!
//! Internals access (`pub(crate)` fields on `LayoutResult`,
//! `cmd_buffer::CmdKind`, `crate::test_support::*`) is intentional —
//! moving these to crate-root `tests/` would force widening half a
//! dozen items to `pub` purely for tests.

mod fill_propagation;
mod no_overlap;
mod support;
mod text_wrap;
