//! A sweep's decisions, arms and scores on hand-worked inputs.

use crate::bench::scene::{BENCH_DPR, BENCH_SURFACE, Scene};
use crate::damage::bench::POLICY_CASES;
use crate::damage::region::{DAMAGE_RECT_CAP, MergePolicy};
use crate::damage::sweep::args::SweepArgs;
use crate::damage::sweep::arms::{Arms, partitions};
use crate::damage::sweep::capture::{Captured, SeenFrames, kept, note, session};
use crate::damage::sweep::error::SweepError;
use crate::damage::sweep::player::{FrameTime, Turns, arm_of_turn, played_frames};
use crate::damage::sweep::policy::{Decision, FrameFacts, Policy, Promote, union_area};
use crate::damage::sweep::report::{beating, order_outcomes};
use crate::damage::sweep::scenario::{REFERENCE, Scope, catalog, named, pair_grid};
use crate::damage::sweep::score::{
    ArmCost, ArmSamples, MetricCost, PolicyScore, ScenarioCosts, score,
};
use crate::damage::sweep::selected;
use crate::damage::sweep::stage::Stage;
use crate::host::harness::Harness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::paint::Paint;
use crate::shape::Shape;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::button::Button;
use crate::widgets::panel::Panel;
use crate::widgets::text_edit::TextEdit;
use std::collections::HashSet;
use std::time::Duration;

const SURFACE: Rect = Rect::new(0.0, 0.0, 1280.0, 720.0);

/// A screen of 400 rows per Mpx, where production's promote share reads 0.5.
const FACTS: FrameFacts = FrameFacts {
    surface: SURFACE,
    rows_per_mpx: 400.0,
    pane_share: 0.0,
};

/// What production decides for the one frame of `name`.
fn production(name: &str) -> Decision {
    Policy::PRODUCTION.decide(&named(name).frames[0], &FACTS)
}

fn partial(rects: &[Rect]) -> Decision {
    Decision::partial(rects.to_vec())
}

/// Each name unique, each frame's rects non-empty and on the window, so a clip changes no case.
#[test]
fn the_catalog_lies_on_its_window() {
    let window = Rect::new(0.0, 0.0, REFERENCE.x, REFERENCE.y);
    let scenarios = catalog();
    let names: HashSet<&str> = scenarios.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names.len(), scenarios.len());
    for scenario in &scenarios {
        assert!(
            !scenario.name.contains(','),
            "a CSV cell: {}",
            scenario.name
        );
        assert_eq!(scenario.weight, 1, "{}", scenario.name);
        assert!(!scenario.frames.is_empty(), "{}", scenario.name);
        for rect in scenario.frames.iter().flatten() {
            assert!(!rect.is_paint_empty(), "{}: {rect:?}", scenario.name);
            assert!(window.contains_rect(*rect), "{}: {rect:?}", scenario.name);
        }
    }
    for case in POLICY_CASES {
        assert!(names.contains(case), "{case}");
    }
}

/// 3 ratios × 4 aspects × 6 relations, plus `contained` where B fits inside the 200 × 200 A: the
/// square at every ratio (3), wide and tall below ratio 1 (4), and thin at 1:100 alone, whose side
/// `sqrt(400 × 32)` = 113 fits where `sqrt(4000 × 32)` = 357 does not.
#[test]
fn the_pair_grid_is_the_full_product() {
    let grid = pair_grid();
    assert_eq!(grid.len(), 3 * 4 * 6 + 3 + 4 + 1);
    let names: HashSet<&str> = grid.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names.len(), grid.len());
}

