//! What a sweep prints and writes.

use crate::damage::sweep::arms::visible;
use crate::damage::sweep::error::SweepError;
use crate::damage::sweep::policy::{Decision, FrameFacts, Policy, union_area};
use crate::damage::sweep::scenario::{Ranqd1, Scenario};
use crate::damage::sweep::score::{PolicyScore, StageResults};
use crate::damage::sweep::stage::Stage;
use crate::primitives::geometry::rect::Rect;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;

/// How a column of a table lines up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Align {
    Left,
    Right,
}

/// Print `rows` under `heads`, each column as wide as its widest cell.
fn table(out: &mut dyn Write, heads: &[(&str, Align)], rows: &[Vec<String>]) -> io::Result<()> {
    let mut widths: Vec<usize> = heads.iter().map(|(head, _)| head.chars().count()).collect();
    for row in rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    let mut line = |cells: &mut dyn Iterator<Item = &str>| {
        let mut text = String::new();
        for (idx, cell) in cells.enumerate() {
            let width = widths[idx];
            let cell_width = cell.chars().count();
            let pad = " ".repeat(width.saturating_sub(cell_width));
            if idx > 0 {
                text.push_str("  ");
            }
            match heads[idx].1 {
                Align::Left => {
                    text.push_str(cell);
                    text.push_str(&pad);
                }
                Align::Right => {
                    text.push_str(&pad);
                    text.push_str(cell);
                }
            }
        }
        writeln!(out, "{}", text.trim_end())
    };
    line(&mut heads.iter().map(|(head, _)| *head))?;
    for row in rows {
        line(&mut row.iter().map(String::as_str))?;
    }
    Ok(())
}

/// A time's µs in a CSV cell.
fn us_cell(time: Duration) -> String {
    format!("{:.2}", time.as_secs_f64() * 1e6)
}

/// A time in whole µs, with one digit under 100 µs, where the digit still says something.
fn us(time: Duration) -> String {
    let value = time.as_secs_f64() * 1e6;
    if value < 100.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.0}")
    }
}

/// A regret: `tie` when it lies within the noise, which a `0` would read as measured to be none.
fn regret_text(regret: Duration) -> String {
    if regret.is_zero() {
        "tie".into()
    } else {
        us(regret)
    }
}

fn budget_text(budget_px: f32) -> String {
    if budget_px.is_finite() {
        format!("{budget_px:.0}")
    } else {
        "inf".into()
    }
}

/// A share as a percent of two digits, `<0.01` where those would read as nothing changed.
fn percent(share: f32) -> String {
    let pct = share * 100.0;
    if pct > 0.0 && pct < 0.01 {
        "<0.01".into()
    } else {
        format!("{pct:.2}")
    }
}

/// How many distinct decisions `raw` ends in over the orders a fold can take it: every order up to
/// four rects, past that a fixed set of shuffles, as the orders outgrow any run.
pub(super) fn order_outcomes(raw: &[Rect], policy: &Policy, facts: &FrameFacts) -> usize {
    const SHUFFLES: u32 = 24;
    fn permutations(rects: &mut Vec<Rect>, from: usize, each: &mut impl FnMut(&[Rect])) {
        if from + 1 >= rects.len() {
            each(rects);
            return;
        }
        for at in from..rects.len() {
            rects.swap(from, at);
            permutations(rects, from + 1, each);
            rects.swap(from, at);
        }
    }
    let mut seen: Vec<Decision> = Vec::new();
    let mut note = |rects: &[Rect]| {
        let decision = policy.decide(rects, facts);
        if !seen.contains(&decision) {
            seen.push(decision);
        }
    };
    let mut rects = raw.to_vec();
    if rects.len() <= 4 {
        permutations(&mut rects, 0, &mut note);
    } else {
        let mut sequence = Ranqd1::new(0x9E37_79B9);
        for _ in 0..SHUFFLES {
            for at in (1..rects.len()).rev() {
                rects.swap(at, sequence.next_bits() as usize % (at + 1));
            }
            note(&rects);
        }
    }
    seen.len()
}

/// The surface `raw` changes, each pixel once, as a share: what lies off it changes no pixel.
fn changed_share(raw: &[Rect], surface: Rect) -> f32 {
    union_area(&visible(raw, surface)) / surface.area()
}

/// The surface a decision repaints, each pixel once, as a share.
fn repaint_share(decision: &Decision, surface: Rect) -> f32 {
    match decision {
        Decision::Full => 1.0,
        Decision::Partial(rects) => union_area(rects) / surface.area(),
    }
}

