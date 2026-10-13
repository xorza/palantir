//! Per-thread counting allocator wrapping `System`. While a thread is in audit
//! ([`with_audit`]) every `alloc` / `realloc` bumps thread-local counters and
//! captures an unresolved backtrace; `dealloc` is delegated unchanged (heap
//! *operations*, not residency).
//!
//! Counters are per-thread because cargo runs tests in parallel; the blind spot is
//! allocation on other threads, which holds while the frame path spawns none.
//! `CAPTURING` is a re-entry guard so bookkeeping allocs neither recurse nor count.
//!
//! Nothing is counted while panicking: on Windows a panic hook printing a backtrace
//! re-enters this allocator inside single-threaded `dbghelp`, and two panicking
//! threads deadlock the `alloc` binary.

#![expect(
    clippy::disallowed_types,
    reason = "a global allocator has no owner to carry its counters"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, RefCell};

use backtrace::Backtrace;
use std::mem;
use std::thread;

#[derive(Debug)]
pub(crate) struct CountingAllocator;

thread_local! {
    static IN_AUDIT: Cell<bool> = const { Cell::new(false) };
    static CAPTURING: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<u64> = const { Cell::new(0) };
    static BYTES: Cell<u64> = const { Cell::new(0) };
    static TRACES: RefCell<Vec<Backtrace>> = const { RefCell::new(Vec::new()) };
}

/// Allocation traces one audit window keeps. Counters stay exact; this bounds only
/// the diagnostic, and failure cost: on Windows an unbounded window ran four
/// harness tests past a minute each (captures and symbol resolution share
/// `dbghelp`'s process-wide mutex).
pub(crate) const TRACE_CAP: usize = 8;

#[inline]
fn track(size: usize) {
    // A panicking thread is unwinding, not rendering; skipping it keeps this
    // allocator out of `dbghelp` under a panic hook.
    if !IN_AUDIT.with(Cell::get) || CAPTURING.with(Cell::get) || thread::panicking() {
        return;
    }
    ALLOCS.with(|c| c.set(c.get() + 1));
    BYTES.with(|c| c.set(c.get() + size as u64));
    CAPTURING.with(|f| f.set(true));
    if TRACES.with(|t| t.borrow().len()) < TRACE_CAP {
        let bt = Backtrace::new_unresolved();
        TRACES.with(|t| t.borrow_mut().push(bt));
    }
    CAPTURING.with(|f| f.set(false));
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        track(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        track(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        track(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[derive(Debug)]
pub(crate) struct AuditResult {
    pub(crate) allocs: u64,
    pub(crate) bytes: u64,
    /// The window's first [`TRACE_CAP`] allocation sites; `allocs` keeps counting
    /// past the cap.
    pub(crate) traces: Vec<Backtrace>,
}

/// RAII guard: clears `IN_AUDIT` on drop so a panic mid-audit cannot strand the
/// flag.
#[derive(Debug)]
struct AuditGuard;

impl AuditGuard {
    /// Panics inside an open window: the inner would drain the outer's traces and
    /// clear its flag.
    fn enter() -> Self {
        assert!(
            !IN_AUDIT.with(Cell::get),
            "with_audit called inside an open audit window"
        );
        IN_AUDIT.with(|f| f.set(true));
        Self
    }
}

impl Drop for AuditGuard {
    fn drop(&mut self) {
        IN_AUDIT.with(|f| f.set(false));
    }
}

/// Run `f` with allocation counting and backtrace capture on this thread, returning
/// the delta and the traces scoped to `f`. Stale traces are drained first.
pub(crate) fn with_audit<F: FnOnce()>(f: F) -> AuditResult {
    let guard = AuditGuard::enter();
    TRACES.with(|t| t.borrow_mut().clear());
    let allocs0 = ALLOCS.with(Cell::get);
    let bytes0 = BYTES.with(Cell::get);
    f();
    drop(guard);
    AuditResult {
        allocs: ALLOCS.with(Cell::get) - allocs0,
        bytes: BYTES.with(Cell::get) - bytes0,
        traces: TRACES.with(|t| mem::take(&mut *t.borrow_mut())),
    }
}

#[cfg(test)]
mod tests;
