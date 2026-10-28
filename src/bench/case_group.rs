//! A criterion group that holds its cases to one id depth.

use criterion::measurement::WallTime;
use criterion::{Bencher, BenchmarkGroup, Throughput};
use std::fmt;

/// A group of cases whose ids, `<backend>/<rest…>` past the group's own name, all have the depth
/// of the first: a filter such as `'^atomic/composer/cpu/[^/]+/256$'` then means the same thing
/// for every case. A case of another depth panics where it registers, so `cargo drive bench
/// --list` finds it.
pub struct CaseGroup<'a> {
    group: BenchmarkGroup<'a, WallTime>,
    name: String,
    depth: Option<usize>,
}

impl fmt::Debug for CaseGroup<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CaseGroup")
            .field("name", &self.name)
            .field("depth", &self.depth)
            .finish_non_exhaustive()
    }
}

impl<'a> CaseGroup<'a> {
    pub(crate) const fn new(group: BenchmarkGroup<'a, WallTime>, name: String) -> Self {
        Self {
            group,
            name,
            depth: None,
        }
    }

    /// Register case `id` of this group, timed by `f`.
    ///
    /// # Panics
    ///
    /// Panics if `id` has another depth than the group's first case.
    #[track_caller]
    pub fn bench_function(
        &mut self,
        id: String,
        f: impl FnMut(&mut Bencher<'_, WallTime>),
    ) -> &mut Self {
        let depth = id.split('/').count();
        let first = *self.depth.get_or_insert(depth);
        assert_eq!(
            depth, first,
            "{}/{id}: every case of a group has the depth of its first",
            self.name
        );
        self.group.bench_function(id, f);
        self
    }

    /// The throughput the cases registered from here on report.
    pub fn throughput(&mut self, throughput: Throughput) -> &mut Self {
        self.group.throughput(throughput);
        self
    }

    /// End the group, as criterion's does.
    pub fn finish(self) {
        self.group.finish();
    }
}