/// Print what [`Policy::PRODUCTION`] decides for each scenario on `stage`, with no device.
pub(super) fn decisions(
    out: &mut impl Write,
    stage: Stage,
    facts: &FrameFacts,
    scenarios: &[Scenario],
) -> io::Result<()> {
    let surface = facts.surface;
    let production = Policy::PRODUCTION;
    let mut rows: Vec<Vec<String>> = Vec::new();
    for scenario in scenarios {
        let frames = stage.raw_frames(scenario);
        for (position, raw) in frames.iter().enumerate() {
            // A run of one frame reads once, under the position it starts at.
            if position > 0 && frames[position - 1] == *raw {
                continue;
            }
            let decision = production.decide(raw, facts);
            rows.push(vec![
                if position == 0 {
                    scenario.name.clone()
                } else {
                    format!("  +{position}")
                },
                raw.len().to_string(),
                match &decision {
                    Decision::Full => "full".to_owned(),
                    Decision::Partial(rects) => rects.len().to_string(),
                },
                percent(changed_share(raw, surface)),
                percent(repaint_share(&decision, surface)),
                order_outcomes(raw, &production, facts).to_string(),
            ]);
        }
    }
    writeln!(
        out,
        "\nwhat production decides on {}: budget {} px², cap {}, promote {}, {:.0} rows per Mpx",
        stage.name,
        budget_text(production.merge.budget_px),
        production.merge.rect_cap,
        production.promote,
        facts.rows_per_mpx,
    )?;
    writeln!(
        out,
        "changed %: the pixels that changed. repainted %: what the decision repaints. orders: \
         distinct decisions over input orders."
    )?;
    table(
        out,
        &[
            ("scenario", Align::Left),
            ("raw", Align::Right),
            ("out", Align::Right),
            ("changed %", Align::Right),
            ("repainted %", Align::Right),
            ("orders", Align::Right),
        ],
        &rows,
    )
}

/// Print each scenario's production arm against its best arm on `stage`.
pub(super) fn arms_table(out: &mut impl Write, results: &StageResults) -> io::Result<()> {
    let rows: Vec<Vec<String>> = results
        .costs
        .iter()
        .zip(&results.arms)
        .map(|(costs, arms)| {
            let picked = costs.of_policy[results.production];
            let standing = costs.sum();
            let best = standing.best;
            let whole = arms
                .iter()
                .position(|arm| arm.frames.iter().all(|frame| *frame == Decision::Full))
                .expect("a scenario has a whole repaint");
            let regret = standing.regrets[picked].regret;
            let pct = regret.as_secs_f64() / costs.costs[best].sum().mean.as_secs_f64() * 100.0;
            vec![
                costs.name.clone(),
                arms.len().to_string(),
                arms[picked].label.clone(),
                us(costs.costs[picked].sum().mean),
                us(costs.costs[picked].cpu),
                arms[best].label.clone(),
                us(costs.costs[best].sum().mean),
                us(costs.costs[best].cpu),
                us(costs.costs[whole].sum().mean),
                regret_text(regret),
                if regret.is_zero() {
                    String::new()
                } else {
                    format!("{pct:.1}")
                },
                us(standing.regrets[picked].noise),
            ]
        })
        .collect();
    table(
        out,
        &[
            ("scenario", Align::Left),
            ("arms", Align::Right),
            ("production", Align::Left),
            ("µs", Align::Right),
            ("cpu", Align::Right),
            ("best", Align::Left),
            ("µs", Align::Right),
            ("cpu", Align::Right),
            ("full µs", Align::Right),
            ("regret µs", Align::Right),
            ("%", Align::Right),
            ("noise µs", Align::Right),
        ],
        &rows,
    )
}

/// One row of a policy table.
fn policy_row(policy: &Policy, score: &PolicyScore, results: &StageResults) -> Vec<String> {
    vec![
        budget_text(policy.merge.budget_px),
        policy.merge.rect_cap.to_string(),
        policy.promote.to_string(),
        regret_text(score.worst_regret),
        score
            .worst_scenario
            .map_or_else(String::new, |idx| results.costs[idx].name.clone()),
        us(score.mean_regret),
        format!("{:.2}", score.mean_regret_pct),
        format!("{:.2}", score.mean_longer_pct),
        if *policy == Policy::PRODUCTION {
            "production".into()
        } else {
            String::new()
        },
    ]
}