/// The rects hand-worked in logical px on a 1280 × 720 window of 921,600 px², under a 5,000 px²
/// budget and a promote share of 0.5.
#[test]
fn production_decides_each_archetype_as_worked_by_hand() {
    let sidebar = Rect::new(0.0, 0.0, 240.0, 720.0);
    let toolbar = Rect::new(240.0, 0.0, 1040.0, 48.0);
    let clock = Rect::new(1200.0, 696.0, 60.0, 18.0);
    let cases = [
        // Bounds of 921,600 past 172,800 + 49,920 waste 698,880: split, at 24.2 %.
        ("pair/l_touching", partial(&[sidebar, toolbar])),
        // A pixel column of overlap changes nothing: 921,600 past 172,800 + 49,968 − 48 waste
        // 698,880 again.
        (
            "pair/l_overlap",
            partial(&[sidebar, Rect::new(239.0, 0.0, 1041.0, 48.0)]),
        ),
        // A crosshair's bounds are the window, so its strips stay apart.
        (
            "pair/cross_strips",
            partial(&[
                Rect::new(630.0, 0.0, 20.0, 720.0),
                Rect::new(0.0, 350.0, 1280.0, 20.0),
            ]),
        ),
        // 400 × 65 = 26,000 past 2 × 12,800 wastes 400.
        (
            "pair/rows_gap1",
            partial(&[Rect::new(40.0, 100.0, 400.0, 65.0)]),
        ),
        // 152 × 44 = 6,688 past 2 × 1,936 wastes 2,816.
        (
            "pair/icons_gap64",
            partial(&[Rect::new(20.0, 8.0, 152.0, 44.0)]),
        ),
        // 488 × 44 = 21,472 past 2 × 1,936 wastes 17,600.
        (
            "pair/icons_gap400",
            partial(&[
                Rect::new(20.0, 8.0, 44.0, 44.0),
                Rect::new(464.0, 8.0, 44.0, 44.0),
            ]),
        ),
        // 2 × 400 × 484 = 387,200, 42.0 %, under the share.
        (
            "pair/panels_sum42",
            partial(&[
                Rect::new(0.0, 0.0, 400.0, 484.0),
                Rect::new(880.0, 236.0, 400.0, 484.0),
            ]),
        ),
        // 96 × 44 = 4,224 past 2 × 1,936 wastes 352; the clock stays far.
        (
            "triple/cluster_outlier",
            partial(&[Rect::new(20.0, 8.0, 96.0, 44.0), clock]),
        ),
        ("one/block_55", Decision::Full),
    ];
    for (name, want) in cases {
        assert_eq!(production(name), want, "{name}");
    }
    assert!(
        matches!(production("one/block_45"), Decision::Partial(rects) if rects.len() == 1),
        "45 % stays partial"
    );
}

/// Each knob changes a decision: near rows split at a budget of 0, far icons join at a cap of 1,
/// and 35 % goes whole under a share of 0.3 and under a thin screen's density, where 40 rows per
/// Mpx read a share of 40 / 440 = 0.09. The full-repaint threshold caps any share: 75 % goes whole
/// at a density whose share reads 0.9.
#[test]
fn each_knob_moves_a_decision() {
    let decide = |name: &str, policy: Policy, facts: &FrameFacts| {
        policy.decide(&named(name).frames[0], facts)
    };
    let merge = |budget_px, rect_cap| Policy {
        merge: MergePolicy {
            budget_px,
            rect_cap,
        },
        ..Policy::PRODUCTION
    };
    assert_eq!(
        decide("pair/rows_gap1", merge(0.0, DAMAGE_RECT_CAP), &FACTS),
        partial(&[
            Rect::new(40.0, 100.0, 400.0, 32.0),
            Rect::new(40.0, 133.0, 400.0, 32.0),
        ])
    );
    assert_eq!(
        decide("pair/icons_gap1000", merge(5_000.0, 1), &FACTS),
        partial(&[Rect::new(20.0, 8.0, 1088.0, 44.0)])
    );
    let one_share = Policy {
        promote: Promote::Coverage(0.3),
        ..Policy::PRODUCTION
    };
    assert_eq!(decide("one/block_35", one_share, &FACTS), Decision::Full);
    let thin = FrameFacts {
        rows_per_mpx: 40.0,
        ..FACTS
    };
    // Panes on half the surface count for 150 rows per Mpx under production: a thin screen's share
    // of 0.09 rises to 190 / 590 = 0.32, which holds 25 % as partial, and does not under a rule that
    // counts no pane.
    let under_panes = FrameFacts {
        pane_share: 0.5,
        ..thin
    };
    let no_panes = Policy {
        promote: Promote::Density {
            half_rows_per_mpx: 400.0,
            pane_rows_per_mpx: 0.0,
        },
        ..Policy::PRODUCTION
    };
    assert!(matches!(
        decide("one/block_25", Policy::PRODUCTION, &under_panes),
        Decision::Partial(_)
    ));
    assert_eq!(
        decide("one/block_25", no_panes, &under_panes),
        Decision::Full
    );
    assert_eq!(
        decide("one/block_15", Policy::PRODUCTION, &thin),
        Decision::Full
    );
    let wider_half = Policy {
        promote: Promote::Density {
            half_rows_per_mpx: 8.0,
            pane_rows_per_mpx: 0.0,
        },
        ..Policy::PRODUCTION
    };
    assert!(matches!(
        decide("one/block_15", wider_half, &thin),
        Decision::Partial(_)
    ));
    let dense = FrameFacts {
        rows_per_mpx: 7_200.0,
        ..FACTS
    };
    assert_eq!(
        decide("one/block_75", Policy::PRODUCTION, &dense),
        Decision::Full
    );
}

#[test]
fn the_grid_holds_production_and_every_combination() {
    let grid = Policy::grid();
    assert_eq!(grid.len(), 9 * 4 * (9 + 10 * 7));
    assert_eq!(grid.iter().filter(|p| **p == Policy::PRODUCTION).count(), 1);
}

