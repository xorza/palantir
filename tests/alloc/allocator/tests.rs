//! The counter, capture and guard: exact counts, silence outside a window, sibling-thread isolation, bookkeeping kept out of the count, and a guard that survives a panicking body.

use crate::allocator;
use crate::allocator::TRACE_CAP;
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
    let r = allocator::with_audit(|| {
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
    let r = allocator::with_audit(|| {});
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

    let r = allocator::with_audit(|| {
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
        let r = allocator::with_audit(|| {
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
        let _ = allocator::with_audit(|| {});
    };
    for (body, expected) in [
        (scene_panics, "scene panicked"),
        (nests, "with_audit called inside an open audit window"),
    ] {
        let msg = catch_unwind(AssertUnwindSafe(|| allocator::with_audit(body)))
            .expect_err("the body panics")
            .downcast::<&str>()
            .map_or_else(
                |_| String::from("<non-str panic payload>"),
                |s| s.to_string(),
            );
        assert_eq!(msg, expected);
        let r = allocator::with_audit(|| {});
        assert_eq!(
            r.allocs, 0,
            "post-panic audit saw {} allocs — IN_AUDIT must have been left set",
            r.allocs,
        );
    }
}

#[test]
fn stale_traces_drained_between_audits() {
    let _ = allocator::with_audit(|| {
        for _ in 0..3 {
            one_alloc();
        }
    });
    let r = allocator::with_audit(|| {});
    assert_eq!(r.traces.len(), 0, "second audit inherited stale traces");
}
