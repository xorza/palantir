//! The damage sweep: what each way of repainting a frame's damage costs on this GPU, and how far
//! each setting of the merge and repaint knobs lands from the cheapest. `cargo damage-sweep`.
//!
//! - **A policy is scored, not rerun.** A scenario's arms are the decisions a grid of policies
//!   ends in, plus the whole repaint and each partition of a few rects; each is timed once, on a
//!   host told to paint it. Every policy of the grid then looks its arm up, so one run scores all
//!   of them, and the best arm can be one no policy reaches. Refused: rebuilding per knob value, a
//!   fat link and a run each.
//! - **Arms take turns.** A cost is read against the other arms of its scenario, so a drift
//!   between two arms reads as a difference between them: every second round runs them backward,
//!   the ABBA order `palantir-showcase/benches/AGENTS.md` asks of two builds. Refused: a criterion
//!   case per arm, one run after another; WebRender keeps a command of its own too,
//!   [`wrench bench`](https://searchfox.org/firefox-main/source/gfx/wr/wrench/src/bench.rs).
//! - **Regret is the score.** It is an arm's cost past the scenario's best; a policy reads as its
//!   worst regret first, as a frame is judged by its worst case, then as its mean per frame.
//! - **Scenarios weigh what happens.** A made scenario counts as one frame, and a scripted session
//!   over the screen adds its frequent frames by count, so a mean reflects use and the catalog
//!   keeps the rare frames a mean would hide.
//! - **Measured on a still screen, through the real frame.** The host keeps its own record, build
//!   and submit, and the decision replaces only the frame's plan
//!   (`WindowDriver::force_next_frame`), so an arm's own cost is all that differs. Refused: widgets
//!   flipping under the rects, whose fills would paint in the merged arms as they do not in a
//!   real frame.

mod args;
mod arms;
mod capture;
mod error;
pub(crate) mod player;
pub(crate) mod policy;
mod report;
pub(crate) mod scenario;
mod score;
pub(crate) mod stage;
#[cfg(test)]
mod tests;

use crate::bench::backends::Backends;
use crate::bench::scene::Scene;
use crate::damage::sweep::args::{SweepArgs, USAGE};
use crate::damage::sweep::arms::Arms;
use crate::damage::sweep::capture::capture;
use crate::damage::sweep::error::SweepError;
use crate::damage::sweep::player::{PlayedFrame, Player, Turns};
use crate::damage::sweep::policy::Policy;
use crate::damage::sweep::report::Csv;
use crate::damage::sweep::scenario::{Scenario, catalog, pair_grid};
use crate::damage::sweep::score::{ScenarioCosts, StageResults, score};
use crate::damage::sweep::stage::Stage;
use crate::gpu::bench_gpu::BenchGpu;
use crate::ui::bench::TIMING;
use std::env;
use std::error::Error;
use std::io::{self, Write};
use std::process::ExitCode;
use std::time::Instant;

/// `stage`'s scenarios among `all` that `filter` selects.
fn selected<'a>(stage: Stage, all: &'a [Scenario], filter: Option<&str>) -> Vec<&'a Scenario> {
    all.iter()
        .filter(|scenario| stage.runs(scenario.scope))
        .filter(|scenario| {
            filter.is_none_or(|filter| format!("{}/{}", stage.name, scenario.name).contains(filter))
        })
        .collect()
}

/// Print each case and how many arms it has.
fn list(
    out: &mut impl Write,
    filter: Option<&str>,
    scenarios: &[Scenario],
    policies: &[Policy],
) -> io::Result<()> {
    let mut total = 0;
    for stage in Stage::ALL {
        for scenario in selected(stage, scenarios, filter) {
            let arms = Arms::of(&stage.raw_frames(scenario), stage.surface(), policies);
            total += arms.arms.len();
            writeln!(
                out,
                "{}/{}: {} arms",
                stage.name,
                scenario.name,
                arms.arms.len()
            )?;
        }
    }
    writeln!(out, "{total} arms")
}

/// What every stage of a timed run shares.
#[derive(Debug)]
struct Measure<'a> {
    gpu: &'a BenchGpu,
    policies: &'a [Policy],
    turns: Turns,
}

