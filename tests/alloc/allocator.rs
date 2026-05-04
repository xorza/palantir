//! Process-global counting allocator. Wraps `System`; increments
//! `ALLOCS` on every `alloc` / `realloc`, and `BYTES` by the requested
//! size. `dealloc` is delegated unchanged — we count heap *operations*,
//! not residency.
//!
//! `Relaxed` ordering is sufficient: snapshots are taken on the test
//! thread between fully-fenced frame loops, and the System allocator
//! provides its own ordering for the actual heap.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

pub(crate) struct CountingAllocator;

static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        BYTES.fetch_add(layout.size() as u64, Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        BYTES.fetch_add(layout.size() as u64, Relaxed);
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        BYTES.fetch_add(new_size as u64, Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Snapshot {
    pub(crate) allocs: u64,
    pub(crate) bytes: u64,
}

pub(crate) fn snapshot() -> Snapshot {
    Snapshot {
        allocs: ALLOCS.load(Relaxed),
        bytes: BYTES.load(Relaxed),
    }
}

pub(crate) fn delta(prev: Snapshot) -> Snapshot {
    let now = snapshot();
    Snapshot {
        allocs: now.allocs - prev.allocs,
        bytes: now.bytes - prev.bytes,
    }
}
