//! Frame-loop drivers around `Ui` that measure heap allocations
//! attributable to one scene's per-frame work.
//!
//! [`Audit`] is the single way in: it carries how long to warm the scene
//! up, how many frames to measure, and what each of those frames may
//! spend. [`Audit::run`] raises a `UiHarness` and drives the scene
//! through it; anything that renders frames its own way drives
//! [`Audit::run_frames`] instead — the renderer fixtures and the
//! full-tree gate through a `FrontendHarness`, the device gates through
//! [`OffscreenTarget`].
//!
//! Both terminals are `#[track_caller]`, so the call site names itself
//! and cargo prints the failing test's own name above whatever it
//! captured — no audit is ever told which fixture it is auditing.
//!
//! Both run inside [`with_audit`], so per-thread counters and backtrace
//! capture stay scoped to the measured window. The counter is per-thread
//! (see `allocator.rs`), so cargo's parallel test runner cannot pollute
//! one fixture's window with another's allocations — no global lock
//! needed.

mod format;
mod offscreen;

pub(crate) use format::user_frames;
pub(crate) use offscreen::OffscreenTarget;

use std::panic::Location;

use glam::UVec2;
use palantir::Ui;
use palantir::internals::{PROBATION_KEEP_FRAMES, SHAPED_BUFFER_RING_FRAMES, UiHarness};

use crate::allocator::{AuditResult, with_audit};

/// Logical display every fixture renders at: `UiHarness`'s own defaults
/// (scale 1.0, pixel-snapped, no refresh rate) at 800×600. One number
/// for [`Audit::run`] and [`new_ui`] alike, so a fixture moved between
/// the two keeps its scene the size it was written for.
const SURFACE: UVec2 = UVec2::new(800, 600);

/// Mono-fallback harness for the alloc audits: private arena, fresh
/// caches, no font loading — exactly what these GPU-less tests want.
pub(crate) fn new_ui() -> UiHarness {
    UiHarness::new(SURFACE)
}

/// How the warmup phase ends.
#[derive(Clone, Copy, Debug)]
enum Warmup {
    /// Stop once `STABLE_RUN` consecutive frames land inside the budget,
    /// giving up at `MAX_WARMUP`. Right for any scene that settles, and
    /// it saves hand-tuning a count per fixture.
    ///
    /// **Wrong for a scene that cycles.** The probe settles as soon as
    /// it sees two quiet frames, and it can find those *within* one
    /// cycle — before the widest frame of that cycle has ever been
    /// recorded. The measured window then meets that frame's one-off
    /// growth and reads it as a per-frame cost. Such a scene wants
    /// [`Audit::warmup`] with a count in whole cycles, which is what the
    /// churn fixtures do.
    Probe,
    /// Exactly this many frames, warmed without any budget check.
    Fixed(usize),
}

/// One allocation audit: how to raise the scene, how long to warm it,
/// how many frames to measure, and what each of those frames may spend.
///
/// The defaults are what a new fixture wants — the probe, 64 measured
/// frames, a strict-zero budget — so `Audit::new().run(scene)` is the
/// whole call for most of them.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Audit {
    text: bool,
    warmup: Warmup,
    frames: usize,
    budget: u64,
    paint_only: bool,
}

/// What an audit observed.
///
/// The worst frame is the number that matters to a budget: it says how
/// much slack one has, and the harness printing it is what keeps a
/// fixture from carrying a hand-recorded measurement nothing rechecks.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Report {
    pub(crate) worst: u64,
    /// The count most measured frames read, the smaller on a tie. A
    /// driver's floor is flat but for a rare frame the pool it draws on
    /// missed, and the mode is the flat value those frames leave alone.
    pub(crate) mode: u64,
}

impl Audit {
    pub(crate) fn new() -> Self {
        Audit {
            text: false,
            warmup: Warmup::Probe,
            frames: 64,
            budget: 0,
            paint_only: false,
        }
    }

    /// Real cosmic shaping instead of the mono fallback, for a fixture
    /// that has to exercise it — warmed and measured in one revolution
    /// of the shaped-buffer expiry ring each.
    ///
    /// A bucket's first drain grows the wheel's scratch, and the probe's
    /// quiet frames come long before the widest bucket of the first
    /// revolution is due; a cost the ring pays once a revolution would
    /// fall outside any shorter window.
    pub(crate) fn text(mut self) -> Self {
        let ring = SHAPED_BUFFER_RING_FRAMES as usize;
        self.text = true;
        self.warmup = Warmup::Fixed(ring);
        self.frames = ring;
        self
    }

    /// A fixed warmup in place of the probe — see [`Warmup::Probe`] for
    /// the scene shape that needs one.
    pub(crate) fn warmup(mut self, frames: usize) -> Self {
        self.warmup = Warmup::Fixed(frames);
        self
    }

    pub(crate) fn frames(mut self, frames: usize) -> Self {
        self.frames = frames;
        self
    }

    /// What one measured frame may allocate. Zero unless said otherwise;
    /// a non-zero budget pins flatness rather than absence, so it is a
    /// ceiling a cost that grew with the frame count would blow through.
    pub(crate) fn budget(mut self, allocs: u64) -> Self {
        self.budget = allocs;
        self
    }

