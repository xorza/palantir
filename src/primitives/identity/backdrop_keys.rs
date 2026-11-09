//! [`BackdropKeys`]: the backdrops a frame has, as one sorted set every stage reads.

use crate::primitives::identity::backdrop_key::BackdropKey;
use std::slice;

/// A frame's backdrop keys, sorted and each once, so a lookup is a binary search. The cascade lists
/// them once a frame; the damage walk and the render buffer copy that list and keep it past the
/// frame. Refused: a list in paint order per stage, each derived again and searched linearly,
/// which made a frame's backdrop bookkeeping quadratic in its backdrops.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct BackdropKeys {
    keys: Vec<BackdropKey>,
}

impl BackdropKeys {
    /// Hold `keys` alone, each of which names one backdrop of the frame.
    pub(crate) fn refill(&mut self, keys: impl IntoIterator<Item = BackdropKey>) {
        self.keys.clear();
        self.keys.extend(keys);
        self.keys.sort_unstable();
        debug_assert!(
            self.keys.windows(2).all(|pair| pair[0] != pair[1]),
            "a backdrop key names one backdrop",
        );
    }

    pub(crate) fn contains(&self, key: BackdropKey) -> bool {
        self.keys.binary_search(&key).is_ok()
    }

    pub(crate) const fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub(crate) fn iter(&self) -> slice::Iter<'_, BackdropKey> {
        self.keys.iter()
    }
}

/// Copies through `Vec::clone_from`, which keeps the buffer, as a frame copies a set each frame.
impl Clone for BackdropKeys {
    fn clone(&self) -> Self {
        Self {
            keys: self.keys.clone(),
        }
    }

    fn clone_from(&mut self, source: &Self) {
        self.keys.clone_from(&source.keys);
    }
}

impl<'a> IntoIterator for &'a BackdropKeys {
    type Item = &'a BackdropKey;
    type IntoIter = slice::Iter<'a, BackdropKey>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    #[cfg(test)]
    use crate::internals::scratch_report;
    use crate::primitives::identity::backdrop_keys::BackdropKeys;

    impl BackdropKeys {
        #[cfg(test)]
        pub(crate) fn scratch_bytes(&self) -> usize {
            scratch_report::vec_bytes(&self.keys)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::identity::backdrop_key::BackdropKey;
    use crate::primitives::identity::backdrop_keys::BackdropKeys;
    use crate::primitives::identity::widget_id::WidgetId;

    fn key(owner: u64, shape: u32) -> BackdropKey {
        BackdropKey {
            owner: WidgetId::of_bits(owner),
            shape,
        }
    }

    /// The set sorts by owner, then row, whatever order it is filled in, and finds exactly what
    /// it holds: the owner alone or the row alone does not match. A refill drops the last keys.
    #[test]
    fn a_set_sorts_finds_its_keys_and_refills() {
        let mut keys = BackdropKeys::default();
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
