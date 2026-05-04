//! Per-thread counting allocator. Wraps `System`; while a thread is
//! "in audit" (set by the harness around the measured frames),
//! increments thread-local `ALLOCS`/`BYTES` on every `alloc` /
//! `realloc` and optionally captures a `Backtrace` so failures can
//! point at the offending call site. `dealloc` is always delegated
//! unchanged — we count heap *operations*, not residency.
//!
//! Per-thread (not global) counters are deliberate: cargo runs tests
//! in parallel on the same process, and a global counter would let
//! other tests' setup allocations on other threads leak into our
//! audit window. Gating on the per-thread `IN_AUDIT` flag means only
//! the auditing thread's audit-window allocs ever increment.
//!
//! `CAPTURING` is a per-thread re-entry guard so the bookkeeping
//! allocs (Vec growth in `TRACES`, backtrace internals) neither
//! recurse forever nor get counted.
//!
//! We use the lower-level `backtrace::Backtrace` (rather than
//! `std::backtrace::Backtrace`) for the captured value because the
//! `Frame`/`Symbol` API exposes filename and line directly, which the
//! harness uses to drop std/runtime/dep frames structurally. Capture
//! is unresolved (`new_unresolved`) so the hot path is just a stack
//! walk; symbol resolution runs lazily inside the harness when a
//! fixture fails. Capture is unconditional — the cost is negligible
//! for passing tests (steady-state audits allocate zero times) and
//! we want traces always available on failure.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, RefCell};

pub(crate) use backtrace::Backtrace;

pub(crate) struct CountingAllocator;

thread_local! {
    static IN_AUDIT: Cell<bool> = const { Cell::new(false) };
    static CAPTURING: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<u64> = const { Cell::new(0) };
    static BYTES: Cell<u64> = const { Cell::new(0) };
    static TRACES: RefCell<Vec<Backtrace>> = const { RefCell::new(Vec::new()) };
}

#[inline]
fn track(layout: Layout) {
    if !IN_AUDIT.with(Cell::get) || CAPTURING.with(Cell::get) {
        return;
    }
    ALLOCS.with(|c| c.set(c.get() + 1));
    BYTES.with(|c| c.set(c.get() + layout.size() as u64));
    CAPTURING.with(|f| f.set(true));
    let bt = Backtrace::new_unresolved();
    TRACES.with(|t| t.borrow_mut().push(bt));
    CAPTURING.with(|f| f.set(false));
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        track(layout);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        track(layout);
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        track(unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) });
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
        allocs: ALLOCS.with(Cell::get),
        bytes: BYTES.with(Cell::get),
    }
}

pub(crate) fn delta(prev: Snapshot) -> Snapshot {
    let now = snapshot();
    Snapshot {
        allocs: now.allocs - prev.allocs,
        bytes: now.bytes - prev.bytes,
    }
}

/// Enable per-thread alloc counting (and backtrace capture if
/// `RUST_BACKTRACE` is set). Each subsequent alloc on this thread
/// increments thread-local counters and pushes one `Backtrace` onto
/// a thread-local buffer until [`set_in_audit(false)`] is called.
pub(crate) fn set_in_audit(on: bool) {
    IN_AUDIT.with(|f| f.set(on));
}

/// Drain captured backtraces from this thread's buffer.
pub(crate) fn take_traces() -> Vec<Backtrace> {
    TRACES.with(|t| std::mem::take(&mut *t.borrow_mut()))
}
