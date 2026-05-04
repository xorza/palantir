//! Frame-loop drivers around `Ui` that measure heap allocations
//! attributable to one scene's per-frame work.
//!
//! Two entry points:
//! - [`run_audit`] takes an explicit `warmup` count — use when you need
//!   precise control or are debugging the harness itself.
//! - [`audit_until_stable`] probes for a stable point on its own and
//!   audits a fixed window after that — use for new fixtures so you
//!   don't have to hand-tune warmup numbers per scene.
//!
//! Both run inside [`with_audit`] so per-thread counters + backtrace
//! capture stay scoped to the measured window. The counter is
//! per-thread (see `allocator.rs`), so cargo's parallel test runner
//! can't pollute one fixture's window with another's allocations —
//! no global lock needed.

mod format;

#[cfg(test)]
pub(crate) use format::user_frames;

use crate::allocator::{AuditResult, with_audit};
use palantir::{Display, Ui};

#[derive(Clone, Copy, Debug)]
pub(crate) struct AllocBudget {
    pub(crate) allocs_per_frame: u64,
}

impl AllocBudget {
    pub(crate) const ZERO: Self = Self {
        allocs_per_frame: 0,
    };
}

const DISPLAY_PHYSICAL_PX: glam::UVec2 = glam::UVec2::new(800, 600);
const DISPLAY_SCALE: f32 = 1.0;

fn display() -> Display {
    Display::from_physical(DISPLAY_PHYSICAL_PX, DISPLAY_SCALE)
}

/// Run `scene` for `warmup` frames untracked, then for `audit` frames
/// inside `with_audit`. Asserts the captured allocation count is
/// within `budget`. On violation, prints filtered backtraces for
/// every alloc seen during the audit window and panics with a
/// diagnostic header.
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

    let display = display();
    let mut ui = Ui::new();

    for _ in 0..warmup {
        run_frame(&mut ui, display, &mut scene);
    }

    let result = with_audit(|| {
        for _ in 0..audit {
            run_frame(&mut ui, display, &mut scene);
        }
    });

    finish_audit(name, audit, warmup, budget, result);
}

/// Like `run_audit` but discovers the warmup point on its own. Probes
/// up to `MAX_PROBE` frames; the warmup phase ends as soon as
/// `STABLE_RUN` consecutive frames have stayed within `budget`. The
/// audit window is always `AUDIT_FRAMES` frames after that.
///
/// Use this for new fixtures — eliminates the eyeballed-warmup
/// problem (caches in `Ui` settle at different rates depending on the
/// scene; a hardcoded warmup either over-pads passing tests or
/// under-pads scenes whose scratch Vecs grow at non-power-of-two
/// thresholds).
///
/// Panics with a diagnostic if no stable run is found in `MAX_PROBE`
/// frames — that's a real regression, not a flake.
pub(crate) fn audit_until_stable<S>(name: &str, budget: AllocBudget, mut scene: S)
where
    S: FnMut(&mut Ui),
{
    const STABLE_RUN: usize = 8;
    const MAX_PROBE: usize = 256;
    const AUDIT_FRAMES: usize = 64;

    let display = display();
    let mut ui = Ui::new();

    let mut consecutive = 0usize;
    let mut warmup = 0usize;
    while consecutive < STABLE_RUN {
        if warmup >= MAX_PROBE {
            panic!(
                "alloc audit `{name}`: no stable run of {STABLE_RUN} frames \
                 within budget of {} alloc(s)/frame in {MAX_PROBE} probe frames",
                budget.allocs_per_frame,
            );
        }
        let r = with_audit(|| run_frame(&mut ui, display, &mut scene));
        if r.allocs <= budget.allocs_per_frame {
            consecutive += 1;
        } else {
            consecutive = 0;
        }
        warmup += 1;
    }

    let result = with_audit(|| {
        for _ in 0..AUDIT_FRAMES {
            run_frame(&mut ui, display, &mut scene);
        }
    });

    finish_audit(name, AUDIT_FRAMES, warmup, budget, result);
}

#[inline]
fn run_frame<S: FnMut(&mut Ui)>(ui: &mut Ui, display: Display, scene: &mut S) {
    ui.begin_frame(display);
    scene(ui);
    let _ = ui.end_frame();
}

fn finish_audit(
    name: &str,
    audit: usize,
    warmup: usize,
    budget: AllocBudget,
    mut result: AuditResult,
) {
    let budget_total = budget.allocs_per_frame * audit as u64;
    let per_frame_allocs = result.allocs as f64 / audit as f64;
    let per_frame_bytes = result.bytes as f64 / audit as f64;
    println!(
        "alloc-audit {name}: {per_frame_allocs:.2} allocs/frame, {per_frame_bytes:.0} B/frame \
         (total {} allocs / {} B over {audit} frames after {warmup} warmup)",
        result.allocs, result.bytes,
    );

    if result.allocs > budget_total {
        for (i, bt) in result.traces.iter_mut().enumerate() {
            eprintln!("--- alloc #{i} backtrace ---\n{}", format::user_frames(bt));
        }
        eprintln!(
            "(set PALANTIR_ALLOC_FULL_BT=1 to disable user-code filtering and see full stacks)",
        );
        panic!(
            "alloc budget exceeded for `{name}` (budget {}/frame)",
            budget.allocs_per_frame,
        );
    }
}
