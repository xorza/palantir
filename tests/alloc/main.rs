//! Per-frame allocation audit suite.
//!
//! Two halves, and which one to reach for matters. `fixtures/` audits
//! small scenes a frame at a time with backtrace capture, so a
//! failure names the line that allocated — start there when a number
//! moves. `gates/` holds the three coarse checks only it can make,
//! each of them over the full tree. Add a gate only for something the
//! fixtures structurally cannot see.
//!
//! Every fixture is GPU-less and reads a strict zero; the ones in
//! `fixtures/renderer.rs` encode and compose on a deviceless frontend.
//! Only `gates/on_gpu.rs` takes a device, because only it asks what the
//! driver costs. Its still-tree gate measures the adapter's floor in the
//! same run and runs everywhere; the scale ramp reads a ceiling measured
//! on one adapter, so it is the one test CI skips. A new device-driven
//! audit belongs in that module.
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