/// Bell's numbers: 1, 2, 5, 15.
#[test]
fn partitions_are_bells_numbers() {
    assert_eq!(partitions(1), vec![vec![0]]);
    assert_eq!(partitions(2), vec![vec![0, 0], vec![0, 1]]);
    assert_eq!(partitions(3).len(), 5);
    assert_eq!(partitions(4).len(), 15);
}

/// A policy looks its arm up, never a new measurement: on every stage, for every scenario, the arm
/// each policy names holds exactly what that policy decides, with no arm listed twice.
#[test]
fn every_policy_lands_on_an_arm_of_its_own_decisions() {
    let policies = Policy::grid();
    for stage in Stage::ALL {
        let facts = FrameFacts {
            surface: stage.surface(),
            ..FACTS
        };
        for scenario in catalog().iter().filter(|s| stage.runs(s.scope)) {
            let raw = stage.raw_frames(scenario);
            let arms = Arms::of(&raw, &facts, &policies);
            for (policy, &arm) in policies.iter().zip(&arms.of_policy) {
                for (frame, decision) in raw.iter().zip(&arms.arms[arm].frames) {
                    assert_eq!(policy.decide(frame, &facts), *decision, "{}", scenario.name);
                }
            }
            for (idx, arm) in arms.arms.iter().enumerate() {
                let earlier = &arms.arms[..idx];
                assert!(!arm.label.contains(','), "a CSV cell: {}", arm.label);
                assert!(
                    !earlier
                        .iter()
                        .any(|e| e.frames == arm.frames || e.label == arm.label),
                    "{}: {} twice",
                    scenario.name,
                    arm.label
                );
            }
        }
    }
}

/// Three rects have 5 partitions and the whole repaint, each one arm, under names that say which
/// raw rects each output rect holds: the spinner lies between the caret and the clock, so their
/// bounds hold it too (`ABC|B`). Two far icons have 2 and the repaint, and one rect only itself.
#[test]
fn arms_cover_each_partition_of_a_few_rects() {
    let labels = |name: &str| {
        let mut labels: Vec<String> = Arms::of(&named(name).frames, &FACTS, &Policy::grid())
            .arms
            .into_iter()
            .map(|arm| arm.label)
            .collect();
        labels.sort();
        labels
    };
    assert_eq!(
        labels("triple/scattered"),
        ["ABC", "ABC|B", "AB|C", "A|BC", "A|B|C", "full"].map(String::from)
    );
    assert_eq!(
        labels("pair/icons_gap400"),
        ["AB", "A|B", "full"].map(String::from)
    );
    assert_eq!(labels("one/block_15"), ["A", "full"].map(String::from));
}

/// A fold sees each rect once, clipped: a repeat and a rect off the window add no arm, and a rect
/// the window cuts in half repaints its half.
#[test]
fn arms_read_what_is_visible() {
    let on = Rect::new(10.0, 10.0, 50.0, 50.0);
    let raw = vec![vec![
        on,
        on,
        Rect::new(5000.0, 0.0, 9.0, 9.0),
        Rect::new(1270.0, 0.0, 20.0, 20.0),
    ]];
    let arms = Arms::of(&raw, &FACTS, &Policy::grid());
    let unmerged = Decision::partial(vec![on, Rect::new(1270.0, 0.0, 10.0, 20.0)]);
    assert!(
        arms.arms
            .iter()
            .any(|arm| arm.frames == [unmerged.clone()] && arm.label == "A|B")
    );
    let mut labels: Vec<&str> = arms.arms.iter().map(|arm| arm.label.as_str()).collect();
    labels.sort_unstable();
    assert_eq!(labels, ["AB", "A|B", "full"]);
}

/// Past eight raw rects an arm's name is a count, the singular read as one.
#[test]
fn a_many_rect_arm_is_named_by_its_count() {
    let labels: Vec<String> = Arms::of(&named("many/rows_32").frames, &FACTS, &Policy::grid())
        .arms
        .into_iter()
        .map(|arm| arm.label)
        .collect();
    assert!(labels.contains(&"1 rect".to_owned()), "{labels:?}");
    assert!(!labels.contains(&"1 rects".to_owned()), "{labels:?}");
}

/// Even rounds forward, odd ones backward, so each arm's mean position in a pair of rounds is the
/// same. A turn plays whole cycles: 8 frames of a 16-frame cycle are 16, 3 of a 2-frame cycle 4.
#[test]
fn arms_take_turns_both_ways_in_whole_cycles() {
    let order = |round| {
        (0..3)
            .map(|turn| arm_of_turn(round, turn, 3))
            .collect::<Vec<_>>()
    };
    assert_eq!(order(0), [0, 1, 2]);
    assert_eq!(order(1), [2, 1, 0]);
    assert_eq!(order(2), [0, 1, 2]);
    assert_eq!(played_frames(8, 1), 8);
    assert_eq!(played_frames(8, 16), 16);
    assert_eq!(played_frames(3, 2), 4);
    assert_eq!(played_frames(1, 2), 2);
    assert_eq!(played_frames(4, 2), 4);
}

