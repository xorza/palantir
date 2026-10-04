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
//! The price is a blind spot: an allocation on any other thread is never
//! counted. That holds for now because the frame path spawns no thread —
//! the crate has no worker pool. Work handed to one would allocate where
//! no window sees it, and every audit would still read zero.
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

/// Allocation traces one audit window keeps. The counters stay exact; this
/// bounds the diagnostic alone.
///
/// A failing frame names its offender in the first few sites, and the
/// harness reports how many it dropped. The bound is what keeps the cost of
/// a failure flat: every capture walks the stack, and every one the harness
/// prints resolves symbols. Windows does both through `dbghelp`, behind one
/// process-wide mutex it shares with the panic printer, at a cost no other
/// platform charges — an unbounded window there ran four of this harness's
/// own tests past a minute each, and the job was canceled before they
/// finished.
pub(crate) const TRACE_CAP: usize = 8;

#[inline]
fn track(size: usize) {
    // A panicking thread is unwinding, not rendering a frame: what it
    // allocates belongs to the panic machinery, not to the audit window.
    // Skipping it is also what keeps this allocator out of `dbghelp`
    // underneath a panic hook already inside it — see the module docs.
    // `panicking()` is a thread-local read, so the hot path is unmoved.
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
    /// Panics inside an open window: the inner one would drain the outer
    /// one's traces on entry and clear its flag on exit, so the outer
    /// window would go on silently counting nothing.
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

/// Run `f` with allocation counting + backtrace capture enabled on
/// the current thread. Returns the allocation delta and drained
/// `TRACES` buffer scoped to `f`. On panic inside `f`, the guard's
/// `Drop` clears `IN_AUDIT` so the thread is left in a clean state
/// before the panic continues unwinding.
///
/// Drains any stale `TRACES` from a previous call on this thread
/// before entering, so callers don't have to remember.
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

/// The counter, the capture and the guard: per-thread semantics the
/// fixtures rely on without checking — exact counts, silence outside a
/// window, isolation from sibling threads, a bookkeeping path that stays
/// out of the count, and a guard that survives a panicking body.
#[cfg(test)]
mod tests {
    use crate::allocator::{TRACE_CAP, with_audit};
    use std::hint;
    use std::hint::black_box;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;

    /// Force one heap alloc that the optimizer can't hoist or elide.
    fn one_alloc() {
        black_box(Box::new(black_box(0u64)));
    }

    #[test]
    fn counts_exactly_what_audit_window_allocates() {
        let r = with_audit(|| {
            for _ in 0..5 {
                one_alloc();
            }
        });
        assert_eq!(r.allocs, 5, "expected 5 allocs in the audited window");
        assert_eq!(r.bytes, 5 * 8, "five boxed u64s, 8 bytes each");
    }

    #[test]
    fn allocs_outside_audit_are_silent() {
        for _ in 0..32 {
            one_alloc();
        }
        let r = with_audit(|| {});
        for _ in 0..32 {
            one_alloc();
        }
        assert_eq!(
            r.allocs, 0,
            "non-audited allocs must not count, got {}",
            r.allocs
        );
    }

    #[test]
    fn sibling_thread_allocs_do_not_pollute_audit() {
        // Spawn the worker *before* entering audit (thread::spawn allocates
        // on the caller). An AtomicBool start flag signals the worker to
        // begin its burst once we're inside the audit window; `t.join()` is
        // the trailing happens-before barrier — no second wait needed.
        //
        // Why not `std::sync::Barrier`: on macOS, the first
        // `Barrier::wait` lazily heap-allocates the underlying pthread
        // `Mutex` (via `OnceBox<Mutex>::get_or_init` → `Box::pin`), which
        // would land on the auditing thread inside `with_audit` and pollute
        // the delta. Linux uses futex-based mutexes with no lazy alloc.
        // Atomics never allocate.
        let go = Arc::new(AtomicBool::new(false));
        let g2 = Arc::clone(&go);
        let t = thread::spawn(move || {
            while !g2.load(Ordering::Acquire) {
                hint::spin_loop();
            }
            for _ in 0..1_000 {
                one_alloc();
            }
        });

        let r = with_audit(|| {
            go.store(true, Ordering::Release);
            t.join().unwrap();
        });

        assert_eq!(
            r.allocs, 0,
            "sibling thread's 1000 allocs leaked into our delta (got {})",
            r.allocs,
        );
    }

    #[test]
    fn re_entry_guard_keeps_counter_and_traces_aligned() {
        // The bookkeeping path (Vec growth in TRACES, Backtrace internals)
        // calls back into the allocator. CAPTURING must suppress those, or the
        // counter would run past what the body allocated and the capture would
        // recurse. The sub-cap row allocates TRACES' initial buffer and grows it
        // once, and the row above the cap is what proves the bound leaves the
        // count alone.
        for allocs in [TRACE_CAP - 1, 64] {
            let r = with_audit(|| {
                for _ in 0..allocs {
                    one_alloc();
                }
            });
            assert_eq!(
                r.allocs as usize, allocs,
                "the guard must keep its own bookkeeping out of the count",
            );
            assert_eq!(
                r.traces.len(),
                allocs.min(TRACE_CAP),
                "traces follow the count to the cap and stop (allocs={allocs})",
            );
        }
    }

    /// If `with_audit`'s body panics, the guard's Drop must clear IN_AUDIT
    /// so a follow-up `with_audit` on this thread starts clean. Without the
    /// guard the flag would stay stuck and the post-panic audit would
    /// inherit allocations from the unwinding path (drop glue, panic
    /// reporting, etc.). A nested window is one such panic: it would drain
    /// the outer window's traces and clear its flag, so it is refused.
    #[test]
    fn audit_guard_clears_in_audit_on_panic() {
        let scene_panics: fn() = || panic!("scene panicked");
        let nests: fn() = || {
            let _ = with_audit(|| {});
        };
        for (body, expected) in [
            (scene_panics, "scene panicked"),
            (nests, "with_audit called inside an open audit window"),
        ] {
            let msg = catch_unwind(AssertUnwindSafe(|| with_audit(body)))
                .expect_err("the body panics")
                .downcast::<&str>()
                .map_or_else(
                    |_| String::from("<non-str panic payload>"),
                    |s| s.to_string(),
                );
            assert_eq!(msg, expected);
            let r = with_audit(|| {});
            assert_eq!(
                r.allocs, 0,
                "post-panic audit saw {} allocs — IN_AUDIT must have been left set",
                r.allocs,
            );
        }
    }

    #[test]
    fn stale_traces_drained_between_audits() {
        let _ = with_audit(|| {
            for _ in 0..3 {
                one_alloc();
            }
        });
        let r = with_audit(|| {});
        assert_eq!(r.traces.len(), 0, "second audit inherited stale traces");
    }
}
