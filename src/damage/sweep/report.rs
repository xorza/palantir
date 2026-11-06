//! What a sweep prints and writes.

use crate::damage::sweep::arms::visible;
use crate::damage::sweep::error::SweepError;
use crate::damage::sweep::policy::{Decision, Policy, union_area};
use crate::damage::sweep::scenario::{Ranqd1, Scenario};
use crate::damage::sweep::score::{PolicyScore, StageResults};
use crate::damage::sweep::stage::Stage;
use crate::primitives::geometry::rect::Rect;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

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

/// A time in whole µs, with one digit under 100 µs, where the digit still says something.
fn us(value: f64) -> String {
    if value < 100.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.0}")
    }
}

/// A regret: `tie` when it lies within the noise, which a `0` would read as measured to be none.
fn regret_text(regret_us: f64) -> String {
    if regret_us > 0.0 {
        us(regret_us)
    } else {
        "tie".into()
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
pub(super) fn order_outcomes(raw: &[Rect], policy: &Policy, surface: Rect) -> usize {
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
        let decision = policy.decide(rects, surface);
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
    scenarios: &[Scenario],
) -> io::Result<()> {
    let surface = stage.surface();
    let production = Policy::PRODUCTION;
    let mut rows: Vec<Vec<String>> = Vec::new();
    for scenario in scenarios {
        let frames = stage.raw_frames(scenario);
        for (position, raw) in frames.iter().enumerate() {
            // A run of one frame reads once, under the position it starts at.
            if position > 0 && frames[position - 1] == *raw {
                continue;
            }
            let decision = production.decide(raw, surface);
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
                order_outcomes(raw, &production, surface).to_string(),
            ]);
        }
    }
    writeln!(
        out,
        "\nwhat production decides on {}: budget {} px², cap {}, promote above {:.2}",
        stage.name,
        budget_text(production.merge.budget_px),
        production.merge.rect_cap,
        production.promote_coverage
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
            let best = costs.best();
            let whole = arms
                .iter()
                .position(|arm| arm.frames.iter().all(|frame| *frame == Decision::Full))
                .expect("a scenario has a whole repaint");
            let regret = costs.regret_us(picked);
            let pct = regret / costs.costs[best].mean_us * 100.0;
            vec![
                costs.name.clone(),
                arms.len().to_string(),
                arms[picked].label.clone(),
                us(costs.costs[picked].mean_us),
                arms[best].label.clone(),
                us(costs.costs[best].mean_us),
                us(costs.costs[whole].mean_us),
                regret_text(regret),
                if regret > 0.0 {
                    format!("{pct:.1}")
                } else {
                    String::new()
                },
                us(costs.costs[picked].spread_us + costs.costs[best].spread_us),
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
            ("best", Align::Left),
            ("µs", Align::Right),
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
        format!("{:.1}", policy.promote_coverage),
        regret_text(score.worst_regret_us),
        score
            .worst_scenario
            .map_or_else(String::new, |idx| results.costs[idx].name.clone()),
        us(score.mean_regret_us),
        if *policy == Policy::PRODUCTION {
            "production".into()
        } else {
            String::new()
        },
    ]
}

const POLICY_HEADS: [(&str, Align); 7] = [
    ("budget px²", Align::Right),
    ("cap", Align::Right),
    ("promote", Align::Right),
    ("worst µs", Align::Right),
    ("at", Align::Left),
    ("mean µs", Align::Right),
    ("", Align::Left),
];

/// The policies, the lowest worst regret first and the lowest mean among equals: a frame is
/// judged by its worst case, so a mean only breaks a tie.
fn ranked(scores: &[PolicyScore]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..scores.len()).collect();
    order.sort_by(|&a, &b| {
        scores[a]
            .worst_regret_us
            .total_cmp(&scores[b].worst_regret_us)
            .then(
                scores[a]
                    .mean_regret_us
                    .total_cmp(&scores[b].mean_regret_us),
            )
    });
    order
}

/// How many policies score strictly better than the one at `idx`: its rank alone would count the
/// ties an unmoved score leaves in front of it.
pub(super) fn beating(scores: &[PolicyScore], idx: usize) -> usize {
    let key = |score: &PolicyScore| (score.worst_regret_us, score.mean_regret_us);
    let (worst, mean) = key(&scores[idx]);
    scores
        .iter()
        .map(key)
        .filter(|&(other_worst, other_mean)| {
            other_worst < worst || (other_worst == worst && other_mean < mean)
        })
        .count()
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
    sweep(out, "budget alone", &|p| {
        p.merge.rect_cap == production.merge.rect_cap
            && p.promote_coverage == production.promote_coverage
    })?;
    sweep(out, "cap alone", &|p| {
        p.merge.budget_px == production.merge.budget_px
            && p.promote_coverage == production.promote_coverage
    })?;
    sweep(out, "promote alone", &|p| p.merge == production.merge)
}