/// A filter reads `<stage>/<scenario>`, past what the stage runs at all.
#[test]
fn a_filter_selects_by_stage_and_scenario() {
    let all = catalog();
    let names = |stage, filter| -> Vec<String> {
        selected(stage, &all, filter)
            .into_iter()
            .map(|s| s.name.clone())
            .collect()
    };
    assert_eq!(
        names(Stage::SCENE, Some("scene/pair/l_")),
        ["pair/l_touching", "pair/l_overlap"]
    );
    assert!(names(Stage::SCENE, Some("dense/pair/l_")).is_empty());
    assert_eq!(names(Stage::SCENE, None).len(), all.len());
    let plain = Stage::ALL[2];
    assert_eq!(plain.name, "plain");
    assert_eq!(
        names(plain, Some("plain/pair/l_")),
        ["pair/l_touching"],
        "l_overlap is not core"
    );
}

/// 8 spinners take 3 columns of 3 rows, the first at the middle of its 426⅔ × 240 cell less 12.
#[test]
fn spinners_spread_over_the_window() {
    let frame = &named("many/spinners_08").frames[0];
    assert_eq!(frame.len(), 8);
    assert_eq!(
        frame[0],
        Rect::new(1280.0 / 3.0 * 0.5 - 12.0, 120.0 - 12.0, 24.0, 24.0)
    );
    assert_eq!(frame[7].min.y, 720.0 / 3.0 * 2.5 - 12.0);
}

/// A cycle's arm names each frame, a repeat as a count: 15 carets, then the whole window.
#[test]
fn a_cycle_names_its_frames_in_runs() {
    let arms = Arms::of(&named("seq/spike_1_in_16").frames, &FACTS, &Policy::grid());
    let production = Policy::production_idx(&Policy::grid());
    assert_eq!(arms.arms[arms.of_policy[production]].label, "A*15/full");
}

/// Medians of [3, 1, 2] and [10, 20] µs are 2 and 15, whose mean is 8.5. The rounds stay as they
/// are, for a pair across arms.
#[test]
fn a_metric_cost_reads_medians_of_its_positions() {
    let rounds = vec![us(4), us(1), us(3), us(2)];
    let cost = MetricCost::of(
        &mut [vec![3_000, 1_000, 2_000], vec![10_000, 20_000]],
        rounds.clone(),
    );
    assert_eq!(
        cost,
        MetricCost {
            positions: vec![us(2), us(15)],
            mean: Duration::from_nanos(8_500),
            rounds,
        }
    );
}

/// An arm's frames under each metric. A frame of 3 µs CPU and 1 µs GPU, then 2 and 5, adds to 4 and
/// 7 and reads 3 and 5 at its longer half, over one round of two positions; its medians of each
/// half are 2.5 and 3.
#[test]
fn samples_read_each_metric_and_each_half() {
    let time = |cpu_us, gpu_us| FrameTime {
        cpu: us(cpu_us),
        gpu: us(gpu_us),
    };
    let mut samples = ArmSamples::new(
        2,
        Turns {
            rounds: 1,
            frames: 2,
        },
    );
    samples.start_round();
    samples.push(0, time(3, 1));
    samples.push(1, time(2, 5));
    let cost = samples.cost();
    assert_eq!(cost.metrics[0].positions, [us(4), us(7)]);
    assert_eq!(cost.metrics[0].rounds, [Duration::from_nanos(5_500)]);
    assert_eq!(cost.metrics[1].positions, [us(3), us(5)]);
    assert_eq!(cost.metrics[1].rounds, [us(4)]);
    assert_eq!(cost.cpu, Duration::from_nanos(2_500));
    assert_eq!(cost.gpu, us(3));
}

const fn us(micros: u64) -> Duration {
    Duration::from_micros(micros)
}

/// An arm with a mean and each round's mean in µs, alike under both metrics.
fn cost(mean_us: u64, rounds_us: &[u64]) -> ArmCost {
    let metric = MetricCost {
        positions: vec![us(mean_us)],
        mean: us(mean_us),
        rounds: rounds_us.iter().copied().map(us).collect(),
    };
    ArmCost {
        metrics: [metric.clone(), metric],
        cpu: us(mean_us),
        gpu: Duration::ZERO,
    }
}