const POLICY_HEADS: [(&str, Align); 9] = [
    ("budget px²", Align::Right),
    ("cap", Align::Right),
    ("promote", Align::Right),
    ("worst µs", Align::Right),
    ("at", Align::Left),
    ("mean µs", Align::Right),
    ("mean %", Align::Right),
    ("longer %", Align::Right),
    ("", Align::Left),
];

/// The policies, the lowest worst regret first and the lowest mean among equals: a frame is
/// judged by its worst case, so a mean only breaks a tie.
fn ranked(scores: &[PolicyScore]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..scores.len()).collect();
    order.sort_by(|&a, &b| {
        scores[a]
            .worst_regret
            .cmp(&scores[b].worst_regret)
            .then(scores[a].mean_regret.cmp(&scores[b].mean_regret))
    });
    order
}

/// How many policies score strictly better than the one at `idx`: its rank alone would count the
/// ties an unmoved score leaves in front of it.
pub(super) fn beating(scores: &[PolicyScore], idx: usize) -> usize {
    let key = |score: &PolicyScore| (score.worst_regret, score.mean_regret);
    let own = key(&scores[idx]);
    scores.iter().map(key).filter(|other| *other < own).count()
}

/// Print the best policies of `policies`, how many beat production, and each knob swept alone with the
/// others at production's values.
pub(super) fn policies_table(
    out: &mut impl Write,
    policies: &[Policy],
    results: &StageResults,
) -> io::Result<()> {
    const TOP: usize = 5;
    let scores = &results.scores;
    let order = ranked(scores);
    writeln!(
        out,
        "\nbest of {} policies ({} score better than production, by worst then mean):",
        policies.len(),
        beating(scores, results.production)
    )?;
    let rows: Vec<Vec<String>> = order
        .iter()
        .take(TOP)
        .map(|&idx| policy_row(&policies[idx], &scores[idx], results))
        .collect();
    table(out, &POLICY_HEADS, &rows)?;
    let production = Policy::PRODUCTION;
    let sweep = |out: &mut dyn Write, title: &str, selects: &dyn Fn(&Policy) -> bool| {
        writeln!(out, "\n{title}:")?;
        let rows: Vec<Vec<String>> = policies
            .iter()
            .zip(scores)
            .filter(|(policy, _)| selects(policy))
            .map(|(policy, score)| policy_row(policy, score, results))
            .collect();
        table(out, &POLICY_HEADS, &rows)
    };
    let same_but = |p: &Policy, knob: &dyn Fn(&mut Policy)| {
        let mut p = *p;
        knob(&mut p);
        p == production
    };
    sweep(out, "budget alone", &|p| {
        same_but(p, &|p| p.merge.budget_px = production.merge.budget_px)
    })?;
    sweep(out, "cap alone", &|p| {
        same_but(p, &|p| p.merge.rect_cap = production.merge.rect_cap)
    })?;
    sweep(out, "promote alone", &|p| {
        same_but(p, &|p| p.promote = production.promote)
    })
}

/// The CSV rows of every stage so far: an arm per frame position, and a policy per line.
#[derive(Debug, Default)]
pub(super) struct Csv {
    arms: String,
    policies: String,
}

impl Csv {
    /// Add a stage's measurements.
    pub(super) fn add(&mut self, policies: &[Policy], results: &StageResults) {
        let stage = results.stage;
        for (costs, arms) in results.costs.iter().zip(&results.arms) {
            for (idx, (arm, cost)) in arms.iter().zip(&costs.costs).enumerate() {
                let standing = costs.sum();
                for (position, (position_time, frame)) in
                    cost.sum().positions.iter().zip(&arm.frames).enumerate()
                {
                    let (rect_count, rect_area) = match frame {
                        Decision::Full => (0, 0.0),
                        Decision::Partial(rects) => (rects.len(), union_area(rects)),
                    };
                    writeln!(
                        self.arms,
                        "{},{},{},{},{},{},{},{},{},{},{},{},{},{rect_count},{rect_area:.0}",
                        stage.name,
                        costs.name,
                        costs.weight,
                        arm.label,
                        idx == costs.of_policy[results.production],
                        idx == standing.best,
                        position,
                        us_cell(*position_time),
                        us_cell(cost.sum().mean),
                        us_cell(cost.cpu),
                        us_cell(cost.gpu),
                        us_cell(standing.regrets[idx].regret),
                        us_cell(standing.regrets[idx].noise),
                    )
                    .expect("a string takes every write");
                }
            }
        }
        for (policy, score) in policies.iter().zip(&results.scores) {
            writeln!(
                self.policies,
                "{},{},{},{},{},{:.2},{},{},{:.3},{:.3}",
                stage.name,
                budget_text(policy.merge.budget_px),
                policy.merge.rect_cap,
                policy.promote,
                us_cell(score.worst_regret),
                score.worst_regret_pct,
                score
                    .worst_scenario
                    .map_or("", |idx| results.costs[idx].name.as_str()),
                us_cell(score.mean_regret),
                score.mean_regret_pct,
                score.mean_longer_pct,
            )
            .expect("a string takes every write");
        }
    }

