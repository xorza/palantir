//! Tests for the audit harness itself. Each runs on its own thread
//! (cargo's parallel runner) and exercises the per-thread counter +
//! capture semantics that the fixtures depend on. Sanity-checks the
//! invariants the production fixtures silently rely on:
//! counter correctness, out-of-audit silence, cross-thread isolation,
//! re-entry-guard balance, panic-safety of the audit guard, the
//! panic + trace-dump failure path, and the `user_frames` filter.

use crate::allocator::with_audit;
use crate::harness::{AllocBudget, run_audit, user_frames};
use palantir::{Button, Configure, Display, Sizing, Ui};
use std::hint::black_box;
use std::panic::{AssertUnwindSafe, catch_unwind};

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
    assert!(
        r.bytes >= 5 * 8,
        "bytes should cover 5×u64, got {}",
        r.bytes
    );
}

#[test]
fn allocs_outside_audit_are_silent() {
    // Allocate before; allocate after; the inner audit sees zero.
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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    let go = Arc::new(AtomicBool::new(false));
    let g2 = go.clone();
    let t = std::thread::spawn(move || {
        while !g2.load(Ordering::Acquire) {
            std::hint::spin_loop();
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
    // calls back into the allocator. CAPTURING must suppress those, so
    // `traces.len() == counter.allocs` even after the very first alloc
    // (when TRACES allocates its initial buffer) and after capacity-
    // doubling pushes.
    let r = with_audit(|| {
        for _ in 0..64 {
            one_alloc();
        }
    });
    assert_eq!(
        r.allocs as usize,
        r.traces.len(),
        "trace count must equal alloc count (counter={}, traces={})",
        r.allocs,
        r.traces.len(),
    );
}

#[test]
fn audit_guard_clears_in_audit_on_panic() {
    // If `with_audit`'s body panics, the guard's Drop must clear
    // IN_AUDIT so a follow-up `with_audit` on this thread starts
    // clean. Without the guard the flag would stay stuck and the
    // post-panic audit would inherit allocations from the unwinding
    // path (drop glue, panic reporting, etc.).
    let _ = catch_unwind(AssertUnwindSafe(|| {
        with_audit(|| panic!("scene panicked"));
    }));
    let r = with_audit(|| {});
    assert_eq!(
        r.allocs, 0,
        "post-panic audit saw {} allocs — IN_AUDIT must have been left set",
        r.allocs,
    );
}

#[test]
fn stale_traces_drained_between_audits() {
    // Two back-to-back audits on the same thread: the second should
    // not see the first's traces.
    let _ = with_audit(|| {
        for _ in 0..3 {
            one_alloc();
        }
    });
    let r = with_audit(|| {});
    assert_eq!(r.traces.len(), 0, "second audit inherited stale traces");
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
        msg.contains("alloc budget exceeded"),
        "panic message missing diagnostic header: {msg}",
    );
    assert!(
        msg.contains("synthetic_overshoot"),
        "panic message missing fixture name: {msg}",
    );
}

#[test]
fn user_frames_keeps_palantir_src_and_excludes_harness_internals() {
    // Provoke a real palantir frame stack so the filter has both
    // `src/...` and `tests/alloc/...` candidates to choose between.
    // The rendered output must:
    //   - include `src/...` frames (the bug source we want to surface),
    //   - exclude every `tests/alloc/` path — including this test
    //     module, since it's harness machinery, not a fixture,
    //   - drop the `alloc::` test-binary-crate prefix.
    let display = Display::from_physical(glam::UVec2::new(800, 600), 1.0);
    let mut ui = Ui::new();
    // Warm caches so we audit a steady-state alloc, not first-frame init.
    for _ in 0..4 {
        let _ = ui.run_frame(display, |ui| {
            Button::new()
                .auto_id()
                .label("hello")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui);
        });
    }
    let r = with_audit(|| {
        let _ = ui.run_frame(display, |ui| {
            Button::new()
                .auto_id()
                .label("hello")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui);
        });
    });
    let mut bt = r
        .traces
        .into_iter()
        .next()
        .expect("button render should produce at least one captured alloc");
    let rendered = user_frames(&mut bt);

    assert!(
        rendered.contains("src/"),
        "rendered frames should include palantir src/ frames:\n{rendered}",
    );
    for plumbing in [
        "tests/alloc/allocator.rs",
        "tests/alloc/harness.rs",
        "tests/alloc/harness_tests.rs",
        "tests/alloc/main.rs",
    ] {
        assert!(
            !rendered.contains(plumbing),
            "rendered frames leaked harness path `{plumbing}`:\n{rendered}",
        );
    }
    assert!(
        !rendered.contains("alloc::"),
        "rendered frames retained `alloc::` test-crate prefix:\n{rendered}",
    );
}
