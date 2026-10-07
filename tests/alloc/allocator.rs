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

/// The counter, capture and guard: exact counts, silence outside a window,
/// sibling-thread isolation, bookkeeping kept out of the count, and a guard that
/// survives a panicking body.
#[cfg(test)]
mod tests {
    use crate::allocator::{TRACE_CAP, with_audit};
    use std::hint;
    use std::hint::black_box;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;

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
        // Spawn the worker *before* the audit (spawn allocates on the caller); a
        // start flag releases its burst inside the window. Not `Barrier`: on macOS
        // its first `wait` lazily heap-allocates on the auditing thread.
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
        // Bookkeeping (`TRACES` growth, Backtrace internals) re-enters the
        // allocator and `CAPTURING` must suppress it; the above-cap row proves the
        // bound leaves the count alone.
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

    /// A panic in `with_audit`'s body must leave `IN_AUDIT` cleared for the next
    /// audit. A nested window is one such panic, so it is refused.
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
