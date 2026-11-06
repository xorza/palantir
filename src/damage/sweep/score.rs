//! What the measurements say: each arm's cost, and how far each policy lands from the best arm.

use crate::damage::sweep::arms::Arm;
use crate::damage::sweep::stage::Stage;

/// A median, of a slice in any order.
fn median(values: &mut [f64]) -> f64 {
    assert!(!values.is_empty(), "the median of nothing");
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    if values.len().is_multiple_of(2) {
        f64::midpoint(values[mid - 1], values[mid])
    } else {
        values[mid]
    }
}

/// Half the interquartile range, each quartile of nearest rank: half the spread of the middle half.
fn half_iqr(values: &mut [f64]) -> f64 {
    assert!(!values.is_empty(), "the spread of nothing");
    values.sort_by(f64::total_cmp);
    let rank = |share: usize| (values.len() * share).div_ceil(4).max(1) - 1;
    (values[rank(3)] - values[rank(1)]) / 2.0
}

/// One arm's time on one stage.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ArmCost {
    /// The median of each frame position of the cycle.
    pub(super) position_us: Vec<f64>,
    /// The mean of [`Self::position_us`]: one frame of the cycle, on average.
    pub(super) mean_us: f64,
    /// How much a round's mean moves: half the interquartile range of the rounds.
    pub(super) spread_us: f64,
}

impl ArmCost {
    /// The cost of `positions`, the samples of each frame position, over rounds of the means in
    /// `rounds`.
    pub(super) fn of(positions: &mut [Vec<f64>], rounds: &mut [f64]) -> Self {
        let position_us: Vec<f64> = positions
            .iter_mut()
            .map(|samples| median(samples))
            .collect();
        Self {
            mean_us: position_us.iter().sum::<f64>() / position_us.len() as f64,
            position_us,
            spread_us: half_iqr(rounds),
        }
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
}

impl ScenarioCosts {
    /// The cheapest arm.
    pub(super) fn best(&self) -> usize {
        self.costs
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| a.mean_us.total_cmp(&b.mean_us))
            .map(|(idx, _)| idx)
            .expect("a scenario has an arm")
    }

    /// What taking `arm` costs past the best arm, zero when the two lie within their spreads: the
    /// lowest of several noisy means reads low by about that much, so a smaller gap says nothing
    /// about which arm is cheaper.
    pub(super) fn regret_us(&self, arm: usize) -> f64 {
        let (picked, best) = (&self.costs[arm], &self.costs[self.best()]);
        let gap = picked.mean_us - best.mean_us;
        if gap <= picked.spread_us + best.spread_us {
            0.0
        } else {
            gap
        }
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
}

/// One policy over all of a stage's scenarios.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PolicyScore {
    /// The largest regret of any scenario.
    pub(super) worst_regret_us: f64,
    /// The scenario of [`Self::worst_regret_us`], none when no regret is above the noise.
    pub(super) worst_scenario: Option<usize>,
    /// The regret per frame, each scenario counted [`ScenarioCosts::weight`] times.
    pub(super) mean_regret_us: f64,
}

/// The score of each of `policy_count` policies over `scenarios`.
pub(super) fn score(scenarios: &[ScenarioCosts], policy_count: usize) -> Vec<PolicyScore> {
    let total_weight: u64 = scenarios.iter().map(|s| u64::from(s.weight)).sum();
    (0..policy_count)
        .map(|policy| {
            let mut out = PolicyScore {
                worst_regret_us: 0.0,
                worst_scenario: None,
                mean_regret_us: 0.0,
            };
            for (idx, scenario) in scenarios.iter().enumerate() {
                let regret = scenario.regret_us(scenario.of_policy[policy]);
                out.mean_regret_us += regret * f64::from(scenario.weight);
                if regret > out.worst_regret_us {
                    out.worst_regret_us = regret;
                    out.worst_scenario = Some(idx);
                }
            }
            out.mean_regret_us /= total_weight as f64;
            out
        })
        .collect()
}