impl Measure<'_> {
    /// Time `scenarios` on `stage`.
    fn stage<S: Scene>(&self, stage: Stage, scenarios: &[&Scenario]) -> StageResults {
        let surface = stage.surface();
        let mut player = Player::<S>::new(self.gpu, stage);
        let mut costs = Vec::with_capacity(scenarios.len());
        let mut all_arms = Vec::with_capacity(scenarios.len());
        for scenario in scenarios {
            let raw = stage.raw_frames(scenario);
            let Arms { arms, of_policy } = Arms::of(&raw, surface, self.policies);
            let played: Vec<Vec<PlayedFrame>> =
                arms.iter().map(|arm| PlayedFrame::of(arm, &raw)).collect();
            costs.push(ScenarioCosts {
                name: scenario.name.clone(),
                weight: scenario.weight,
                costs: player.sample(&played, self.turns),
                of_policy,
            });
            all_arms.push(arms);
        }
        StageResults {
            stage,
            production: Policy::production_idx(self.policies),
            scores: score(&costs, self.policies.len()),
            costs,
            arms: all_arms,
        }
    }
}

fn run_args<S: Scene>(args: &SweepArgs, out: &mut impl Write) -> Result<(), SweepError> {
    let captured = capture::<S>();
    writeln!(
        out,
        "session: {} painted frames, {} distinct; {} kept, for {} frames",
        captured.frame_count,
        captured.distinct_count,
        captured.scenarios.len(),
        captured.kept_frame_count
    )?;
    let filter = args.filter.as_deref();
    let mut scenarios = catalog();
    scenarios.extend(captured.scenarios);
    let policies = Policy::grid();
    if args.list {
        list(out, filter, &scenarios, &policies)?;
        return Ok(());
    }
    let backends = Backends::probe();
    if backends.gpu().is_none() {
        scenarios.extend(pair_grid());
        let chosen: Vec<Scenario> = selected(Stage::SCENE, &scenarios, filter)
            .into_iter()
            .cloned()
            .collect();
        report::decisions(out, Stage::SCENE, &chosen)?;
        return Ok(());
    }
    let measure = Measure {
        gpu: backends.device(TIMING),
        policies: &policies,
        turns: Turns {
            rounds: args.rounds,
            frames: args.frames,
        },
    };
    let mut csv = Csv::default();
    // Before the first stage, so a folder it cannot write fails before a long run, and after each,
    // so a run stopped late keeps what it measured.
    let write_csv = |csv: &Csv| args.csv.as_deref().map_or(Ok(()), |dir| csv.write(dir));
    write_csv(&csv)?;
    for stage in Stage::ALL {
        let chosen = selected(stage, &scenarios, filter);
        if chosen.is_empty() {
            continue;
        }
        let started = Instant::now();
        let results = measure.stage::<S>(stage, &chosen);
        writeln!(
            out,
            "\n== {} ({} × {} at {}×, in {:.0} s) ==",
            stage.name,
            stage.physical.x,
            stage.physical.y,
            stage.scale.get(),
            started.elapsed().as_secs_f64()
        )?;
        report::arms_table(out, &results)?;
        report::policies_table(out, &policies, &results)?;
        csv.add(&policies, &results);
        write_csv(&csv)?;
    }
    Ok(())
}

/// Print `error` and each cause to stderr.
#[expect(
    clippy::print_stderr,
    reason = "a bench that cannot run says why where a person reads it"
)]
fn report_error(error: &dyn Error) {
    eprintln!("{error}");
    let mut source = error.source();
    while let Some(cause) = source {
        eprintln!("caused by: {cause}");
        source = cause.source();
    }
}

/// A sweep over `S` as its command line says: `cargo damage-sweep`.
#[expect(
    clippy::print_stderr,
    reason = "a command line that does not parse answers with its usage"
)]
pub(crate) fn run<S: Scene>() -> ExitCode {
    let args = match SweepArgs::parse(env::args().skip(1)) {
        Ok(args) => args,
        Err(error) => {
            report_error(&error);
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    let mut out = io::stdout().lock();
    match run_args::<S>(&args, &mut out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            report_error(&error);
            ExitCode::FAILURE
        }
    }
}
