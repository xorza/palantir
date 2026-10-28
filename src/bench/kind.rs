//! The policy every case follows: its kind and how criterion samples it.

use crate::bench::case_group::CaseGroup;
use criterion::measurement::WallTime;
use criterion::{BenchmarkGroup, Criterion, SamplingMode};
use std::time::Duration;

/// What a case times, the first part of its id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// One stage, one widget or one operation on a made input.
    Atomic,
    /// Palantir's frame, or a chain of its stages from record onward, over a scene.
    Integration,
}

impl Kind {
    /// The group `name` of this kind, sampled as `scale` says. Each name is made once, as the
    /// group holds its cases to one depth only among the cases it registers.
    pub fn group<'a>(self, c: &'a mut Criterion, name: &str, scale: Scale) -> CaseGroup<'a> {
        let kind = match self {
            Self::Atomic => "atomic",
            Self::Integration => "integration",
        };
        let name = format!("{kind}/{name}");
        let mut group = c.benchmark_group(&name);
        scale.apply(&mut group);
        CaseGroup::new(group, name)
    }
}

/// How long one iteration of a case takes, which sets how criterion samples it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scale {
    /// Microseconds an iteration.
    Sample,
    /// Up to 300 ms an iteration (every case here, as measured), so fewer and longer samples.
    Frame,
}

impl Scale {
    /// Sets the sampling of the cases `group` registers from here on.
    #[expect(
        clippy::disallowed_methods,
        reason = "`Scale::apply` is the one place a group's sampling is set"
    )]
    pub(crate) fn apply(self, group: &mut BenchmarkGroup<'_, WallTime>) {
        match self {
            Self::Sample => group
                .sample_size(50)
                .sampling_mode(SamplingMode::Auto)
                .warm_up_time(Duration::from_millis(500))
                .measurement_time(Duration::from_secs(1)),
            Self::Frame => group
                .sample_size(10)
                .sampling_mode(SamplingMode::Auto)
                .warm_up_time(Duration::from_secs(1))
                .measurement_time(Duration::from_secs(3)),
        };
    }
}
