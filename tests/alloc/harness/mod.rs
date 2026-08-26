//! Frame-loop drivers around `Ui` that measure heap allocations
//! attributable to one scene's per-frame work.
//!
//! [`Audit`] is the single way in: it carries how long to warm the scene
//! up, how many frames to measure, and what each of those frames may
//! spend. [`Audit::run`] raises a `UiHarness` and drives the scene
//! through it; a gate that renders frames its own way — through an
//! `OffscreenHost`, say — drives [`Audit::run_frames`] instead.
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

pub(crate) use format::user_frames;

use std::panic::Location;

use glam::UVec2;
use palantir::Ui;
use palantir::internals::UiHarness;

use crate::allocator::{AuditResult, with_audit};

/// Logical display an audit runs at unless [`Audit::surface`] says
/// otherwise — `UiHarness`'s own defaults (scale 1.0, pixel-snapped, no
/// refresh rate) at 800×600.
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
    surface: UVec2,
    dpr: Option<f32>,
    text: bool,
    warmup: Warmup,
    frames: usize,
    budget: u64,
}

/// What an audit observed.
///
/// The worst frame is the number that matters: it says how much slack a
/// budget has, and the harness printing it is what keeps a fixture from
/// carrying a hand-recorded measurement nothing rechecks.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Report {
    pub(crate) worst: u64,
}

impl Audit {
    pub(crate) fn new() -> Self {
        Audit {
            surface: SURFACE,
            dpr: None,
            text: false,
            warmup: Warmup::Probe,
            frames: 64,
            budget: 0,
        }
    }

    /// Real cosmic shaping instead of the mono fallback, for a fixture
    /// that has to exercise it.
    pub(crate) fn text(mut self) -> Self {
        self.text = true;
        self
    }

    pub(crate) fn surface(mut self, size: UVec2) -> Self {
        self.surface = size;
        self
    }

    pub(crate) fn dpr(mut self, dpr: f32) -> Self {
        self.dpr = Some(dpr);
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

    /// Drive `scene` through a `UiHarness` raised from [`Self::text`],
    /// [`Self::surface`] and [`Self::dpr`].
    #[track_caller]
    pub(crate) fn run(self, mut scene: impl FnMut(&mut Ui)) -> Report {
        let mut ui = if self.text {
            UiHarness::with_text(self.surface)
        } else {
            UiHarness::new(self.surface)
        };
        // Only when asked: `scale` re-syncs the display and marks the
        // harness warm, which the default path has no reason to do.
        if let Some(dpr) = self.dpr {
            ui = ui.scale(dpr);
        }
        self.measure(Location::caller(), || {
            let _ = ui.frame(&mut scene);
        })
    }

    /// The same measured loop over a frame the caller renders itself.
    /// [`Self::text`], [`Self::surface`] and [`Self::dpr`] describe the
    /// harness [`Self::run`] raises, so they say nothing here.
    #[track_caller]
    pub(crate) fn run_frames(self, frame: impl FnMut()) -> Report {
        self.measure(Location::caller(), frame)
    }

    fn measure(self, at: &'static Location<'static>, mut frame: impl FnMut()) -> Report {
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

        let mut worst = 0;
        let mut total = 0;
        for i in 0..self.frames {
            let result = with_audit(&mut frame);
            if result.allocs > self.budget {
                self.fail(at, i, warmup, result);
            }
            worst = worst.max(result.allocs);
            total += result.allocs;
        }

        println!(
            "alloc-audit {at}: worst {worst}, mean {:.2}, budget {} — over {} frames \
             after {warmup} warmup",
            total as f64 / self.frames as f64,
            self.budget,
            self.frames,
        );
        Report { worst }
    }

    fn probe(self, frame: &mut impl FnMut()) -> usize {
        const MAX_WARMUP: usize = 8;
        const STABLE_RUN: usize = 2;

        let mut warmup = 0;
        let mut stable = 0;
        while warmup < MAX_WARMUP {
            let result = with_audit(&mut *frame);
            warmup += 1;
            stable = if result.allocs <= self.budget {
                stable + 1
            } else {
                0
            };
            if stable >= STABLE_RUN {
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
        for (i, bt) in result.traces.iter_mut().enumerate() {
            eprintln!("--- alloc #{i} backtrace ---\n{}", format::user_frames(bt));
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
