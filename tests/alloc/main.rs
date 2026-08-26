//! Per-frame allocation audit suite.
//!
//! Two halves, and which one to reach for matters. `fixtures/` audits
//! ~20 small scenes a frame at a time with backtrace capture, so a
//! failure names the line that allocated — start there when a number
//! moves. `gates.rs` holds the two coarse checks only it can make:
//! whether the pipeline allocates at all at full scale, and whether the
//! wgpu driver floor beneath it has drifted. Add a gate only for
//! something the fixtures structurally cannot see.
//!
//! One `CountingAllocator` serves both. Its counters are per-thread, so
//! cargo's parallel runner cannot pollute one window with another's
//! allocations, and the tests sharing this binary pay no allocator tax.

use crate::allocator::CountingAllocator;

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

mod allocator;
mod fixtures;
mod gates;
mod harness;
#[cfg(test)]
mod harness_tests;