/// One stage's CSV rows: an arm per frame position, and a policy per line.
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
            let best = costs.best();
            for (idx, (arm, cost)) in arms.iter().zip(&costs.costs).enumerate() {
                for (position, position_us) in cost.position_us.iter().enumerate() {
                    writeln!(
                        self.arms,
                        "{},{},{},{},{},{},{},{position_us:.2},{:.2},{:.2}",
                        stage.name,
                        costs.name,
                        costs.weight,
                        arm.label,
                        idx == costs.of_policy[results.production],
                        idx == best,
                        position,
                        cost.mean_us,
                        cost.spread_us,
                    )
                    .expect("a string takes every write");
                }
            }
        }
        for (policy, score) in policies.iter().zip(&results.scores) {
            writeln!(
                self.policies,
                "{},{},{},{},{:.2},{},{:.2}",
                stage.name,
                budget_text(policy.merge.budget_px),
                policy.merge.rect_cap,
                policy.promote_coverage,
                score.worst_regret_us,
                score
                    .worst_scenario
                    .map_or("", |idx| results.costs[idx].name.as_str()),
                score.mean_regret_us,
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
            "stage,scenario,weight,arm,production,best,position,position_us,mean_us,spread_us",
            &self.arms,
        )?;
        write(
            "policies.csv",
            "stage,budget_px,rect_cap,promote,worst_regret_us,worst_scenario,mean_regret_us",
            &self.policies,
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::damage::region::{DAMAGE_RECT_CAP, MergePolicy};
    use crate::damage::sweep::arms::Arm;
    use crate::damage::sweep::error::SweepError;
    use crate::damage::sweep::policy::{Decision, Policy};
    use crate::damage::sweep::report::{
        Align, Csv, budget_text, changed_share, percent, ranked, regret_text, table, us,
    };
    use crate::damage::sweep::score::{ArmCost, PolicyScore, ScenarioCosts, StageResults};
    use crate::damage::sweep::stage::Stage;
    use crate::primitives::geometry::rect::Rect;
    use std::error::Error;
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
        assert_eq!(regret_text(0.0), "tie");
        assert_eq!(regret_text(32.74), "32.7");
        assert_eq!(us(99.94), "99.9");
        assert_eq!(us(100.4), "100");
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
        let of = |worst_regret_us, mean_regret_us| PolicyScore {
            worst_regret_us,
            worst_scenario: None,
            mean_regret_us,
        };
        assert_eq!(
            ranked(&[of(5.0, 1.0), of(4.0, 9.0), of(5.0, 0.5), of(6.0, 0.0)]),
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
        let cost = |position_us: Vec<f64>, mean_us, spread_us| ArmCost {
            position_us,
            mean_us,
            spread_us,
        };
        let arm = |label: &str| Arm {
            label: label.to_owned(),
            frames: vec![Decision::Full; 2],
        };
        let other = Policy {
            merge: MergePolicy {
                budget_px: f32::INFINITY,
                rect_cap: DAMAGE_RECT_CAP,
            },
            promote_coverage: 0.5,
        };
        let results = StageResults {
            stage: Stage::SCENE,
            production: 0,
            costs: vec![ScenarioCosts {
                name: "seq/a".to_owned(),
                weight: 3,
                costs: vec![
                    cost(vec![10.0, 30.0], 20.0, 1.0),
                    cost(vec![12.0, 14.0], 13.0, 0.5),
                ],
                of_policy: vec![0, 1],
            }],
            arms: vec![vec![arm("A/full"), arm("full*2")]],
            scores: vec![
                PolicyScore {
                    worst_regret_us: 7.0,
                    worst_scenario: Some(0),
                    mean_regret_us: 7.0,
                },
                PolicyScore {
                    worst_regret_us: 0.0,
                    worst_scenario: None,
                    mean_regret_us: 0.0,
                },
            ],
        };
        let mut csv = Csv::default();
        csv.add(&[Policy::PRODUCTION, other], &results);
        assert_eq!(
            csv.arms,
            "scene,seq/a,3,A/full,true,false,0,10.00,20.00,1.00\n\
             scene,seq/a,3,A/full,true,false,1,30.00,20.00,1.00\n\
             scene,seq/a,3,full*2,false,true,0,12.00,13.00,0.50\n\
             scene,seq/a,3,full*2,false,true,1,14.00,13.00,0.50\n"
        );
        assert_eq!(
            csv.policies,
            "scene,20000,8,0.4,7.00,seq/a,7.00\nscene,inf,8,0.5,0.00,,0.00\n"
        );
    }
}