/// Two policies over two scenarios, each arm steady, so a gap reads whole.
///
/// - `a`, weight 1: arms at 100 and 110, a gap of 10, 10 %.
/// - `b`, weight 3: arms at 200 and 203, a gap of 3, 1.5 %.
///
/// Policy 0 takes `a`'s best and `b`'s worse: worst 3, mean 3 × 3 / 4 = 2.25 µs and
/// 1.5 × 3 / 4 = 1.125 %. Policy 1 takes `a`'s worse and `b`'s best: worst 10, mean 2.5 µs and 2.5 %.
#[test]
fn a_policy_scores_its_worst_and_weighted_mean_regret() {
    let scenarios = [
        ScenarioCosts::new(
            "a".into(),
            1,
            vec![cost(100, &[100]), cost(110, &[110])],
            vec![0, 1],
        ),
        ScenarioCosts::new(
            "b".into(),
            3,
            vec![cost(200, &[200]), cost(203, &[203])],
            vec![1, 0],
        ),
    ];
    assert_eq!(
        score(&scenarios, 2),
        [
            PolicyScore {
                worst_regret: us(3),
                worst_regret_pct: 1.5,
                worst_scenario: Some(1),
                mean_regret: Duration::from_nanos(2_250),
                mean_regret_pct: 1.125,
                mean_longer_pct: 1.125,
            },
            PolicyScore {
                worst_regret: us(10),
                worst_regret_pct: 10.0,
                worst_scenario: Some(0),
                mean_regret: Duration::from_nanos(2_500),
                mean_regret_pct: 2.5,
                mean_longer_pct: 2.5,
            },
        ]
    );
}

/// A gap reads round for round, so a device that slows between rounds moves both arms: rounds of
/// 100/102, 200/202, 300/302 are a steady gap of 2, where their own medians alone could not tell.
/// Gaps of 2 with a spread of 4 lie inside it: rounds 100/96, 100/104, 100/102, 100/102 sort their
/// gaps to −4, 2, 2, 4, of median 2 and half interquartile range (2 − (−4)) / 2 = 3, a tie. An arm
/// faster than the best by its mean reads a negative gap, which is no regret.
#[test]
fn a_regret_pairs_rounds_and_a_gap_inside_the_noise_reads_as_none() {
    let of = |rounds_a: &[u64], rounds_b: &[u64]| {
        ScenarioCosts::new(
            "a".into(),
            1,
            vec![cost(100, rounds_a), cost(101, rounds_b)],
            vec![1],
        )
    };
    let steady = of(&[100, 200, 300], &[102, 202, 302]);
    assert_eq!(steady.sum().regrets[1].regret, us(2));
    assert_eq!(steady.sum().regrets[1].noise, Duration::ZERO);
    assert_eq!(steady.sum().regrets[0].regret, Duration::ZERO);
    let noisy = of(&[100, 100, 100, 100], &[96, 104, 102, 102]);
    assert_eq!(noisy.sum().regrets[1].noise, us(3));
    assert_eq!(noisy.sum().regrets[1].regret, Duration::ZERO);
    let faster = of(&[100, 100, 100], &[90, 90, 90]);
    assert_eq!(faster.sum().regrets[1].regret, Duration::ZERO);
    assert_eq!(
        score(&[noisy], 1),
        [PolicyScore {
            worst_regret: Duration::ZERO,
            worst_regret_pct: 0.0,
            worst_scenario: None,
            mean_regret: Duration::ZERO,
            mean_regret_pct: 0.0,
            mean_longer_pct: 0.0,
        }]
    );
}

/// Beating is strict: a lower worst, or an equal worst and a lower mean. The ties of a score that
/// did not move stand beside it, whatever order a sort left them in.
#[test]
fn only_a_strictly_better_score_beats_a_policy() {
    let of = |worst_us, mean_us| PolicyScore {
        worst_regret: us(worst_us),
        worst_regret_pct: 0.0,
        worst_scenario: None,
        mean_regret: us(mean_us),
        mean_regret_pct: 0.0,
        mean_longer_pct: 0.0,
    };
    let scores = [of(5, 2), of(5, 2), of(5, 1), of(4, 9), of(6, 0)];
    assert_eq!(beating(&scores, 0), 2);
    assert_eq!(beating(&scores, 3), 0);
    assert_eq!(beating(&scores, 4), 4);
}

/// Each pixel counts once: two 10² squares sharing 5² are 175, one inside another 100, apart 200.
#[test]
fn union_area_counts_each_pixel_once() {
    let a = Rect::new(0.0, 0.0, 10.0, 10.0);
    assert_eq!(union_area(&[]), 0.0);
    assert_eq!(union_area(&[a]), 100.0);
    assert_eq!(union_area(&[a, Rect::new(5.0, 5.0, 10.0, 10.0)]), 175.0);
    assert_eq!(union_area(&[a, Rect::new(2.0, 2.0, 3.0, 3.0)]), 100.0);
    assert_eq!(union_area(&[a, Rect::new(20.0, 0.0, 10.0, 10.0)]), 200.0);
}

