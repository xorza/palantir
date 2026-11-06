//! One stage on the GPU: a host that paints a decision on a still screen, and the turns arms take.

use crate::bench::scene::{Scene, SceneFrame};
use crate::damage::Damage;
use crate::damage::sweep::arms::Arm;
use crate::damage::sweep::policy::Decision;
use crate::damage::sweep::score::{ArmCost, ArmSamples};
use crate::damage::sweep::stage::Stage;
use crate::gpu::bench_gpu::BenchGpu;
use crate::gpu::surface::owned_target::OwnedTarget;
use crate::host::offscreen::{OffscreenHost, OffscreenTime};
use crate::primitives::geometry::rect::Rect;
use crate::ui::Ui;
use crate::ui::bench::{bench_host, settle_look};
use crate::ui::frame_report::FramePaint;
use std::time::{Duration, Instant};

/// Frames a new stage runs first: its theme arrives on the second, and the first whole repaint
/// fills its atlases.
const LOOK_FRAMES: u32 = 4;

/// How long a new stage repaints before it is timed, as criterion's warm-up of a frame case: the
/// first scenario of a run read 639 µs against the next one's 324, and 253 once settled.
const WARM: Duration = Duration::from_secs(1);

/// One frame of an arm, ready to paint.
#[derive(Clone, Debug)]
pub(crate) struct PlayedFrame {
    damage: Damage,
    /// What the frame's content changed, which leaves the panes it touches stale.
    changed: Vec<Rect>,
}

impl PlayedFrame {
    /// A frame that repaints as `decision` says after a change of `changed`.
    pub(crate) fn new(decision: &Decision, changed: &[Rect]) -> Self {
        Self {
            damage: decision.damage(),
            changed: changed.to_vec(),
        }
    }

    /// The frames of `arm` over a cycle of `raw` rects.
    pub(super) fn of(arm: &Arm, raw: &[Vec<Rect>]) -> Vec<Self> {
        arm.frames
            .iter()
            .zip(raw)
            .map(|(decision, changed)| Self::new(decision, changed))
            .collect()
    }
}

/// One frame's time in its two halves, which the device and the host overlap in a real window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FrameTime {
    /// Until the host returns, its submit last: the CPU half.
    pub(crate) cpu: Duration,
    /// From there until the device is done: the GPU half, and a wait's constant cost.
    pub(crate) gpu: Duration,
}

/// How many frames an arm plays in a turn, and how many turns it takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Turns {
    pub(super) rounds: u32,
    pub(super) frames: u32,
}

/// The arm that takes turn `turn` of `round`: every second round runs backward, so a slow drift, as
/// a chip warming up, charges each arm alike, where one run per arm would charge it to the last.
pub(super) const fn arm_of_turn(round: u32, turn: usize, arm_count: usize) -> usize {
    if round.is_multiple_of(2) {
        turn
    } else {
        arm_count - 1 - turn
    }
}

/// The frames a turn plays of a cycle of `cycle_len`: at least `frames`, in whole cycles, so each
/// position counts alike.
pub(super) const fn played_frames(frames: u32, cycle_len: usize) -> usize {
    (frames as usize).div_ceil(cycle_len) * cycle_len
}

/// A stage's host, holding one screen still while arms take turns painting over it.
#[derive(Debug)]
pub(crate) struct Player<'a, S> {
    gpu: &'a BenchGpu,
    stage: Stage,
    host: OffscreenHost,
    target: OwnedTarget,
    scene: SceneFrame<S>,
}

impl<'a, S: Scene> Player<'a, S> {
    /// A warm host of `stage` on `gpu`.
    pub(crate) fn new(gpu: &'a BenchGpu, stage: Stage) -> Self {
        let target = gpu.target("palantir.damage_sweep", stage.physical);
        let mut player = Self {
            gpu,
            stage,
            host: bench_host(gpu, OffscreenTime::Realtime),
            target,
            scene: SceneFrame::default(),
        };
        for n in 0..LOOK_FRAMES {
            settle_look(player.host.ui_mut(), n);
            player.paint();
            player.gpu.wait();
        }
        player.warm();
        player
    }

    /// Repaint for [`WARM`], whole and in part: each path's pipelines and targets exist then, and
    /// the device runs at the speed it holds. A frame after a whole one repaints whole again, as
    /// the backbuffer is stale, so the partial frame comes twice.
    fn warm(&mut self) {
        let whole = PlayedFrame::new(&Decision::Full, &[]);
        let corner = Rect::new(0.0, 0.0, 16.0, 16.0);
        let part = PlayedFrame::new(&Decision::partial(vec![corner]), &[corner]);
        let started = Instant::now();
        while started.elapsed() < WARM {
            self.settle(&[whole.clone(), part.clone(), part.clone()]);
        }
    }

    fn paint(&mut self) -> FramePaint {
        let Self {
            stage,
            host,
            target,
            scene,
            ..
        } = self;
        let mut app = |ui: &mut Ui| stage.record(scene, ui);
        host.set_system_scale(stage.scale)
            .frame(target.as_target(), &mut app)
            .paint()
    }

    /// Paint `frame` and wait for the device, timing each half.
    pub(crate) fn play(&mut self, frame: &PlayedFrame) -> FrameTime {
        let start = Instant::now();
        self.host.force_next_frame(frame.damage, &frame.changed);
        let paint = self.paint();
        let submitted = Instant::now();
        self.gpu.wait();
        let done = Instant::now();
        let want = match frame.damage {
            Damage::Full => FramePaint::Full,
            Damage::Partial(_) => FramePaint::Partial,
        };
        assert_eq!(
            paint, want,
            "the frame planned another paint than its arm's"
        );
        FrameTime {
            cpu: submitted - start,
            gpu: done - submitted,
        }
    }

    /// One cycle untimed, so what the last arm left in the backbuffer and the canvases is gone.
    pub(crate) fn settle(&mut self, frames: &[PlayedFrame]) {
        for frame in frames {
            self.play(frame);
        }
    }

    /// Each of `arms`' costs, the arms taking turns.
    pub(super) fn sample(&mut self, arms: &[Vec<PlayedFrame>], turns: Turns) -> Vec<ArmCost> {
        let mut samples: Vec<ArmSamples> = arms
            .iter()
            .map(|frames| ArmSamples::new(frames.len(), turns))
            .collect();
        for round in 0..turns.rounds {
            for turn in 0..arms.len() {
                let idx = arm_of_turn(round, turn, arms.len());
                let frames = &arms[idx];
                self.settle(frames);
                samples[idx].start_round();
                for at in 0..played_frames(turns.frames, frames.len()) {
                    let position = at % frames.len();
                    let time = self.play(&frames[position]);
                    samples[idx].push(position, time);
                }
            }
        }
        samples.iter_mut().map(ArmSamples::cost).collect()
    }
}
