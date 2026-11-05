//! [`PaneKeys`]: the panes a frame has, as one sorted set every stage reads.

use crate::primitives::identity::pane_key::PaneKey;
use std::slice;

/// A frame's pane keys, sorted and each once, so a lookup is a binary search. The cascade lists
/// them once a frame; the damage walk and the render buffer copy that list and keep it past the
/// frame. Refused: a list in paint order per stage, each derived again and searched linearly,
/// which made a frame's pane bookkeeping quadratic in its panes.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct PaneKeys {
    keys: Vec<PaneKey>,
}

impl PaneKeys {
    /// Hold `keys` alone, each of which names one pane of the frame.
    pub(crate) fn refill(&mut self, keys: impl IntoIterator<Item = PaneKey>) {
        self.keys.clear();
        self.keys.extend(keys);
        self.keys.sort_unstable();
        debug_assert!(
            self.keys.windows(2).all(|pair| pair[0] != pair[1]),
            "a pane key names one pane",
        );
    }

    pub(crate) fn contains(&self, key: PaneKey) -> bool {
        self.keys.binary_search(&key).is_ok()
    }

    pub(crate) const fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub(crate) fn iter(&self) -> slice::Iter<'_, PaneKey> {
        self.keys.iter()
    }
}

/// Copies through `Vec::clone_from`, which keeps the buffer, as a frame copies a set each frame.
impl Clone for PaneKeys {
    fn clone(&self) -> Self {
        Self {
            keys: self.keys.clone(),
        }
    }

    fn clone_from(&mut self, source: &Self) {
        self.keys.clone_from(&source.keys);
    }
}

impl<'a> IntoIterator for &'a PaneKeys {
    type Item = &'a PaneKey;
    type IntoIter = slice::Iter<'a, PaneKey>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    #[cfg(test)]
    use crate::internals::scratch_report;
    use crate::primitives::identity::pane_keys::PaneKeys;

    impl PaneKeys {
        #[cfg(test)]
        pub(crate) fn scratch_bytes(&self) -> usize {
            scratch_report::vec_bytes(&self.keys)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::identity::pane_key::PaneKey;
    use crate::primitives::identity::pane_keys::PaneKeys;
    use crate::primitives::identity::widget_id::WidgetId;
    use std::num::NonZeroU64;

    fn key(owner: u64, shape: u32) -> PaneKey {
        PaneKey {
            owner: WidgetId(NonZeroU64::new(owner).unwrap()),
            shape,
        }
    }

    /// The set sorts by owner, then row, whatever order it is filled in, and finds exactly what
    /// it holds: the owner alone or the row alone does not match. A refill drops the last keys.
    #[test]
    fn a_set_sorts_finds_its_keys_and_refills() {
        let mut keys = PaneKeys::default();
        keys.refill([key(2, 0), key(1, 3), key(1, 0)]);
        assert_eq!(
            keys.iter().copied().collect::<Vec<_>>(),
            [key(1, 0), key(1, 3), key(2, 0)]
        );
        assert!(keys.contains(key(1, 3)));
        assert!(!keys.contains(key(1, 1)), "the owner alone");
        assert!(!keys.contains(key(3, 0)), "the row alone");
        keys.refill([key(3, 0)]);
        assert!(!keys.contains(key(1, 0)));
        assert!(keys.contains(key(3, 0)));
        keys.refill([]);
        assert!(keys.is_empty());
    }
}
