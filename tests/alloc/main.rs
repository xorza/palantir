//! Per-frame allocation audit suite.
//!
//! Two halves, and which one to reach for matters. `fixtures/` audits
//! ~20 small scenes a frame at a time with backtrace capture, so a
//! failure names the line that allocated — start there when a number
//! moves. `gates/` holds the three coarse checks only it can make,
//! each of them over the full tree. Add a gate only for something the
//! fixtures structurally cannot see.
//!
//! Most fixtures are GPU-less and read a strict zero. The four in
//! `fixtures/renderer.rs` are not: encode and compose live behind a
//! `Frontend`, so those go through a device and read a ceiling instead.
//! `gates/on_gpu.rs` holds the two gates in the same position.
//!
//! A ceiling belongs to the driver, so `fixtures::renderer` and
//! `gates::on_gpu` are the module paths CI skips. A new device-driven
//! audit belongs in one of them.
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
