//! Per-thread counting allocator. Wraps `System`; while a thread is
//! "in audit" (set by [`with_audit`] around the measured frames),
//! increments thread-local counters on every `alloc` / `realloc` and
//! captures a `backtrace::Backtrace` so failures can point at the
//! offending call site. `dealloc` is always delegated unchanged — we
//! count heap *operations*, not residency.
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
//! Capture is unconditional and unresolved (`new_unresolved`) so the
//! hot path is just a stack walk; symbol resolution runs lazily
//! inside the harness when a fixture fails. Cost is negligible for
//! passing tests (steady-state audits allocate zero times) and we
//! want traces always available on failure.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, RefCell};

use backtrace::Backtrace;

pub(crate) struct CountingAllocator;

thread_local! {
    static IN_AUDIT: Cell<bool> = const { Cell::new(false) };
    static CAPTURING: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<u64> = const { Cell::new(0) };
    static BYTES: Cell<u64> = const { Cell::new(0) };
    static TRACES: RefCell<Vec<Backtrace>> = const { RefCell::new(Vec::new()) };
}

#[inline]
fn track(size: usize) {
    if !IN_AUDIT.with(Cell::get) || CAPTURING.with(Cell::get) {
        return;
    }
    ALLOCS.with(|c| c.set(c.get() + 1));
    BYTES.with(|c| c.set(c.get() + size as u64));
    CAPTURING.with(|f| f.set(true));
    let bt = Backtrace::new_unresolved();
    TRACES.with(|t| t.borrow_mut().push(bt));
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
    pub(crate) traces: Vec<Backtrace>,
}

/// RAII guard: clears `IN_AUDIT` on drop so a panic mid-audit can't
/// strand the flag and poison subsequent operations on this thread.
struct AuditGuard;

impl AuditGuard {
    fn enter() -> Self {
        IN_AUDIT.with(|f| f.set(true));
        Self
    }
}

impl Drop for AuditGuard {
    fn drop(&mut self) {
        IN_AUDIT.with(|f| f.set(false));
    }
}

/// Run `f` with allocation counting + backtrace capture enabled on
/// the current thread. Returns the allocation delta and drained
/// `TRACES` buffer scoped to `f`. On panic inside `f`, the guard's
/// `Drop` clears `IN_AUDIT` so the thread is left in a clean state
/// before the panic continues unwinding.
///
/// Drains any stale `TRACES` from a previous call on this thread
/// before entering, so callers don't have to remember.
pub(crate) fn with_audit<F: FnOnce()>(f: F) -> AuditResult {
    TRACES.with(|t| t.borrow_mut().clear());
    let allocs0 = ALLOCS.with(Cell::get);
    let bytes0 = BYTES.with(Cell::get);
    let guard = AuditGuard::enter();
    f();
    drop(guard);
    AuditResult {
        allocs: ALLOCS.with(Cell::get) - allocs0,
        bytes: BYTES.with(Cell::get) - bytes0,
        traces: TRACES.with(|t| std::mem::take(&mut *t.borrow_mut())),
    }
}
