//! What the measurements say: each arm's cost, and how far each policy lands from the best arm.

use crate::damage::sweep::arms::Arm;
use crate::damage::sweep::player::{FrameTime, Turns, played_frames};
use crate::damage::sweep::stage::Stage;
use std::time::Duration;

/// A time as signed nanoseconds: exact, and two can differ either way.
fn nanos(time: Duration) -> i64 {
    i64::try_from(time.as_nanos()).expect("a frame takes under 292 years")
}

/// The time of `nanos`, which a median or a regret above the noise keeps above zero.
fn time_of(nanos: i64) -> Duration {
    Duration::from_nanos(u64::try_from(nanos).expect("a time below zero"))
}

/// A median, of a slice in any order, the lower nanosecond between two.
fn median(values: &mut [i64]) -> i64 {
    assert!(!values.is_empty(), "the median of nothing");
    values.sort_unstable();
    let mid = values.len() / 2;
    if values.len().is_multiple_of(2) {
        i64::midpoint(values[mid - 1], values[mid])
    } else {
        values[mid]
    }
}

/// Half the interquartile range, each quartile of nearest rank: half the spread of the middle half.
fn half_iqr(values: &mut [i64]) -> i64 {
    assert!(!values.is_empty(), "the spread of nothing");
    values.sort_unstable();
    let rank = |share: usize| (values.len() * share).div_ceil(4).max(1) - 1;
    (values[rank(3)] - values[rank(1)]) / 2
}

/// How a frame's two halves count: both are real work, and which one a frame waits for depends on
/// what the host is short of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Metric {
    /// CPU and GPU added, the work a frame costs, and the time when they do not overlap.
    Sum,
    /// The longer half, what a frame rate follows when they overlap, as in a real window.
    Longer,
}

impl Metric {
    pub(super) const ALL: [Self; 2] = [Self::Sum, Self::Longer];

    const fn idx(self) -> usize {
        self as usize
    }

    /// A frame of `time` under this metric.
    fn of(self, time: FrameTime) -> Duration {
        match self {
            Self::Sum => time.cpu + time.gpu,
            Self::Longer => time.cpu.max(time.gpu),
        }
    }
}

/// What a sampled arm holds until its cost is read.
#[derive(Debug)]
pub(super) struct ArmSamples {
    /// The frames a round holds.
    round_frames: u32,
    /// Per metric, each frame position's samples.
    positions: [Vec<Vec<i64>>; Metric::ALL.len()],
    /// Per metric, each round's frames added.
    rounds: [Vec<Duration>; Metric::ALL.len()],
    cpu: Vec<i64>,
    gpu: Vec<i64>,
}

impl ArmSamples {
    /// Room for `turns` over a cycle of `cycle_len`.
    pub(super) fn new(cycle_len: usize, turns: Turns) -> Self {
        let round_frames = played_frames(turns.frames, cycle_len);
        let round_count = turns.rounds as usize;
        let per_position = round_frames / cycle_len * round_count;
        let positions = || vec![Vec::with_capacity(per_position); cycle_len];
        Self {
            round_frames: u32::try_from(round_frames).expect("a turn plays a few frames"),
            positions: [positions(), positions()],
            rounds: [
                Vec::with_capacity(round_count),
                Vec::with_capacity(round_count),
            ],
            cpu: Vec::with_capacity(round_frames * round_count),
            gpu: Vec::with_capacity(round_frames * round_count),
        }
    }

    pub(super) fn start_round(&mut self) {
        for round in &mut self.rounds {
            round.push(Duration::ZERO);
        }
    }

    pub(super) fn push(&mut self, position: usize, time: FrameTime) {
        for metric in Metric::ALL {
            let frame = metric.of(time);
            self.positions[metric.idx()][position].push(nanos(frame));
            *self.rounds[metric.idx()]
                .last_mut()
                .expect("a round started") += frame;
        }
        self.cpu.push(nanos(time.cpu));
        self.gpu.push(nanos(time.gpu));
    }

