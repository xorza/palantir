//! Frame-loop drivers around `Ui` that measure heap allocations attributable to one
//! scene's per-frame work. [`Audit`] is the single way in: warmup length, measured
//! frame count and per-frame budget. [`Audit::run`] raises a `UiHarness`; anything
//! rendering frames its own way (renderer fixtures, the full-tree gate, device
//! gates through [`OffscreenTarget`]) drives [`Audit::run_frames`].
//!
//! Both terminals are `#[track_caller]`, so cargo prints the failing test's own
//! name, and both run inside [`with_audit`] whose per-thread counter (see
//! `allocator.rs`) parallel tests cannot pollute.

#![expect(
    clippy::print_stderr,
    clippy::print_stdout,
    reason = "the allocation suite reports each failing allocation and its backtrace to the terminal"
)]

mod format;
mod offscreen;

pub(crate) use format::user_frames;
pub(crate) use offscreen::OffscreenTarget;

use std::panic::Location;

use glam::UVec2;
use palantir::Ui;
use palantir::internals::harness::UiHarness;
use palantir::internals::{PROBATION_KEEP_FRAMES, SHAPED_BUFFER_RING_FRAMES};

use crate::allocator::{AuditResult, with_audit};

/// Logical display every fixture renders at: `UiHarness`'s defaults (scale 1.0,
/// pixel-snapped, no refresh rate) at 800×600, shared by [`Audit::run`] and
/// [`new_ui`].
const SURFACE: UVec2 = UVec2::new(800, 600);

/// Mono-fallback harness for the alloc audits: private arena, fresh caches, no font
/// loading.
pub(crate) fn new_ui() -> UiHarness {
    UiHarness::new(SURFACE)
}

#[derive(Clone, Copy, Debug)]
enum Warmup {
    /// Stop once `STABLE_RUN` consecutive frames land inside the budget, giving up
    /// at `MAX_WARMUP`. Right for any scene that settles.
    ///
    /// **Wrong for a scene that cycles.** The probe can settle on two quiet frames
    /// *within* one cycle, before the widest frame has been recorded, so the
    /// measured window meets that frame's one-off growth and reads it as per-frame
    /// cost. Such a scene wants [`Audit::warmup`] with whole cycles.
    Probe,
    /// Exactly this many frames, warmed without any budget check.
    Fixed(usize),
}

/// One allocation audit: how to raise the scene, how long to warm it, how many
/// frames to measure and what each may spend. The defaults (the probe, 64 frames,
/// strict-zero budget) make `Audit::new().run(scene)` the whole call for most
/// fixtures.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Audit {
    text: bool,
    warmup: Warmup,
    frames: usize,
    budget: u64,
    paint_only: bool,
}

/// What an audit observed. The worst frame is the number that matters to a budget;
/// printing it keeps fixtures from carrying a hand-recorded measurement nothing
/// rechecks.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Report {
    pub(crate) worst: u64,
    /// The count most measured frames read, the smaller on a tie: a driver's floor
    /// is flat but for a rare frame its pool missed.
    pub(crate) mode: u64,
}

impl Audit {
    pub(crate) const fn new() -> Self {
        Audit {
            text: false,
            warmup: Warmup::Probe,
            frames: 64,
            budget: 0,
            paint_only: false,
        }
    }

    /// Real cosmic shaping instead of the mono fallback, warmed and measured in one
    /// revolution of the shaped-buffer expiry ring each: a bucket's first drain
    /// grows the wheel's scratch, after the probe's quiet frames.
    pub(crate) const fn text(mut self) -> Self {
        let ring = SHAPED_BUFFER_RING_FRAMES as usize;
        self.text = true;
        self.warmup = Warmup::Fixed(ring);
        self.frames = ring;
        self
    }

    /// A fixed warmup in place of the probe; see [`Warmup::Probe`] for the scene
    /// shape that needs one.
    pub(crate) const fn warmup(mut self, frames: usize) -> Self {
        self.warmup = Warmup::Fixed(frames);
        self
    }

    pub(crate) const fn frames(mut self, frames: usize) -> Self {
        self.frames = frames;
        self
    }

    /// What one measured frame may allocate. Zero unless said otherwise; a non-zero
    /// budget pins flatness, a ceiling a cost growing with frame count would blow
    /// through.
    pub(crate) const fn budget(mut self, allocs: u64) -> Self {
        self.budget = allocs;
        self
    }

    /// Measure the frames that repaint from the retained tree without running the
    /// scene (an idle animation's steady state).
    ///
    /// Otherwise every measured frame must run the scene, since a frame skipping it
    /// measures nothing the fixture wrote; a scene with a paint-only animation that
    /// still wants its record measured calls `Ui::request_repaint` each frame.
    pub(crate) const fn paint_only(mut self) -> Self {
        self.paint_only = true;
        self
    }

    /// Drive `scene` through a `UiHarness` raised from [`Self::text`], every
    /// measured frame checked against [`Self::paint_only`].
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

    /// The same measured loop over a frame the caller renders itself; the caller
    /// asserts what its own frames ran.
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

    /// `frame` returns whether it ran the scene, which only the measured window
    /// checks.
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
        // Real shaping defers one allocation past any run of quiet frames: the
        // shaped-buffer cache's first expiry drain grows its wheel's scratch. Its
        // tickets are filed on the first frame, one probation window plus a frame
        // out, and the clock ticks at the next frame's start, so warmup covers that
        // frame too.
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
            eprintln!("--- alloc #{i} backtrace ---\n{}", user_frames(bt));
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
