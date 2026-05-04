//! Per-frame allocation audit suite. See `alloc-testing.md`.

use crate::allocator::CountingAllocator;

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

mod allocator;
mod fixtures;
mod harness;
#[cfg(test)]
mod harness_tests;
