//! Frame-loop driver around `Ui` that measures heap allocations
//! attributable to one scene's per-frame work.
//!
//! `run_audit` runs `warmup` frames untracked, snapshots the
//! per-thread counter, runs `audit` frames, and asserts the delta is
//! within `budget`. The counter is per-thread (see `allocator.rs`),
//! so cargo's parallel test runner can't pollute one fixture's
//! window with another's allocations — no global lock needed.
//! With `RUST_BACKTRACE=1`, every audit-window alloc is captured
//! and dumped on budget failure.

use crate::allocator::{delta, set_in_audit, snapshot, take_traces};
use palantir::{Display, Ui};
use std::backtrace::BacktraceStatus;

#[derive(Clone, Copy, Debug)]
pub(crate) struct AllocBudget {
    pub(crate) allocs_per_frame: u64,
}

impl AllocBudget {
    pub(crate) const ZERO: Self = Self {
        allocs_per_frame: 0,
    };
}

pub(crate) fn run_audit<S>(
    name: &str,
    warmup: usize,
    audit: usize,
    budget: AllocBudget,
    mut scene: S,
) where
    S: FnMut(&mut Ui),
{
    assert!(audit > 0, "audit frame count must be > 0");

    let display = Display::from_physical(glam::UVec2::new(800, 600), 1.0);
    let mut ui = Ui::new();

    for _ in 0..warmup {
        ui.begin_frame(display);
        scene(&mut ui);
        let _ = ui.end_frame();
    }
    let before = snapshot();
    set_in_audit(true);
    for _ in 0..audit {
        ui.begin_frame(display);
        scene(&mut ui);
        let _ = ui.end_frame();
    }
    set_in_audit(false);
    let measured = delta(before);
    let traces = take_traces();

    let budget_total = budget.allocs_per_frame * audit as u64;
    let per_frame_allocs = measured.allocs as f64 / audit as f64;
    let per_frame_bytes = measured.bytes as f64 / audit as f64;
    println!(
        "alloc-audit {name}: {per_frame_allocs:.2} allocs/frame, {per_frame_bytes:.0} B/frame \
         (total {} allocs / {} B over {audit} frames after {warmup} warmup)",
        measured.allocs, measured.bytes,
    );

    if measured.allocs > budget_total {
        let traces_disabled = traces
            .iter()
            .any(|b| matches!(b.status(), BacktraceStatus::Disabled));
        if traces_disabled {
            eprintln!(
                "captured {} allocs but backtraces disabled — re-run with RUST_BACKTRACE=1 \
                 to see call sites",
                traces.len(),
            );
        } else {
            for (i, bt) in traces.iter().enumerate() {
                eprintln!("--- alloc #{i} backtrace ---\n{bt}");
            }
        }
        panic!(
            "alloc budget exceeded for `{name}`: {} allocs over {} frames \
             (budget {}/frame = {} total, {:.2} actual/frame)",
            measured.allocs, audit, budget.allocs_per_frame, budget_total, per_frame_allocs,
        );
    }
}
