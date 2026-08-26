//! Frame-loop drivers around `Ui` that measure heap allocations
//! attributable to one scene's per-frame work.
//!
//! Three entry points:
//! - [`run_audit`] takes an explicit `warmup` count — use when you need
//!   precise control or are debugging the harness itself.
//! - [`audit_steady_state`] probes for a stable point on its own and
//!   audits a fixed window after that — use for new fixtures so you
//!   don't have to hand-tune warmup numbers per scene.
//! - [`run_audit_with_ui`] is [`run_audit`] over a harness the caller
//!   raised, which is what `gates.rs` needs.
//!
//! None is told which fixture it is auditing: all three are
//! `#[track_caller]`, so the call site names itself, and cargo prints
//! the failing test's own name above whatever it captured.
//!
//! All run inside [`with_audit`] so per-thread counters + backtrace
//! capture stay scoped to the measured window. The counter is
//! per-thread (see `allocator.rs`), so cargo's parallel test runner
//! can't pollute one fixture's window with another's allocations —
//! no global lock needed.

mod format;

#[cfg(test)]
pub(crate) use format::user_frames;

use std::panic::Location;

use crate::allocator::{AuditResult, with_audit};
use palantir::Ui;
use palantir::internals::UiHarness;

/// Logical display every audit runs at — `UiHarness`'s own defaults
/// (scale 1.0, pixel-snapped, no refresh rate) at 800×600.
const SURFACE: glam::UVec2 = glam::UVec2::new(800, 600);

/// Mono-fallback harness for the alloc audits: private arena, fresh
/// caches, no font loading — exactly what these GPU-less tests want.
pub(crate) fn new_ui() -> UiHarness {
    UiHarness::new(SURFACE)
}

/// Run `scene` for `warmup` frames untracked, then audit each of
/// `audit` frames individually. Fails as soon as a single frame
/// exceeds `max_allocs`, dumping that frame's captured backtraces.
#[track_caller]
pub(crate) fn run_audit<S>(warmup: usize, audit: usize, max_allocs: u64, scene: S)
where
    S: FnMut(&mut Ui),
{
    run_audit_with_ui(warmup, audit, max_allocs, new_ui(), scene);
}

/// [`run_audit`] against a caller-supplied harness. The gates in
/// `gates.rs` render the frame bench's own surface and dpr, which is
/// exactly what [`new_ui`]'s small defaults are not.
#[track_caller]
pub(crate) fn run_audit_with_ui<S>(
    warmup: usize,
    audit: usize,
    max_allocs: u64,
    mut ui: UiHarness,
    mut scene: S,
) where
    S: FnMut(&mut Ui),
{
    assert!(audit > 0, "audit frame count must be > 0");
    let at = Location::caller();

    for _ in 0..warmup {
        run_frame(&mut ui, &mut scene);
    }

    for i in 0..audit {
        let result = with_audit(|| run_frame(&mut ui, &mut scene));
        if result.allocs > max_allocs {
            fail_audit(at, i, audit, warmup, max_allocs, result);
        }
    }

    println!(
        "alloc-audit {at}: 0..={max_allocs} allocs/frame over {audit} frames \
         after {warmup} warmup",
    );
}

/// Probes up to `MAX_WARMUP` frames; once `STABLE_RUN` consecutive
/// frames stay within `max_allocs`, the warmup phase ends and the
/// audit window starts. Then audits each of `AUDIT_FRAMES` frames
/// individually — any frame over budget fails.
///
/// Use this for new fixtures so you don't have to eyeball a warmup count.
///
/// **Not for a scene that cycles.** The probe settles as soon as it sees
/// `STABLE_RUN` quiet frames, and it can find those *within* one cycle —
/// before the widest frame of that cycle has ever been recorded. The audit
/// window then meets that frame's one-off growth and reads it as a per-frame
/// cost. A scene whose work varies from frame to frame wants [`run_audit`]
/// with a warmup counted in whole cycles, which is what the churn fixtures do.
#[track_caller]
pub(crate) fn audit_steady_state<S>(max_allocs: u64, mut scene: S)
where
    S: FnMut(&mut Ui),
{
    audit_steady_state_with_ui(Location::caller(), max_allocs, new_ui(), &mut scene);
}

/// Cosmic-text counterpart used when a fixture must exercise real
/// multi-line shaping rather than the mono fallback.
#[track_caller]
pub(crate) fn audit_text_steady_state<S>(max_allocs: u64, mut scene: S)
where
    S: FnMut(&mut Ui),
{
    audit_steady_state_with_ui(
        Location::caller(),
        max_allocs,
        UiHarness::with_text(SURFACE),
        &mut scene,
    );
}

fn audit_steady_state_with_ui<S>(
    at: &'static Location<'static>,
    max_allocs: u64,
    mut ui: UiHarness,
    scene: &mut S,
) where
    S: FnMut(&mut Ui),
{
    const MAX_WARMUP: usize = 8;
    const STABLE_RUN: usize = 2;
    const AUDIT_FRAMES: usize = 64;

    let mut warmup = 0usize;
    let mut stable = 0usize;
    while warmup < MAX_WARMUP {
        let r = with_audit(|| run_frame(&mut ui, scene));
        warmup += 1;
        stable = if r.allocs <= max_allocs {
            stable + 1
        } else {
            0
        };
        if stable >= STABLE_RUN {
            break;
        }
    }

    println!("alloc-audit {at}: warmup={warmup} (stable_run={stable})");

    for i in 0..AUDIT_FRAMES {
        let result = with_audit(|| run_frame(&mut ui, scene));
        if result.allocs > max_allocs {
            fail_audit(at, i, AUDIT_FRAMES, warmup, max_allocs, result);
        }
    }
}

#[inline]
fn run_frame<S: FnMut(&mut Ui)>(ui: &mut UiHarness, scene: &mut S) {
    let _ = ui.frame(scene);
}

pub(crate) fn fail_audit(
    at: &'static Location<'static>,
    frame_idx: usize,
    audit: usize,
    warmup: usize,
    max_allocs: u64,
    mut result: AuditResult,
) -> ! {
    eprintln!(
        "alloc-audit {at}: frame {frame_idx}/{audit} (after {warmup} warmup) \
         allocated {} times, {} B — budget is {max_allocs}/frame",
        result.allocs, result.bytes,
    );
    for (i, bt) in result.traces.iter_mut().enumerate() {
        eprintln!("--- alloc #{i} backtrace ---\n{}", format::user_frames(bt));
    }
    eprintln!("(set PALANTIR_ALLOC_FULL_BT=1 to disable user-code filtering and see full stacks)");
    panic!(
        "alloc budget exceeded at {at} on frame {frame_idx} \
         (budget {max_allocs}/frame, got {})",
        result.allocs,
    );
}
