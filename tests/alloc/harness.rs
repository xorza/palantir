//! Frame-loop driver around `Ui` that measures heap allocations
//! attributable to one scene's per-frame work.
//!
//! `run_audit` runs `warmup` frames untracked, then snapshots the
//! global counter, runs `audit` frames, and asserts the delta is
//! within `budget`. Warmup *and* the audited region are serialized
//! via `AUDIT_LOCK` so cargo's parallel test runner can't have one
//! fixture's warmup allocs leak into another's measured window
//! (the counter is process-global, the lock is the only barrier).

use crate::allocator::{Snapshot, delta, snapshot};
use palantir::{Display, Ui};
use std::sync::Mutex;

static AUDIT_LOCK: Mutex<()> = Mutex::new(());

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

    let measured: Snapshot;
    {
        let _guard = AUDIT_LOCK.lock().unwrap();
        for _ in 0..warmup {
            ui.begin_frame(display);
            scene(&mut ui);
            let _ = ui.end_frame();
        }
        let before = snapshot();
        for _ in 0..audit {
            ui.begin_frame(display);
            scene(&mut ui);
            let _ = ui.end_frame();
        }
        measured = delta(before);
    }

    let budget_total = budget.allocs_per_frame * audit as u64;
    let per_frame_allocs = measured.allocs as f64 / audit as f64;
    let per_frame_bytes = measured.bytes as f64 / audit as f64;
    println!(
        "alloc-audit {name}: {per_frame_allocs:.2} allocs/frame, {per_frame_bytes:.0} B/frame \
         (total {} allocs / {} B over {audit} frames after {warmup} warmup)",
        measured.allocs, measured.bytes,
    );

    assert!(
        measured.allocs <= budget_total,
        "alloc budget exceeded for `{name}`: {} allocs over {} frames \
         (budget {}/frame = {} total, {:.2} actual/frame)",
        measured.allocs,
        audit,
        budget.allocs_per_frame,
        budget_total,
        per_frame_allocs,
    );
}