    /// The arm's cost from what it took.
    pub(super) fn cost(&mut self) -> ArmCost {
        let round_frames = self.round_frames;
        let mut read = |metric: Metric| {
            let idx = metric.idx();
            let rounds = self.rounds[idx]
                .iter()
                .map(|total| *total / round_frames)
                .collect();
            MetricCost::of(&mut self.positions[idx], rounds)
        };
        ArmCost {
            metrics: [read(Metric::Sum), read(Metric::Longer)],
            cpu: time_of(median(&mut self.cpu)),
            gpu: time_of(median(&mut self.gpu)),
        }
    }
}

/// One arm's time under one metric.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct MetricCost {
    /// The median of each frame position of the cycle.
    pub(super) positions: Vec<Duration>,
    /// The mean of [`Self::positions`]: one frame of the cycle, on average.
    pub(super) mean: Duration,
    /// Each round's mean frame, in round order, which [`ScenarioCosts`] pairs across arms.
    pub(super) rounds: Vec<Duration>,
}

impl MetricCost {
    /// The cost of `positions`, each frame position's samples in nanoseconds, over `rounds`.
    pub(super) fn of(positions: &mut [Vec<i64>], rounds: Vec<Duration>) -> Self {
        let positions: Vec<Duration> = positions
            .iter_mut()
            .map(|samples| time_of(median(samples)))
            .collect();
        let count = u32::try_from(positions.len()).expect("a cycle has a few frames");
        Self {
            mean: positions.iter().sum::<Duration>() / count,
            positions,
            rounds,
        }
    }
}

/// One arm's time on one stage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ArmCost {
    pub(super) metrics: [MetricCost; Metric::ALL.len()],
    /// The median frame's CPU half.
    pub(super) cpu: Duration,
    /// The median frame's GPU half.
    pub(super) gpu: Duration,
}

impl ArmCost {
    pub(super) const fn sum(&self) -> &MetricCost {
        &self.metrics[Metric::Sum.idx()]
    }
}

/// What an arm costs past the best, round for round.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Regret {
    /// The median gap, zero when within [`Self::noise`]: the lowest of several noisy means reads
    /// low by about that much, so a smaller gap says nothing about which arm is cheaper.
    pub(super) regret: Duration,
    /// Half the interquartile range of the rounds' gaps.
    pub(super) noise: Duration,
}

/// A scenario's arms under one metric: the cheapest, and each against it.
#[derive(Debug)]
pub(super) struct Standing {
    pub(super) best: usize,
    pub(super) regrets: Vec<Regret>,
}

impl Standing {
    /// Each arm's gap to the best is read round by round, as arms run side by side in a round: a
    /// device that changes speed between rounds, as a mobile GPU's clock does, moves both alike,
    /// where comparing each arm's own median would read the change as a gap.
    fn of(costs: &[ArmCost], metric: Metric) -> Self {
        let of_metric: Vec<&MetricCost> =
            costs.iter().map(|arm| &arm.metrics[metric.idx()]).collect();
        let best = of_metric
            .iter()
            .enumerate()
            .min_by_key(|(_, arm)| arm.mean)
            .map(|(idx, _)| idx)
            .expect("a scenario has an arm");
        let regrets = of_metric
            .iter()
            .map(|arm| {
                let mut gaps: Vec<i64> = arm
                    .rounds
                    .iter()
                    .zip(&of_metric[best].rounds)
                    .map(|(arm, best)| nanos(*arm) - nanos(*best))
                    .collect();
                let gap = median(&mut gaps);
                let noise = half_iqr(&mut gaps);
                Regret {
                    regret: if gap > noise {
                        time_of(gap)
                    } else {
                        Duration::ZERO
                    },
                    noise: time_of(noise),
                }
            })
            .collect();
        Self { best, regrets }
    }
}

/// One scenario's arms, timed on one stage.
#[derive(Debug)]
pub(super) struct ScenarioCosts {
    pub(super) name: String,
    pub(super) weight: u32,
    pub(super) costs: Vec<ArmCost>,
    /// The arm of each policy of the grid.
    pub(super) of_policy: Vec<usize>,
    /// Each metric's standing.
    pub(super) standings: [Standing; Metric::ALL.len()],
}

