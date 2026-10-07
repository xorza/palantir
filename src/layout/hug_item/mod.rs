//! The one `Sizing::HUG` sharer: when content-sized siblings want more than the container has, each gives way from what it wants toward its floor, in proportion to how far it can give. Shared by the stack's non-Fill children and the grid's Hug tracks. A rigid item (floor = extent) keeps its size and overflows.

use crate::layout::measured::Measured;

/// One participant in a Hug share. `key` is the caller's handle (child `NodeId` or track index), carried through untouched.
#[derive(Clone, Copy, Debug)]
pub(super) struct HugItem<K> {
    pub(super) key: K,
    /// The share once [`Self::share`] returns.
    pub(super) size: f32,
    /// What the item cannot go below.
    lo: f32,
    /// What the item wants.
    hi: f32,
}

impl<K> HugItem<K> {
    pub(super) fn new(key: K, lo: f32, hi: f32) -> Self {
        debug_assert!(
            lo <= hi,
            "a Hug item's floor {lo} exceeds what it wants, {hi}"
        );
        Self {
            key,
            size: 0.0,
            lo,
            hi,
        }
    }

    /// Share `budget` across `items`, writing each extent into its [`Self::size`]: all at what they want when that fits or the budget is unbounded; all at their floor when floors don't fit; otherwise floor plus slack in proportion to `hi - lo`.
    ///
    /// Returns the least finite budget from which the shares hold; see [`Measured::stable_from`].
    pub(super) fn share(items: &mut [Self], budget: f32) -> f32 {
        let (lo_sum, hi_sum) = items.iter().fold((0.0_f32, 0.0_f32), |(lo, hi), item| {
            (lo + item.lo, hi + item.hi)
        });
        if budget.is_infinite() || hi_sum <= budget {
            for item in items.iter_mut() {
                item.size = item.hi;
            }
            return hi_sum;
        }
        let slack = budget - lo_sum;
        let range = hi_sum - lo_sum;
        for item in items.iter_mut() {
            item.size = if slack <= 0.0 {
                item.lo
            } else {
                (item.lo + slack * (item.hi - item.lo) / range).min(item.hi)
            };
        }
        Measured::AT_OFFER_ONLY
    }
}

#[cfg(test)]
mod tests;
