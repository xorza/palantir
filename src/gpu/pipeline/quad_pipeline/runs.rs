//! A draw's span of instances cut where the pipeline its instances need
//! changes.

use crate::common::span::Span;

/// Instances that draw through one pipeline, the one `key` picks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Run<K> {
    pub(crate) key: K,
    pub(crate) instances: Span,
}

/// A span's [`Run`]s, in paint order: each instance's key read from
/// `keys`, or `fallback` for every instance when there are none.
#[derive(Debug)]
pub(crate) struct Runs<'a, K> {
    keys: Option<&'a [K]>,
    fallback: K,
    next: u32,
    end: u32,
}

impl<'a, K: Copy> Runs<'a, K> {
    /// The runs of `instances`, keyed by `keys`, which hold a key for
    /// every uploaded instance, or by `fallback` alone.
    pub(crate) const fn new(instances: Span, keys: Option<&'a [K]>, fallback: K) -> Self {
        Self {
            keys,
            fallback,
            next: instances.start,
            end: instances.start + instances.len,
        }
    }

    /// The keys cover every instance of the frame, so an instance past them
    /// is a span from another frame: a panic, not a guess.
    fn key(&self, at: u32) -> K {
        self.keys.map_or(self.fallback, |keys| keys[at as usize])
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
    use crate::gpu::pipeline::quad_pipeline::runs::{Run, Runs};

    /// A span splits where the key changes, in paint order, and starts and
    /// ends where the span does, not where the keys do. Without keys, the
    /// span is one run of the fallback.
    #[test]
    fn runs_split_a_span_where_the_key_changes() {
        let keys = [1, 1, 2, 1, 1, 1, 2];
        let runs = |keys: Option<&[u8]>, start: u32, len: u32| -> Vec<Run<u8>> {
            Runs::new(Span::new(start, len), keys, 0).collect()
        };
        let run = |key, start, len| Run {
            key,
            instances: Span::new(start, len),
        };
        assert_eq!(
            runs(Some(&keys), 1, 5),
            [run(1, 1, 1), run(2, 2, 1), run(1, 3, 3)],
        );
        assert_eq!(runs(Some(&keys), 3, 2), [run(1, 3, 2)]);
        assert!(runs(Some(&keys), 2, 0).is_empty());
        assert_eq!(runs(None, 4, 3), [run(0, 4, 3)]);
    }

    /// A span past the keys comes from another frame's instances.
    #[test]
    #[should_panic(expected = "index out of bounds")]
    fn a_span_past_the_keys_panics() {
        let keys = [1u8; 2];
        let _ = Runs::new(Span::new(1, 2), Some(&keys), 0).count();
    }
}
