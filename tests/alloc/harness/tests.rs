//! The audit driver and its report: the worst and the mode, the
//! failures that name themselves, and the `user_frames` filter.

use crate::allocator::with_audit;
use crate::harness;
use crate::harness::{Audit, user_frames};
use palantir::widget::Mesh;
use palantir::{Block, Configure, Panel, Spinner, Ui};
use std::hint::black_box;
use std::panic::catch_unwind;

/// Force one heap alloc that the optimizer can't hoist or elide.
fn one_alloc() {
    black_box(Box::new(black_box(0u64)));
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
        first_at.starts_with("src/primitives/geometry/mesh/mod.rs:"),
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
        "tests/alloc/harness/tests.rs",
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
