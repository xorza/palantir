//! A draw's instance span cut where the needed binding changes: a quad's pipeline, a shadow's
//! cutout entry, an image's texture.

use crate::common::span::Span;

/// Instances that draw through one binding, the one `key` picks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Run<K> {
    pub(crate) key: K,
    pub(crate) instances: Span,
}

/// A span's [`Run`]s in paint order, keyed per instance by `key_of`, which reads the frame's own
/// data at an instance index. Only neighbors merge, so paint order holds and nothing is sorted:
/// `A B A` stays three runs. Lazy; allocates nothing.
#[derive(Debug)]
pub(crate) struct Runs<F> {
    key_of: F,
    next: u32,
    end: u32,
}

impl<F> Runs<F> {
    pub(crate) const fn new(instances: Span, key_of: F) -> Self {
        Self {
            key_of,
            next: instances.start,
            end: instances.start + instances.len,
        }
    }
}

impl<K: Eq, F: Fn(u32) -> K> Iterator for Runs<F> {
    type Item = Run<K>;

    fn next(&mut self) -> Option<Run<K>> {
        if self.next >= self.end {
            return None;
        }
        let start = self.next;
        let key = (self.key_of)(start);
        self.next += 1;
        while self.next < self.end && (self.key_of)(self.next) == key {
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
    use crate::gpu::pipeline::runs::{Run, Runs};

    /// A span splits where its key changes, indexed into the frame's keys from the span's start;
    /// an empty span has no run; a uniform key is one run; `7 9 7` stays three.
    #[test]
    fn runs_split_a_span_where_the_key_changes() {
        let keys = [1, 1, 2, 1, 1, 1, 2];
        let per_instance = |i: u32| keys[i as usize];
        let runs = |start: u32, len: u32| -> Vec<Run<u8>> {
            Runs::new(Span::new(start, len), per_instance).collect()
        };
        let run = |key, start, len| Run {
            key,
            instances: Span::new(start, len),
        };
        assert_eq!(runs(1, 5), [run(1, 1, 1), run(2, 2, 1), run(1, 3, 3)]);
        assert_eq!(runs(3, 2), [run(1, 3, 2)]);
        assert!(runs(2, 0).is_empty());
        let uniform: Vec<_> = Runs::new(Span::new(4, 3), |_| 7u8).collect();
        assert_eq!(uniform, [run(7, 4, 3)]);
        let alternating = [7u8, 9, 7];
        let split: Vec<_> = Runs::new(Span::new(0, 3), |i| alternating[i as usize]).collect();
        assert_eq!(split, [run(7, 0, 1), run(9, 1, 1), run(7, 2, 1)]);
    }
}
