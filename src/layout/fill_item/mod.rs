//! The one `Sizing::fill` distributor: hand every weighted item its
//! proportional share of a budget, clamp that share to `[floor, cap]`, and
//! re-divide whatever the clamps free.
//!
//! Both drivers land here — the stack's Fill children and the grid's
//! Phase-3 Fill tracks — so `Sizing::fill` means one thing inside a
//! `Panel` and the same thing inside a `Grid`.
//!
//! Freezing follows CSS Flexbox §9.7 *resolve flexible lengths*: each pass
//! clamps every unfrozen item, sums the adjustments, and freezes only the
//! violators whose own adjustment has the same sign as that total. Both
//! shortcuts around it are wrong. Freezing *every* violator pins an item
//! at its floor before a later item's cap frees the room that would have
//! cleared it. Freezing *one* violator per pass makes the answer depend
//! on the order the items were pushed. The sign test does neither and
//! still converges in at most one pass per item.

/// One participant in a Fill distribution.
///
/// `key` is the caller's own handle — a child `NodeId` for the stack, a
/// track index for the grid — carried through untouched so an allocation
/// comes back attached to whatever asked for it.
#[derive(Clone, Copy, Debug)]
pub(super) struct FillItem<K> {
    pub(super) key: K,
    /// The allocation once [`Self::distribute`] returns.
    pub(super) size: f32,
    weight: f32,
    /// Least extent this item accepts. A floor above `cap` resolves to
    /// `cap` — the hard maximum wins.
    floor: f32,
    cap: f32,
    /// How far the last pass's clamp moved `size` off the proportional
    /// share: positive at the floor, negative at the cap.
    violation: f32,
    frozen: bool,
}

impl<K> FillItem<K> {
    pub(super) fn new(key: K, weight: f32, floor: f32, cap: f32) -> Self {
        Self {
            key,
            size: 0.0,
            weight,
            floor,
            cap,
            violation: 0.0,
            frozen: false,
        }
    }

    /// Split `budget` across `items`, writing each allocation into its
    /// own [`Self::size`]. Every item is allocated, including the ones a
    /// clamp pinned to a bound.
    pub(super) fn distribute(items: &mut [Self], budget: f32) {
        let mut remaining = budget;
        let mut active_weight: f64 = items.iter().map(|item| f64::from(item.weight)).sum();
        loop {
            let mut total_violation = 0.0_f64;
            let mut any_active = false;
            for item in items.iter_mut() {
                if item.frozen {
                    continue;
                }
                any_active = true;
                // Divided in f64 because `active_weight` sheds a term per
                // freeze and an f32 running total drifts. A pool of zero
                // total weight has no proportional answer, and the zero
                // share then clamps every item onto its own floor.
                let share = if active_weight > 0.0 {
                    (f64::from(remaining) * f64::from(item.weight) / active_weight) as f32
                } else {
                    0.0
                };
                item.size = share.clamp(item.floor.min(item.cap), item.cap);
                // An infinite budget hands an uncapped item an infinite
                // share, and `INF - INF` is a NaN that would stall the
                // sign test below.
                item.violation = if item.size == share {
                    0.0
                } else {
                    item.size - share
                };
                total_violation += f64::from(item.violation);
            }
            if !any_active {
                return;
            }
            // A total of zero — or a NaN one, from mixed infinities —
            // freezes the lot and ends the solve.
            let sign = if total_violation > 0.0 {
                1.0
            } else if total_violation < 0.0 {
                -1.0
            } else {
                0.0
            };
            for item in items.iter_mut() {
                if item.frozen || (sign != 0.0 && item.violation * sign <= 0.0) {
                    continue;
                }
                item.frozen = true;
                remaining -= item.size;
                active_weight -= f64::from(item.weight);
            }
            remaining = remaining.max(0.0);
        }
    }
}

#[cfg(test)]
mod tests;
