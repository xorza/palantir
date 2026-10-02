//! Tests for the audit harness itself. Each runs on its own thread
//! (cargo's parallel runner) and exercises the per-thread counter +
//! capture semantics that the fixtures depend on. Sanity-checks the
//! invariants the production fixtures silently rely on:
//! counter correctness, out-of-audit silence, cross-thread isolation,
//! re-entry-guard balance, panic-safety of the audit guard, the
//! panic + trace-dump failure path, and the `user_frames` filter.

use crate::allocator::{TRACE_CAP, with_audit};
use crate::harness;
use crate::harness::{Audit, user_frames};
use palantir::widget::Mesh;
use palantir::{Block, Configure, Panel, Spinner, Ui};
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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    let go = Arc::new(AtomicBool::new(false));
    let g2 = Arc::clone(&go);
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
            .map(|s| s.to_string())
            .unwrap_or_else(|_| String::from("<non-str panic payload>"));
        assert_eq!(msg, expected);
        let r = with_audit(|| {});
        assert_eq!(
            r.allocs, 0,
            "post-panic audit saw {} allocs — IN_AUDIT must have been left set",
            r.allocs,
        );
    }
}

/// Frames allocating 2, 1, 2, 3, 1 sort to 1, 1, 2, 2, 3: the worst is
/// 3, and 1 and 2 tie on two frames each, so the mode is the smaller, 1.
/// Then one more 2 breaks the tie its way.
#[test]
fn report_reads_worst_and_mode() {
    for (counts, mode) in [(&[2, 1, 2, 3, 1][..], 1), (&[2, 1, 2, 3, 1, 2], 2)] {
        let mut frame = 0;
        let report = Audit::new()
            .warmup(0)
            .frames(counts.len())
            .budget(u64::MAX)
            .run_frames(|| {
                for _ in 0..counts[frame] {
                    one_alloc();
                }
                frame += 1;
            });
        assert_eq!(report.worst, 3, "{counts:?}");
        assert_eq!(report.mode, mode, "{counts:?}");
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

/// Each way an audit fails names itself, frame 0 and the caller. The
/// spinner animates paint-only, so after its first frame it skips the
/// scene; the block does not animate, so every frame runs it.
#[test]
fn audit_panics_with_diagnostic_message() {
    let over_budget: fn() = || {
        Audit::new().warmup(0).frames(4).run(|_ui: &mut Ui| {
            one_alloc();
        });
    };
    let skips_scene: fn() = || {
        Audit::new().warmup(4).frames(4).run(|ui: &mut Ui| {
            Spinner::new().id_salt("spin").show(ui);
        });
    };
    let records_paint_only: fn() = || {
        Audit::new()
            .paint_only()
            .warmup(4)
            .frames(4)
            .run(|ui: &mut Ui| {
                Block::new().id_salt("still").size(10.0).show(ui);
            });
    };
    for (audit, expected) in [
        (over_budget, "alloc budget exceeded"),
        (skips_scene, "frame 0/4 (after 4 warmup) skipped the scene"),
        (
            records_paint_only,
            "frame 0/4 (after 4 warmup) ran the scene",
        ),
    ] {
        let msg = catch_unwind(audit)
            .expect_err("the audit should panic")
            .downcast::<String>()
            .map(|s| *s)
            .unwrap_or_else(|_| String::from("<non-string panic payload>"));
        assert!(
            msg.contains(expected),
            "panic message missing {expected:?}: {msg}"
        );
        // The fixture names itself by where it is, not by a string it
        // repeats: `Audit::run` is `#[track_caller]`, so the location is
        // this file.
        assert!(
            msg.contains(file!()),
            "panic message missing the caller's location: {msg}",
        );
    }
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
    //
    // The allocation is one palantir makes by contract, recorded inside a
    // frame so the stack runs through the frame path as well; the primed
    // frame around it allocates nothing, so it is the first trace.
    let scene = |ui: &mut Ui| {
        Panel::vstack().auto_id().show(ui, |_| {
            black_box(Mesh::with_capacity(4, 6));
        });
    };
    let mut ui = harness::new_ui();
    ui.prime(4, scene);
    let r = with_audit(|| {
        let _ = ui.frame(scene);
    });
    assert_eq!(r.allocs, 2, "the mesh's vertex and index buffers");
    let mut bt = r.traces.into_iter().next().expect("two traces");
    let rendered = user_frames(&mut bt);

    let first_at = rendered
        .lines()
        .find_map(|line| line.trim().strip_prefix("at "))
        .expect("at least one kept frame");
    assert!(
        first_at.starts_with("src/primitives/mesh/mod.rs:"),
        "the innermost kept frame is the allocating constructor:\n{rendered}",
    );
    assert!(
        rendered.contains("at src/ui/"),
        "the frame path above the scene is kept too:\n{rendered}",
    );
    for plumbing in [
        "tests/alloc/allocator.rs",
        "tests/alloc/harness/mod.rs",
        "tests/alloc/harness/format.rs",
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
