//! [`TabKey`]: a tab's stable identity within its strip.

use crate::common::content_hash::ContentHash;
use std::hash::Hash;

/// A tab's stable identity within one strip, from a value the caller names the tab by, so hover,
/// animation and drag follow the tab when tabs close or reorder. Every id the strip derives for a
/// chip comes from it. Refused: a raw `u64`, which invited hand-picked numbers that collide; a bare
/// [`ContentHash`], which the compiler would take for an animation key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TabKey(ContentHash);

impl TabKey {
    /// The key of `value`, by its hash: equal values give equal keys.
    pub fn of(value: impl Hash) -> Self {
        Self(ContentHash::of(value))
    }
}