/// Under a budget of 30, `a` joins `b` and `b` joins `c`, each wasting 20, while `a` and `c`, or a
/// pair and the third, waste 164 or more: whichever pair folds first holds, so the three end two ways.
/// Rects in one row at equal gaps chain whatever the order, and end one way.
#[test]
fn order_outcomes_finds_a_fold_that_depends_on_its_order() {
    let policy = Policy {
        merge: MergePolicy {
            budget_px: 30.0,
            rect_cap: DAMAGE_RECT_CAP,
        },
        promote: Promote::Coverage(0.9),
    };
    let facts = FrameFacts {
        surface: Rect::new(0.0, 0.0, 100.0, 100.0),
        ..FACTS
    };
    let hook = [
        Rect::new(0.0, 0.0, 10.0, 10.0),
        Rect::new(12.0, 0.0, 10.0, 10.0),
        Rect::new(12.0, 12.0, 10.0, 10.0),
    ];
    assert_eq!(order_outcomes(&hook, &policy, &facts), 2);
    let row = [
        Rect::new(0.0, 0.0, 10.0, 10.0),
        Rect::new(12.0, 0.0, 10.0, 10.0),
        Rect::new(24.0, 0.0, 10.0, 10.0),
    ];
    assert_eq!(order_outcomes(&row, &policy, &facts), 1);
}

/// The dense stage at 4K holds the reference window at 1.5×, the 1× one at 1×. A glass frame's rect
/// that touches a pane adds the pane's rect after it, as a stale pane's joins a real frame's damage;
/// rects far from both add none.
#[test]
fn a_stage_scales_rects_and_adds_the_panes_they_touch() {
    let stage = |name: &str| Stage::ALL.into_iter().find(|s| s.name == name).unwrap();
    let sidebar = named("pair/bars_left_right");
    assert_eq!(stage("dense_1x").surface(), SURFACE);
    assert_eq!(
        stage("dense_4k").surface(),
        Rect::new(0.0, 0.0, 1920.0, 1080.0)
    );
    assert_eq!(
        stage("dense_4k").raw_frames(&sidebar)[0][0],
        Rect::new(0.0, 0.0, 360.0, 1080.0)
    );
    assert_eq!(
        stage("glass").raw_frames(&named("pair/corners"))[0].len(),
        2
    );
    // The middle block reaches the middle pane, (500, 200) + 320 × 220, and not the one at
    // (40, 400) + 240 × 240.
    // A square under the toolbar reaches it, and its rect reaches the sidebar's footprint, as a
    // walk stales the panes above a stale one; the popup stays out of both.
    assert_eq!(
        stage("scene_glass").raw_frames(&named("one/under_toolbar"))[0],
        [
            Rect::new(300.0, 20.0, 12.0, 12.0),
            Rect::new(0.0, 0.0, 1280.0, 48.0),
            Rect::new(0.0, 48.0, 240.0, 672.0),
        ]
    );
    let touched = &stage("glass").raw_frames(&named("one/block_05"))[0];
    assert_eq!(touched.len(), 2);
    assert_eq!(touched[1], Rect::new(500.0, 200.0, 320.0, 220.0));
}

/// A scene counts the rows that draw on its surface: a filled block, and neither a block far off the
/// window nor the marker its parent holds for each child.
#[test]
fn a_scene_counts_the_rows_that_draw_on_the_surface() {
    let count = |far_blocks: u32| {
        let mut h = Harness::builder(glam::UVec2::new(100, 100)).mono().build();
        let block = |ui: &mut Ui, salt: u32, at: (f32, f32)| {
            Block::new()
                .id_salt(("visible-rows", salt))
                .at(at)
                .size((Sizing::fixed(10.0), Sizing::fixed(10.0)))
                .chrome(Chrome::new(RgbaF32::WHITE))
                .show(ui);
        };
        h.frame(|ui| {
            Panel::canvas()
                .id_salt("visible-rows")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    block(ui, 0, (10.0, 10.0));
                    for far in 0..far_blocks {
                        block(ui, far + 1, (500.0, 500.0));
                    }
                });
        });
        h.frames().ui().frame_scene().visible_row_count()
    };
    assert_eq!(count(0), 1);
    assert_eq!(count(3), 1);
}

