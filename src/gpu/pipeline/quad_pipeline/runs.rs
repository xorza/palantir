//! A draw's instance span cut where the needed pipeline changes.

use crate::common::span::Span;

/// Instances that draw through one pipeline, the one `key` picks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Run<K> {
    pub(crate) key: K,
    pub(crate) instances: Span,
}

/// Which pipeline each instance of a frame draws through.
#[derive(Clone, Copy, Debug)]
pub(crate) enum RunKeys<'a, K> {
    /// One key per instance of the frame.
    PerInstance(&'a [K]),
    /// The same key for every instance.
    Uniform(K),
}

/// A span's [`Run`]s in paint order, keyed from `keys`.
#[derive(Debug)]
pub(crate) struct Runs<'a, K> {
    keys: RunKeys<'a, K>,
    next: u32,
    end: u32,
}

impl<'a, K: Copy> Runs<'a, K> {
    pub(crate) const fn new(instances: Span, keys: RunKeys<'a, K>) -> Self {
        Self {
            keys,
            next: instances.start,
            end: instances.start + instances.len,
        }
    }

    /// The keys cover every instance of the frame; an instance past them is another frame's span, a panic.
    const fn key(&self, at: u32) -> K {
        match self.keys {
            RunKeys::PerInstance(keys) => keys[at as usize],
            RunKeys::Uniform(key) => key,
        }
    }
}

impl<K: Copy + Eq> Iterator for Runs<'_, K> {
    type Item = Run<K>;

    fn next(&mut self) -> Option<Run<K>> {
        if self.next >= self.end {
            return None;
        }
        let start = self.next;
        let key = self.key(start);
        while self.next < self.end && self.key(self.next) == key {
            self.next += 1;
        }
        Some(Run {
            key,
            instances: Span::new(start, self.next - start),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::common::span::Span;
    use crate::gpu::pipeline::quad_pipeline::runs::{Run, RunKeys, Runs};

    #[test]
    fn runs_split_a_span_where_the_key_changes() {
        let keys = [1, 1, 2, 1, 1, 1, 2];
        let runs = |keys: RunKeys<'_, u8>, start: u32, len: u32| -> Vec<Run<u8>> {
            Runs::new(Span::new(start, len), keys).collect()
        };
        let run = |key, start, len| Run {
            key,
            instances: Span::new(start, len),
        };
        let per_instance = RunKeys::PerInstance(&keys);
        assert_eq!(
            runs(per_instance, 1, 5),
            [run(1, 1, 1), run(2, 2, 1), run(1, 3, 3)],
        );
        assert_eq!(runs(per_instance, 3, 2), [run(1, 3, 2)]);
        assert!(runs(per_instance, 2, 0).is_empty());
        assert_eq!(runs(RunKeys::Uniform(7), 4, 3), [run(7, 4, 3)]);
    }

    #[test]
    #[should_panic(expected = "index out of bounds")]
    fn a_span_past_the_keys_panics() {
        let keys = [1u8; 2];
        let _ = Runs::new(Span::new(1, 2), RunKeys::PerInstance(&keys)).count();
    }
}
