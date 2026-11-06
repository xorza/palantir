//! A sweep's command line.

use crate::damage::sweep::error::SweepError;
use std::path::PathBuf;

/// What a run measures and where it writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SweepArgs {
    /// Only cases whose `<stage>/<scenario>` holds this.
    pub(super) filter: Option<String>,
    /// How many times each arm takes its turn. Under `cargo test`, which passes no `--bench`, one
    /// round of one frame, as criterion runs each case once there.
    pub(super) rounds: u32,
    /// The frames each arm plays in a turn, whole cycles at least.
    pub(super) frames: u32,
    /// List the cases and their arms, and measure nothing.
    pub(super) list: bool,
    /// The folder a run writes `arms.csv` and `policies.csv` into.
    pub(super) csv: Option<PathBuf>,
}

/// Rounds of a real run. Half of them run the arms backward, so a drift in time charges each arm
/// alike; twelve give the spread a quartile of three rounds.
const ROUNDS: u32 = 12;
/// Frames of a turn in a real run.
const FRAMES: u32 = 8;

pub(super) const USAGE: &str = "cargo damage-sweep -- [FILTER] [--rounds N] [--frames N] [--list] [--csv DIR]\n\
     PALANTIR_BENCH_NO_GPU=1 prints each decision and times nothing.";

impl SweepArgs {
    pub(super) fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, SweepError> {
        let mut args = args.into_iter();
        let mut timed = false;
        let mut rounds = None;
        let mut frames = None;
        let mut out = Self {
            filter: None,
            rounds: 1,
            frames: 1,
            list: false,
            csv: None,
        };
        let count = |flag: &'static str, value: Option<String>| {
            let value = value.ok_or(SweepError::MissingValue(flag))?;
            match value.parse() {
                Ok(count) if count > 0 => Ok(count),
                _ => Err(SweepError::NotACount { flag, value }),
            }
        };
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--bench" => timed = true,
                "--list" => out.list = true,
                "--rounds" => rounds = Some(count("--rounds", args.next())?),
                "--frames" => frames = Some(count("--frames", args.next())?),
                "--csv" => {
                    let dir = args.next().ok_or(SweepError::MissingValue("--csv"))?;
                    out.csv = Some(PathBuf::from(dir));
                }
                flag if flag.starts_with("--") => return Err(SweepError::UnknownFlag(arg)),
                _ if out.filter.is_some() => return Err(SweepError::SecondFilter(arg)),
                _ => out.filter = Some(arg),
            }
        }
        if timed {
            out.rounds = ROUNDS;
            out.frames = FRAMES;
        }
        out.rounds = rounds.unwrap_or(out.rounds);
        out.frames = frames.unwrap_or(out.frames);
        Ok(out)
    }
}
