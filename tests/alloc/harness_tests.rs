//! Tests for the audit harness itself. Each runs on its own thread
//! (cargo's parallel runner) and exercises the per-thread counter +
//! capture semantics that the fixtures depend on. Sanity-checks the
//! invariants the production fixtures silently rely on:
//! counter correctness, out-of-audit silence, cross-thread isolation,
//! re-entry-guard balance, and the panic + trace-dump failure path.

use crate::allocator::{delta, set_in_audit, snapshot, take_traces};
use crate::harness::{AllocBudget, run_audit};
use palantir::Ui;
use std::hint::black_box;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Force one heap alloc that the optimizer can't hoist or elide.
fn one_alloc() {
    black_box(Box::new(black_box(0u64)));
}

#[test]
fn counts_exactly_what_audit_window_allocates() {
    let _ = take_traces();
    let before = snapshot();
    set_in_audit(true);
    for _ in 0..5 {
        one_alloc();
    }
    set_in_audit(false);
    let d = delta(before);
    assert_eq!(d.allocs, 5, "expected 5 allocs in the audited window");
    assert!(
        d.bytes >= 5 * 8,
        "bytes should cover 5×u64, got {}",
        d.bytes
    );
}

#[test]
fn allocs_outside_audit_are_silent() {
    let before = snapshot();
    for _ in 0..32 {
        one_alloc();
    }
    let d = delta(before);
    assert_eq!(
        d.allocs, 0,
        "non-audited allocs must not count, got {}",
        d.allocs,
    );
}

#[test]
fn sibling_thread_allocs_do_not_pollute_audit() {
    // Spawn the worker *before* entering audit (thread::spawn allocates
    // on the caller). A barrier synchronizes the worker's allocation
    // burst with our audit window so per-thread isolation is what's
    // actually being tested.
    use std::sync::{Arc, Barrier};
    let _ = take_traces();
    let barrier = Arc::new(Barrier::new(2));
    let b2 = barrier.clone();
    let t = std::thread::spawn(move || {
        b2.wait();
        for _ in 0..1_000 {
            one_alloc();
        }
        b2.wait();
    });

    let before = snapshot();
    set_in_audit(true);
    barrier.wait();
    barrier.wait();
    set_in_audit(false);
    let d = delta(before);
    t.join().unwrap();

    assert_eq!(
        d.allocs, 0,
        "sibling thread's 1000 allocs leaked into our delta (got {})",
        d.allocs,
    );
}

#[test]
fn re_entry_guard_keeps_counter_and_traces_aligned() {
    // The bookkeeping path (Vec growth in TRACES, Backtrace::capture
    // internals) calls back into the allocator. CAPTURING must
    // suppress those, so traces.len() == counter.allocs even after
    // the very first alloc (when TRACES allocates its initial buffer)
    // and after capacity-doubling pushes.
    let _ = take_traces();
    let before = snapshot();
    set_in_audit(true);
    for _ in 0..64 {
        one_alloc();
    }
    set_in_audit(false);
    let d = delta(before);
    let traces = take_traces();
    assert_eq!(
        d.allocs as usize,
        traces.len(),
        "trace count must equal alloc count (counter={}, traces={})",
        d.allocs,
        traces.len(),
    );
}

#[test]
fn take_traces_drains() {
    let _ = take_traces();
    set_in_audit(true);
    one_alloc();
    set_in_audit(false);
    assert_eq!(take_traces().len(), 1);
    assert_eq!(take_traces().len(), 0, "second call should return empty");
}

#[test]
fn run_audit_panics_with_diagnostic_message_on_budget_violation() {
    let result = catch_unwind(AssertUnwindSafe(|| {
        run_audit(
            "synthetic_overshoot",
            0,
            4,
            AllocBudget::ZERO,
            |_ui: &mut Ui| {
                one_alloc();
            },
        );
    }));
    let msg = result
        .expect_err("run_audit should panic when budget exceeded")
        .downcast::<String>()
        .map(|s| *s)
        .unwrap_or_else(|_| String::from("<non-string panic payload>"));
    assert!(
        msg.contains("alloc budget exceeded for `synthetic_overshoot`"),
        "panic message missing diagnostic header: {msg}",
    );
    assert!(
        msg.contains("4 allocs over 4 frames"),
        "panic message missing per-frame summary: {msg}",
    );
}

#[test]
fn captured_traces_include_caller_after_resolve() {
    let _ = take_traces();
    set_in_audit(true);
    one_alloc();
    set_in_audit(false);
    let mut traces = take_traces();
    assert_eq!(traces.len(), 1);
    traces[0].resolve();
    let dump = format!("{:?}", traces[0]);
    assert!(
        dump.contains("harness_tests"),
        "resolved backtrace should mention the test module, got:\n{dump}",
    );
}