    /// Write `arms.csv` and `policies.csv` into `dir`, which it makes if missing.
    pub(super) fn write(&self, dir: &Path) -> Result<(), SweepError> {
        let write = |name: &str, head: &str, rows: &str| {
            let path = dir.join(name);
            fs::create_dir_all(dir)
                .and_then(|()| fs::write(&path, format!("{head}\n{rows}")))
                .map_err(|source| SweepError::Write { path, source })
        };
        write(
            "arms.csv",
            "stage,scenario,weight,arm,production,best,position,position_us,mean_us,cpu_us,gpu_us,regret_us,noise_us,rect_count,rect_area",
            &self.arms,
        )?;
        write(
            "policies.csv",
            "stage,budget_px,rect_cap,promote,worst_regret_us,worst_regret_pct,worst_scenario,mean_regret_us,mean_regret_pct,mean_longer_pct",
            &self.policies,
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::damage::region::{DAMAGE_RECT_CAP, MergePolicy};
    use crate::damage::sweep::arms::Arm;
    use crate::damage::sweep::error::SweepError;
    use crate::damage::sweep::policy::{Decision, Policy, Promote};
    use crate::damage::sweep::report::{
        Align, Csv, budget_text, changed_share, percent, ranked, regret_text, table, us, us_cell,
    };
    use crate::damage::sweep::score::{
        ArmCost, MetricCost, PolicyScore, ScenarioCosts, StageResults,
    };
    use crate::damage::sweep::stage::Stage;
    use crate::primitives::geometry::rect::Rect;
    use std::error::Error;
    use std::time::Duration;
    use std::{env, fs, process};

    /// Each column as wide as its widest cell, its numbers to the right, no trailing blanks.
    #[test]
    fn a_table_lines_its_columns_up() {
        let mut out = Vec::new();
        table(
            &mut out,
            &[
                ("name", Align::Left),
                ("µs", Align::Right),
                ("", Align::Left),
            ],
            &[
                vec!["a".to_owned(), "1000".to_owned(), "mark".to_owned()],
                vec!["longer".to_owned(), "5".to_owned(), String::new()],
            ],
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "name      µs\na       1000  mark\nlonger     5\n"
        );
    }

    /// What must not read as something else: a caret's 0.004 % as nothing, a regret inside the
    /// noise as measured to be none, an endless budget as a number.
    #[test]
    fn a_cell_reads_what_it_measured() {
        assert_eq!(percent(36.0 / 921_600.0), "<0.01");
        assert_eq!(percent(0.0), "0.00");
        assert_eq!(percent(0.0001), "0.01");
        assert_eq!(percent(1.0), "100.00");
        assert_eq!(regret_text(Duration::ZERO), "tie");
        assert_eq!(regret_text(Duration::from_nanos(32_740)), "32.7");
        assert_eq!(us(Duration::from_nanos(99_940)), "99.9");
        assert_eq!(us(Duration::from_nanos(100_400)), "100");
        assert_eq!(us_cell(Duration::from_nanos(1_234)), "1.23");
        assert_eq!(budget_text(20_000.0), "20000");
        assert_eq!(budget_text(f32::INFINITY), "inf");
    }

    /// A rect half off the window counts its half, and a repeat once: 10 × 20 of a 100 × 100 window
    /// is 2 %, whatever lies past its edge.
    #[test]
    fn a_change_counts_what_is_on_the_window() {
        let window = Rect::new(0.0, 0.0, 100.0, 100.0);
        let half_off = Rect::new(90.0, 0.0, 20.0, 20.0);
        assert_eq!(changed_share(&[half_off, half_off], window), 0.02);
        assert_eq!(
            changed_share(&[Rect::new(500.0, 0.0, 9.0, 9.0)], window),
            0.0
        );
    }

    /// The lowest worst first, the mean only between equals, however low it is elsewhere.
    #[test]
    fn policies_rank_by_their_worst_then_their_mean() {
        let of = |worst_us, mean_us| PolicyScore {
            worst_regret: Duration::from_micros(worst_us),
            worst_regret_pct: 0.0,
            worst_scenario: None,
            mean_regret: Duration::from_micros(mean_us),
            mean_regret_pct: 0.0,
            mean_longer_pct: 0.0,
        };
        assert_eq!(
            ranked(&[of(5, 2), of(4, 9), of(5, 1), of(6, 0)]),
            [1, 2, 0, 3]
        );
    }

    /// A folder it can make takes both files, the header before any row; a file where the folder
    /// should be fails with the path it could not write, and the cause.
    #[test]
    fn the_csv_writes_or_names_what_stopped_it() {
        let root = env::temp_dir().join(format!("palantir-sweep-csv-{}", process::id()));
        let dir = root.join("made");
        let csv = Csv {
            arms: "a-row\n".to_owned(),
            policies: "p-row\n".to_owned(),
        };
        csv.write(&dir).unwrap();
        let read = |name: &str| fs::read_to_string(dir.join(name)).unwrap();
        assert!(read("arms.csv").starts_with("stage,scenario,"));
        assert!(read("arms.csv").ends_with("\na-row\n"));
        assert!(read("policies.csv").starts_with("stage,budget_px,"));
        assert!(read("policies.csv").ends_with("\np-row\n"));
        let file = root.join("file");
        fs::write(&file, "").unwrap();
        let failed = csv.write(&file).unwrap_err();
        assert!(
            matches!(&failed, SweepError::Write { path, .. } if *path == file.join("arms.csv")),
            "{failed:?}"
        );
        assert!(failed.source().is_some());
        fs::remove_dir_all(&root).unwrap();
    }

    /// A row per arm and frame position, marked by who takes it, and a row per policy.
    #[test]
    fn the_csv_holds_each_position_and_policy() {
        let us = Duration::from_micros;
        let cost = |positions: [u64; 2], mean_us: u64, cpu_us: u64| {
            let metric = MetricCost {
                positions: positions.map(us).to_vec(),
                mean: us(mean_us),
                rounds: vec![us(mean_us)],
            };
            ArmCost {
                metrics: [metric.clone(), metric],
                cpu: us(cpu_us),
                gpu: us(mean_us - cpu_us),
            }
        };
        let arm = |label: &str, frames| Arm {
            label: label.to_owned(),
            frames,
        };
        let rect = Rect::new(0.0, 0.0, 10.0, 10.0);
        let other = Policy {
            merge: MergePolicy {
                budget_px: f32::INFINITY,
                rect_cap: DAMAGE_RECT_CAP,
            },
            promote: Promote::Coverage(0.5),
        };
        let results = StageResults {
            stage: Stage::SCENE,
            production: 0,
            costs: vec![ScenarioCosts::new(
                "seq/a".to_owned(),
                3,
                vec![cost([10, 30], 20, 5), cost([12, 14], 13, 4)],
                vec![0, 1],
            )],
            arms: vec![vec![
                arm(
                    "A/full",
                    vec![Decision::partial(vec![rect]), Decision::Full],
                ),
                arm("full*2", vec![Decision::Full; 2]),
            ]],
            scores: vec![
                PolicyScore {
                    worst_regret: us(7),
                    worst_regret_pct: 53.846,
                    worst_scenario: Some(0),
                    mean_regret: us(7),
                    mean_regret_pct: 53.846,
                    mean_longer_pct: 53.846,
                },
                PolicyScore {
                    worst_regret: Duration::ZERO,
                    worst_regret_pct: 0.0,
                    worst_scenario: None,
                    mean_regret: Duration::ZERO,
                    mean_regret_pct: 0.0,
                    mean_longer_pct: 0.0,
                },
            ],
            rows_per_mpx: 800.0,
        };
        let mut csv = Csv::default();
        csv.add(&[Policy::PRODUCTION, other], &results);
        assert_eq!(
            csv.arms,
            "scene,seq/a,3,A/full,true,false,0,10.00,20.00,5.00,15.00,7.00,0.00,1,100\n\
             scene,seq/a,3,A/full,true,false,1,30.00,20.00,5.00,15.00,7.00,0.00,0,0\n\
             scene,seq/a,3,full*2,false,true,0,12.00,13.00,4.00,9.00,0.00,0.00,0,0\n\
             scene,seq/a,3,full*2,false,true,1,14.00,13.00,4.00,9.00,0.00,0.00,0,0\n"
        );
        assert_eq!(
            csv.policies,
            "scene,5000,8,d400p300,7.00,53.85,seq/a,7.00,53.846,53.846\n\
             scene,inf,8,0.5,0.00,0.00,,0.00,0.000,0.000\n"
        );
    }
}
