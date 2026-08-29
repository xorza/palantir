//! Per-frame allocation audit suite.
//!
//! Two halves, and which one to reach for matters. `fixtures/` audits
//! ~20 small scenes a frame at a time with backtrace capture, so a
//! failure names the line that allocated — start there when a number
//! moves. `gates.rs` holds the three coarse checks only it can make,
//! each of them over the full tree. Add a gate only for something the
//! fixtures structurally cannot see.
//!
//! Most fixtures are GPU-less and read a strict zero. The four in
//! `fixtures/renderer.rs` are not: encode and compose live behind a
//! `Frontend`, so those go through a device and read a ceiling instead.
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
mod harness_tests;
