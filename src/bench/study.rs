//! The studies the bench target runs by name in place of its criterion cases, one-shot reports
//! with command lines of their own that share the target and so its one release link, and how a
//! command line picks a run.

use crate::bench::scene::Scene;
use crate::damage;
use crate::gpu::atlas_memory;
use std::process::ExitCode;

/// A study, named by the first argument of the bench target's command line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Study {
    /// `cargo damage-sweep`: each way of repainting a frame's damage, timed and scored.
    DamageSweep,
    /// `cargo atlas-memory`: what each GPU atlas holds over the bench scenes.
    AtlasMemory,
}

impl Study {
    pub(super) const ALL: [Self; 2] = [Self::DamageSweep, Self::AtlasMemory];

    /// The name that runs it, which its cargo alias passes.
    const fn name(self) -> &'static str {
        match self {
            Self::DamageSweep => "damage-sweep",
            Self::AtlasMemory => "atlas-memory",
        }
    }

    /// Run the study over `S` with `args`, its command line past its name.
    pub(super) fn run<S: Scene>(self, args: Vec<String>) -> ExitCode {
        match self {
            Self::DamageSweep => damage::sweep::run::<S>(args),
            Self::AtlasMemory => atlas_memory::bench::study::<S>(&args),
        }
    }
}

/// What one run of the bench target does, from its command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Invocation {
    /// The study named, with the arguments past its name.
    Study(Study, Vec<String>),
    /// The criterion cases, which read the arguments themselves.
    Cases,
    /// No argument at all, as `cargo test` runs the target: every case once, then every study once.
    Everything,
}

impl Invocation {
    /// The run `args` asks for: a study where its first argument but `--bench`, which
    /// `cargo bench` adds, and `--`, which an alias's own `--` leaves before a user's, names one,
    /// with the rest past the name; the criterion cases for any other; everything for none.
    pub(super) fn of(args: impl IntoIterator<Item = String>) -> Self {
        let mut args: Vec<String> = args.into_iter().filter(|arg| arg != "--").collect();
        if args.is_empty() {
            return Self::Everything;
        }
        let named = args.iter().position(|arg| arg != "--bench").and_then(|at| {
            Study::ALL
                .into_iter()
                .find(|study| study.name() == args[at])
                .map(|study| (at, study))
        });
        let Some((at, study)) = named else {
            return Self::Cases;
        };
        args.remove(at);
        Self::Study(study, args)
    }
}

#[cfg(test)]
mod tests {
    use crate::bench::study::{Invocation, Study};

    fn of(args: &[&str]) -> Invocation {
        Invocation::of(args.iter().map(|&arg| arg.to_owned()))
    }

    /// A study's name, first past `--bench` and any `--`, selects it and leaves the rest:
    /// `cargo damage-sweep -- scene/pair/` arrives as `damage-sweep -- scene/pair/ --bench` and runs
    /// the sweep over `scene/pair/ --bench`; `--bench` ahead of the name selects alike. A filter, a
    /// criterion flag and `--bench` alone run the cases; no argument, as `cargo bench-once` passes,
    /// runs everything.
    #[test]
    fn the_first_argument_names_the_run() {
        let owned = |args: &[&str]| args.iter().map(|&arg| arg.to_owned()).collect();
        assert_eq!(
            of(&["damage-sweep", "--", "scene/pair/", "--bench"]),
            Invocation::Study(Study::DamageSweep, owned(&["scene/pair/", "--bench"]))
        );
        assert_eq!(
            of(&["--bench", "atlas-memory"]),
            Invocation::Study(Study::AtlasMemory, owned(&["--bench"]))
        );
        assert_eq!(
            of(&["atlas-memory"]),
            Invocation::Study(Study::AtlasMemory, Vec::new())
        );
        assert_eq!(of(&["^atomic/", "--bench"]), Invocation::Cases);
        assert_eq!(
            of(&["--save-baseline", "damage-sweep", "--bench"]),
            Invocation::Cases
        );
        assert_eq!(of(&["--bench"]), Invocation::Cases);
        assert_eq!(of(&["--"]), Invocation::Everything);
        assert_eq!(of(&[]), Invocation::Everything);
    }
}
