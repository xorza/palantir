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
//! Nothing is counted while the thread is panicking. Beyond the counts
//! being meaningless once a frame is unwinding rather than rendering,
//! this is what keeps the suite off a Windows deadlock. Stack walking and
//! symbol resolution both go through `dbghelp`, which is single-threaded;
//! the `backtrace` crate and the copy vendored into std serialize on one
//! shared named mutex to cope, and that mutex is recursive per thread. So
//! a panic hook printing its own backtrace (`RUST_BACKTRACE=1`, which CI
//! sets) allocates, re-enters this allocator, and walks the stack again
//! from inside dbghelp's own call — reacquiring the outer mutex happily
//! while blocking on dbghelp's internal ones. With a second test thread
//! panicking at the same time the two wedge each other, and the `alloc`
//! binary hangs until the job times out.
//!
//! Capture is unresolved (`new_unresolved`) so the hot path is just a
//! stack walk, and symbol resolution runs lazily inside the harness
//! when a fixture fails. A window keeps the first [`TRACE_CAP`] of
//! them. Cost is nil for a passing test: a steady-state audit
//! allocates zero times, so it walks nothing.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, RefCell};

use backtrace::Backtrace;

#[derive(Debug)]
pub(crate) struct CountingAllocator;

thread_local! {
    static IN_AUDIT: Cell<bool> = const { Cell::new(false) };
    static CAPTURING: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<u64> = const { Cell::new(0) };
    static BYTES: Cell<u64> = const { Cell::new(0) };
    static TRACES: RefCell<Vec<Backtrace>> = const { RefCell::new(Vec::new()) };
}

/// Allocation traces one audit window keeps. The counters stay exact; this
/// bounds the diagnostic alone.
///
/// A failing frame names its offender in the first few sites, and the
/// harness reports how many it dropped. The bound is what keeps the cost of
/// a failure flat: every capture walks the stack, and every one the harness
/// prints resolves symbols. Windows does both through `dbghelp`, behind one
/// process-wide mutex it shares with the panic printer, at a cost no other
/// platform charges — an unbounded window there ran four of this harness's
/// own tests past a minute each, and the job was cancelled before they
/// finished.
pub(crate) const TRACE_CAP: usize = 8;

#[inline]
fn track(size: usize) {
    // A panicking thread is unwinding, not rendering a frame: what it
    // allocates belongs to the panic machinery, not to the audit window.
    // Skipping it is also what keeps this allocator out of `dbghelp`
    // underneath a panic hook already inside it — see the module docs.
    // `panicking()` is a thread-local read, so the hot path is unmoved.
    if !IN_AUDIT.with(Cell::get) || CAPTURING.with(Cell::get) || std::thread::panicking() {
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
    /// The window's first [`TRACE_CAP`] allocation sites. Past the cap
    /// `allocs` keeps counting, so it can name more allocations than there
    /// are traces.
    pub(crate) traces: Vec<Backtrace>,
}

/// RAII guard: clears `IN_AUDIT` on drop so a panic mid-audit can't
/// strand the flag and poison subsequent operations on this thread.
#[derive(Debug)]
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