/// Two panes over a whole window read a share of 2, each counted alone, and none reads 0.
#[test]
fn a_scene_reads_the_share_its_panes_hold() {
    let share = |pane_count| {
        let mut h = Harness::builder(glam::UVec2::new(100, 100)).mono().build();
        let record = |ui: &mut Ui| {
            Panel::canvas()
                .id_salt("share-panes")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    for _ in 0..pane_count {
                        ui.add_shape(
                            Shape::rect().at(Rect::new(0.0, 0.0, 100.0, 100.0)),
                            Paint::from(RgbaF32::TRANSPARENT).with_material(Material::blur(2.0)),
                        );
                    }
                });
        };
        h.frame(record);
        h.frames().ui().frame_scene().pane_share()
    };
    assert_eq!(share(0), 0.0);
    assert_eq!(share(1), 1.0);
    assert_eq!(share(2), 2.0);
}

/// Which scopes a stage runs: core everywhere, the catalog on the two bench-surface screens, and a
/// session's frames only on the screen they came from.
#[test]
fn a_stage_runs_the_scopes_it_can_read() {
    for stage in Stage::ALL {
        let primary = matches!(stage.name, "scene" | "dense");
        assert!(stage.runs(Scope::Core), "{}", stage.name);
        assert_eq!(stage.runs(Scope::Primary), primary, "{}", stage.name);
        assert_eq!(
            stage.runs(Scope::Captured),
            stage.name == "scene",
            "{}",
            stage.name
        );
    }
    assert_eq!(Stage::SCENE.physical, BENCH_SURFACE);
    assert_eq!(Stage::SCENE.scale, BENCH_DPR);
}

#[test]
fn args_parse_as_cargo_passes_them() {
    let parse = |args: &[&str]| SweepArgs::parse(args.iter().map(ToString::to_string));
    // `cargo test` passes no `--bench`: one round of one frame.
    let once = parse(&[]).unwrap();
    assert_eq!((once.rounds, once.frames), (1, 1));
    let timed = parse(&["--bench"]).unwrap();
    assert_eq!((timed.rounds, timed.frames), (12, 8));
    let set = parse(&[
        "pair/", "--rounds", "3", "--frames", "2", "--list", "--csv", "out", "--bench",
    ])
    .unwrap();
    assert_eq!((set.rounds, set.frames), (3, 2));
    assert_eq!(set.filter.as_deref(), Some("pair/"));
    assert!(set.list);
    assert_eq!(set.csv.as_deref().and_then(|p| p.to_str()), Some("out"));
    assert!(matches!(parse(&["--nope"]), Err(SweepError::UnknownFlag(flag)) if flag == "--nope"));
    assert!(matches!(
        parse(&["a", "b"]),
        Err(SweepError::SecondFilter(second)) if second == "b"
    ));
    assert!(matches!(
        parse(&["--csv"]),
        Err(SweepError::MissingValue("--csv"))
    ));
    assert!(matches!(
        parse(&["--rounds"]),
        Err(SweepError::MissingValue("--rounds"))
    ));
    assert!(matches!(
        parse(&["--rounds", "0"]),
        Err(SweepError::NotACount {
            flag: "--rounds",
            ..
        })
    ));
    assert!(matches!(
        parse(&["--frames", "x"]),
        Err(SweepError::NotACount {
            flag: "--frames",
            ..
        })
    ));
}

fn bits(x: f32) -> [u32; 4] {
    [x, 0.0, 10.0, 10.0].map(f32::to_bits)
}

/// Three frames' rects seen 2, 5 and 1 times: the most frequent first, each weighing its count and
/// named by its bounds. A frame of the same rects in another order counts as the first, and plays
/// in the order first seen.
#[test]
fn a_session_keeps_its_most_frequent_frames() {
    let mut seen = SeenFrames::new();
    for _ in 0..2 {
        note(&mut seen, vec![bits(1.0)]);
    }
    for _ in 0..5 {
        note(&mut seen, vec![bits(2.0)]);
    }
    note(&mut seen, vec![bits(40.0), bits(3.0)]);
    let mut merged = seen;
    note(&mut merged, vec![bits(3.0), bits(40.0)]);
    let captured = kept(merged);
    assert_eq!(captured.frame_count, 9);
    assert_eq!(captured.distinct_count, 3);
    assert_eq!(captured.kept_frame_count, 9);
    let got: Vec<(&str, u32)> = captured
        .scenarios
        .iter()
        .map(|s| (s.name.as_str(), s.weight))
        .collect();
    assert_eq!(
        got,
        [
            ("captured/2_0_10x10_1r", 5),
            ("captured/1_0_10x10_1r", 2),
            ("captured/3_0_47x10_2r", 2),
        ]
    );
    assert_eq!(
        captured.scenarios[2].frames,
        [[
            Rect::new(40.0, 0.0, 10.0, 10.0),
            Rect::new(3.0, 0.0, 10.0, 10.0)
        ]]
    );
    assert_eq!(captured.scenarios[0].scope, Scope::Captured);
}

