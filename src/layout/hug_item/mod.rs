//! The one `Sizing::HUG` sharer: when content-sized siblings want more
//! than their container has, each gives way from what it wants toward
//! what it cannot go below, in proportion to how far it can give.
//!
//! Both drivers land here — the stack's non-Fill children and the grid's
//! Phase-2 Hug tracks — so a Hug child gives way the same inside a
//! `Panel` and inside a `Grid`. A rigid item is one whose floor is its
//! extent: it keeps it, and overflows with the rest when even the floors
//! do not fit.

use crate::layout::measured::Measured;

/// One participant in a Hug share.
///
/// `key` is the caller's own handle — a child `NodeId` for the stack, a
/// track index for the grid — carried through untouched so a share comes
/// back attached to whatever asked for it.
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

    /// Share `budget` across `items`, writing each one's extent into its
    /// own [`Self::size`]:
    /// - every item at what it wants when that all fits, or the budget is
    ///   unbounded;
    /// - every item at its floor when even the floors do not fit;
    /// - otherwise each at its floor plus the slack, shared in proportion
    ///   to how far it can give, `hi - lo`.
    ///
    /// Returns the least finite budget from which the shares hold — see
    /// [`Measured::stable_from`]: what the items want, summed, while it
    /// fits, and [`Measured::AT_OFFER_ONLY`] once one gives way.
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
