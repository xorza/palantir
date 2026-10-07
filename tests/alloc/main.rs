//! Per-frame allocation audit suite.
//!
//! `fixtures/` audits small scenes a frame at a time with backtrace capture, so a failure names the allocating line; start there when a number moves. `gates/` holds the three coarse full-tree checks only it can make; add one only for what fixtures structurally cannot see.
//!
//! Every fixture is GPU-less and reads a strict zero (`fixtures/renderer.rs` encodes and composes on a deviceless frontend). Only `gates/on_gpu.rs` takes a device, to ask what the driver costs: its still-tree gate measures the adapter's floor through the same target and runs everywhere; the scale ramp reads a ceiling measured on one adapter, so CI skips it. New device-driven audits belong there.
//!
//! One `CountingAllocator` serves both; counters are per-thread, so parallel tests don't pollute each other's windows and the tests pay no allocator tax.

use crate::allocator::CountingAllocator;

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

mod allocator;
mod fixtures;
mod gates;
mod harness;
