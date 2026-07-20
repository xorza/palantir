//! Frame-loop drivers around `Ui` that measure heap allocations
//! attributable to one scene's per-frame work.
//!
//! Two entry points:
//! - [`run_audit`] takes an explicit `warmup` count — use when you need
//!   precise control or are debugging the harness itself.
//! - [`audit_steady_state`] probes for a stable point on its own and
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
use aperture::{Display, FrameReport, Ui};
use std::time::Duration;

/// Mono-fallback `Ui` for the alloc audits — `Ui::default` is the
/// self-contained constructor (mono shaper + private arena + fresh
/// caches), exactly what these GPU-less tests want.
pub(crate) fn new_ui() -> Ui {
    Ui::default()
}

const DISPLAY: Display = Display {
    physical: glam::UVec2::new(800, 600),
    scale_factor: 1.0,
    pixel_snap: true,
    refresh_millihertz: None,
};

pub(crate) fn record(
    ui: &mut Ui,
    display: Display,
    time: Duration,
    record: impl FnMut(&mut Ui),
) -> FrameReport {
    ui.record_test_frame(display, time, record)
}

/// Run `scene` for `warmup` frames untracked, then audit each of
/// `audit` frames individually. Fails as soon as a single frame
/// exceeds `max_allocs`, dumping that frame's captured backtraces.
pub(crate) fn run_audit<S>(name: &str, warmup: usize, audit: usize, max_allocs: u64, mut scene: S)
where
    S: FnMut(&mut Ui),
{
    assert!(audit > 0, "audit frame count must be > 0");

    let mut ui = new_ui();

    for _ in 0..warmup {
        run_frame(&mut ui, &mut scene);
    }

    for i in 0..audit {
        let result = with_audit(|| run_frame(&mut ui, &mut scene));
        if result.allocs > max_allocs {
            fail_audit(name, i, audit, warmup, max_allocs, result);
        }
    }

    println!(
        "alloc-audit {name}: 0..={max_allocs} allocs/frame over {audit} frames \
         after {warmup} warmup",
    );
}

/// Probes up to `MAX_WARMUP` frames; once `STABLE_RUN` consecutive
/// frames stay within `max_allocs`, the warmup phase ends and the
/// audit window starts. Then audits each of `AUDIT_FRAMES` frames
/// individually — any frame over budget fails.
///
/// Use this for new fixtures so you don't have to eyeball a warmup count.
pub(crate) fn audit_steady_state<S>(name: &str, max_allocs: u64, mut scene: S)
where
    S: FnMut(&mut Ui),
{
    audit_steady_state_with_ui(name, max_allocs, new_ui(), &mut scene);
}

/// Cosmic-text counterpart used when a fixture must exercise real
/// multi-line shaping rather than the mono fallback.
pub(crate) fn audit_text_steady_state<S>(name: &str, max_allocs: u64, mut scene: S)
where
    S: FnMut(&mut Ui),
{
    let ui = Ui::for_test_text();
    audit_steady_state_with_ui(name, max_allocs, ui, &mut scene);
}

fn audit_steady_state_with_ui<S>(name: &str, max_allocs: u64, mut ui: Ui, scene: &mut S)
where
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

    println!("alloc-audit {name}: warmup={warmup} (stable_run={stable})");

    for i in 0..AUDIT_FRAMES {
        let result = with_audit(|| run_frame(&mut ui, scene));
        if result.allocs > max_allocs {
            fail_audit(name, i, AUDIT_FRAMES, warmup, max_allocs, result);
        }
    }
}

#[inline]
fn run_frame<S: FnMut(&mut Ui)>(ui: &mut Ui, scene: &mut S) {
    let _ = record(ui, DISPLAY, Duration::ZERO, scene);
}

fn fail_audit(
    name: &str,
    frame_idx: usize,
    audit: usize,
    warmup: usize,
    max_allocs: u64,
    mut result: AuditResult,
) -> ! {
    eprintln!(
        "alloc-audit {name}: frame {frame_idx}/{audit} (after {warmup} warmup) \
         allocated {} times, {} B — budget is {max_allocs}/frame",
        result.allocs, result.bytes,
    );
    for (i, bt) in result.traces.iter_mut().enumerate() {
        eprintln!("--- alloc #{i} backtrace ---\n{}", format::user_frames(bt));
    }
    eprintln!("(set APERTURE_ALLOC_FULL_BT=1 to disable user-code filtering and see full stacks)");
    panic!(
        "alloc budget exceeded for `{name}` on frame {frame_idx} \
         (budget {max_allocs}/frame, got {})",
        result.allocs,
    );
}