impl ScenarioCosts {
    pub(super) fn new(
        name: String,
        weight: u32,
        costs: Vec<ArmCost>,
        of_policy: Vec<usize>,
    ) -> Self {
        let standings = [
            Standing::of(&costs, Metric::Sum),
            Standing::of(&costs, Metric::Longer),
        ];
        Self {
            name,
            weight,
            costs,
            of_policy,
            standings,
        }
    }

    /// The standing frames are judged by first.
    pub(super) const fn sum(&self) -> &Standing {
        &self.standings[Metric::Sum.idx()]
    }
}

/// What a stage measured, and the score it gives each policy of a grid.
#[derive(Debug)]
pub(super) struct StageResults {
    pub(super) stage: Stage,
    /// Where production stands in the grid.
    pub(super) production: usize,
    pub(super) costs: Vec<ScenarioCosts>,
    /// Each scenario's arms, whose costs [`Self::costs`] holds in the same order.
    pub(super) arms: Vec<Vec<Arm>>,
    pub(super) scores: Vec<PolicyScore>,
    /// The stage's density, which a density promote read.
    pub(super) rows_per_mpx: f32,
}

/// One policy over all of a stage's scenarios.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PolicyScore {
    /// The largest regret of any scenario.
    pub(super) worst_regret: Duration,
    /// [`Self::worst_regret`] as a percent of that scenario's best frame.
    pub(super) worst_regret_pct: f64,
    /// The scenario of [`Self::worst_regret`], none when no regret is above the noise.
    pub(super) worst_scenario: Option<usize>,
    /// The regret per frame, each scenario counted [`ScenarioCosts::weight`] times.
    pub(super) mean_regret: Duration,
    /// Each scenario's regret as a percent of its best frame, weighed alike: what compares across
    /// hosts, whose frames differ by a factor of 20.
    pub(super) mean_regret_pct: f64,
    /// [`Self::mean_regret_pct`] under [`Metric::Longer`].
    pub(super) mean_longer_pct: f64,
}

/// `part` as a percent of `whole`.
fn pct(part: Duration, whole: Duration) -> f64 {
    part.as_secs_f64() / whole.as_secs_f64() * 100.0
}

/// The score of each of `policy_count` policies over `scenarios`.
pub(super) fn score(scenarios: &[ScenarioCosts], policy_count: usize) -> Vec<PolicyScore> {
    let total_weight: u32 = scenarios.iter().map(|s| s.weight).sum();
    (0..policy_count)
        .map(|policy| {
            let mut out = PolicyScore {
                worst_regret: Duration::ZERO,
                worst_regret_pct: 0.0,
                worst_scenario: None,
                mean_regret: Duration::ZERO,
                mean_regret_pct: 0.0,
                mean_longer_pct: 0.0,
            };
            for (idx, scenario) in scenarios.iter().enumerate() {
                let arm = scenario.of_policy[policy];
                let weight = f64::from(scenario.weight);
                let standing = scenario.sum();
                let regret = standing.regrets[arm].regret;
                let regret_pct = pct(regret, scenario.costs[standing.best].sum().mean);
                out.mean_regret += regret * scenario.weight;
                out.mean_regret_pct += regret_pct * weight;
                if regret > out.worst_regret {
                    out.worst_regret = regret;
                    out.worst_regret_pct = regret_pct;
                    out.worst_scenario = Some(idx);
                }
                let longer = &scenario.standings[Metric::Longer.idx()];
                out.mean_longer_pct += pct(
                    longer.regrets[arm].regret,
                    scenario.costs[longer.best].metrics[Metric::Longer.idx()].mean,
                ) * weight;
            }
            out.mean_regret /= total_weight;
            out.mean_regret_pct /= f64::from(total_weight);
            out.mean_longer_pct /= f64::from(total_weight);
            out
        })
        .collect()
}