/// Two frames of one bounds and count differ by a suffix, set by their rects: the counts swapped,
/// each keeps its name.
#[test]
fn frames_of_one_name_keep_their_suffix_as_counts_move() {
    let tall = [0.0, 0.0, 10.0, 40.0].map(f32::to_bits);
    let row = [0.0, 30.0, 10.0, 10.0].map(f32::to_bits);
    let corner = [0.0, 0.0, 10.0, 10.0].map(f32::to_bits);
    let lower = [0.0, 20.0, 10.0, 20.0].map(f32::to_bits);
    let names = |first: u32, second: u32| {
        let mut seen = SeenFrames::new();
        for _ in 0..first {
            note(&mut seen, vec![tall, row]);
        }
        for _ in 0..second {
            note(&mut seen, vec![corner, lower]);
        }
        let mut by_rects: Vec<(String, Rect)> = kept(seen)
            .scenarios
            .into_iter()
            .map(|s| (s.name, s.frames[0][0]))
            .collect();
        by_rects.sort_by_key(|(name, _)| name.clone());
        by_rects
    };
    assert_eq!(names(5, 1), names(1, 5));
    assert_eq!(names(5, 1)[0].0, "captured/0_0_10x40_2r");
    assert_eq!(names(5, 1)[1].0, "captured/0_0_10x40_2r #2");
}

/// Past the cap the least frequent go, and the count says how much of the session stays: 17 frames
/// seen once each but the first, seen twice, keep 16 for 17 of 18 frames.
#[test]
fn a_session_keeps_no_more_than_its_cap() {
    let mut seen = SeenFrames::new();
    note(&mut seen, vec![bits(0.0)]);
    for x in 0..17 {
        note(&mut seen, vec![bits(x as f32)]);
    }
    let captured = kept(seen);
    assert_eq!(captured.frame_count, 18);
    assert_eq!(captured.distinct_count, 17);
    assert_eq!(captured.scenarios.len(), 16);
    assert_eq!(captured.kept_frame_count, 17);
    assert_eq!(captured.scenarios[0].weight, 2);
    assert_eq!(captured.scenarios[0].name, "captured/0_0_10x10_1r");
}

/// Two buttons and a field, a screen small enough for a session in a test.
#[derive(Debug, Default)]
struct SmallScene {
    text: String,
}

impl Scene for SmallScene {
    fn record(&mut self, ui: &mut Ui) {
        Panel::vstack()
            .id_salt("small-scene")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for id in self.hover_targets() {
                    Button::new().id(id).label("target").show(ui);
                }
                let field = self.field();
                TextEdit::new(&mut self.text)
                    .id(field)
                    .size((Sizing::fixed(200.0), Sizing::fixed(30.0)))
                    .show(ui);
            });
    }

    fn hover_targets(&self) -> [WidgetId; 2] {
        [
            WidgetId::from_hash("small-a"),
            WidgetId::from_hash("small-b"),
        ]
    }

    fn field(&self) -> WidgetId {
        WidgetId::from_hash("small-field")
    }

    fn field_len(&self) -> usize {
        self.text.len()
    }
}

/// A session on a screen with nothing of its own in real time repeats frame for frame, so what moves
/// in a run of the showcase comes from the showcase.
#[test]
fn a_session_repeats_on_a_stepped_clock() {
    let once = || {
        let harness = Harness::builder(BENCH_SURFACE)
            .system_scale(BENCH_DPR)
            .mono()
            .build();
        session(harness, &mut SmallScene::default())
    };
    let weighed = |captured: Captured| -> Vec<(String, u32)> {
        captured
            .scenarios
            .into_iter()
            .map(|s| (s.name, s.weight))
            .collect()
    };
    assert_eq!(weighed(once()), weighed(once()));
}

/// A session reaches what it aims at: its keys land in the field, and it keeps frames that paint,
/// each with rects on the window, in order of frequency.
#[test]
fn a_session_records_what_its_script_damages() {
    let harness = Harness::builder(BENCH_SURFACE)
        .system_scale(BENCH_DPR)
        .mono()
        .build();
    let mut scene = SmallScene::default();
    let captured = session(harness, &mut scene);
    assert_eq!(scene.field_len(), "the quick brown fox".len());
    assert!(captured.frame_count > 0, "the session painted nothing");
    assert!(captured.kept_frame_count <= captured.frame_count);
    let window = Rect::new(0.0, 0.0, REFERENCE.x, REFERENCE.y);
    let weights: Vec<u32> = captured.scenarios.iter().map(|s| s.weight).collect();
    assert!(weights.is_sorted_by(|a, b| a >= b), "{weights:?}");
    for rect in captured
        .scenarios
        .iter()
        .flat_map(|s| s.frames.iter().flatten())
    {
        assert!(window.contains_rect(*rect), "{rect:?}");
    }
}