    /// Measure the frames that repaint from the retained tree without
    /// running the scene — an idle animation's steady state.
    ///
    /// Without this, every measured frame must run the scene: a frame
    /// that skips it measures nothing the fixture wrote, and a paint-only
    /// animation anywhere in the scene skips it on every frame after the
    /// first. A scene that has one and still wants its record measured
    /// calls `Ui::request_repaint` each frame, as an app that redraws
    /// continuously does.
    pub(crate) fn paint_only(mut self) -> Self {
        self.paint_only = true;
        self
    }

    /// Drive `scene` through a `UiHarness` raised from [`Self::text`],
    /// every measured frame checked against [`Self::paint_only`].
    #[track_caller]
    pub(crate) fn run(self, mut scene: impl FnMut(&mut Ui)) -> Report {
        let mut ui = if self.text {
            UiHarness::with_text(SURFACE)
        } else {
            UiHarness::new(SURFACE)
        };
        let mut recorded = false;
        self.measure(Location::caller(), || {
            recorded = false;
            let _ = ui.frame(|ui| {
                recorded = true;
                scene(ui);
            });
            recorded
        })
    }

    /// The same measured loop over a frame the caller renders itself.
    /// [`Self::text`] describes the harness [`Self::run`] raises, so it
    /// says nothing here beyond its window, and the caller asserts what its
    /// own frames ran.
    #[track_caller]
    pub(crate) fn run_frames(self, mut frame: impl FnMut()) -> Report {
        assert!(
            !self.paint_only,
            "paint_only reads the scene's record passes, which run_frames never sees",
        );
        self.measure(Location::caller(), || {
            frame();
            true
        })
    }

    /// `frame` returns whether it ran the scene, which only the measured
    /// window checks: a paint-only scene still records while it warms.
    fn measure(self, at: &'static Location<'static>, mut frame: impl FnMut() -> bool) -> Report {
        assert!(self.frames > 0, "an audit must measure at least one frame");

        let warmup = match self.warmup {
            Warmup::Fixed(n) => {
                for _ in 0..n {
                    frame();
                }
                n
            }
            Warmup::Probe => self.probe(&mut frame),
        };

        let mut counts = Vec::with_capacity(self.frames);
        for i in 0..self.frames {
            let mut recorded = false;
            let result = with_audit(|| recorded = frame());
            if recorded == self.paint_only {
                let what = if recorded {
                    "ran the scene, but the audit is paint_only"
                } else {
                    "skipped the scene, so the audit measured none of it — see Audit::paint_only"
                };
                panic!(
                    "alloc-audit {at}: frame {i}/{} (after {warmup} warmup) {what}",
                    self.frames,
                );
            }
            if result.allocs > self.budget {
                self.fail(at, i, warmup, result);
            }
            counts.push(result.allocs);
        }
        counts.sort_unstable();
        let worst = *counts.last().expect("at least one frame");
        let total: u64 = counts.iter().sum();
        let mode = counts
            .chunk_by(|a, b| a == b)
            .fold((0, 0), |(best, run), chunk| {
                if chunk.len() > run {
                    (chunk[0], chunk.len())
                } else {
                    (best, run)
                }
            })
            .0;

        println!(
            "alloc-audit {at}: worst {worst}, mode {mode}, mean {:.2}, budget {} — over {} \
             frames after {warmup} warmup",
            total as f64 / self.frames as f64,
            self.budget,
            self.frames,
        );
        Report { worst, mode }
    }

    fn probe(self, frame: &mut impl FnMut() -> bool) -> usize {
        const MAX_WARMUP: usize = 8;
        const STABLE_RUN: usize = 2;
        // Real shaping defers one allocation past any run of quiet frames:
        // the shaped-buffer cache's first expiry drain grows its wheel's
        // scratch. The tickets it drains are filed on the first frame, one
        // probation window plus a frame out, and the clock ticks at the
        // start of the frame after — so the warmup covers that frame too.
        let floor = if self.text {
            PROBATION_KEEP_FRAMES as usize + 2
        } else {
            0
        };

        let mut warmup = 0;
        let mut stable = 0;
        while warmup < MAX_WARMUP {
            let result = with_audit(|| {
                frame();
            });
            warmup += 1;
            stable = if result.allocs <= self.budget {
                stable + 1
            } else {
                0
            };
            if stable >= STABLE_RUN && warmup >= floor {
                break;
            }
        }
        warmup
    }

    fn fail(
        self,
        at: &'static Location<'static>,
        frame_idx: usize,
        warmup: usize,
        mut result: AuditResult,
    ) -> ! {
        eprintln!(
            "alloc-audit {at}: frame {frame_idx}/{} (after {warmup} warmup) allocated {} times, \
             {} B — budget is {}/frame",
            self.frames, result.allocs, result.bytes, self.budget,
        );
        let traced = result.traces.len() as u64;
        for (i, bt) in result.traces.iter_mut().enumerate() {
            eprintln!("--- alloc #{i} backtrace ---\n{}", format::user_frames(bt));
        }
        if result.allocs > traced {
            eprintln!(
                "({} further allocations went untraced — a window keeps the first {traced})",
                result.allocs - traced,
            );
        }
        eprintln!(
            "(set PALANTIR_ALLOC_FULL_BT=1 to disable user-code filtering and see full stacks)"
        );
        panic!(
            "alloc budget exceeded at {at} on frame {frame_idx} (budget {}/frame, got {})",
            self.budget, result.allocs,
        );
    }
}

#[cfg(test)]
mod tests;
