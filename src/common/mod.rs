//! Cross-cutting utilities that don't fit any single subsystem.
//! Submodules are `pub(crate)`; canonical paths are
//! `crate::common::<sub>::<item>`.

pub(crate) mod cache_arena;
pub(crate) mod hash;
pub(crate) mod platform;
