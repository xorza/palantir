//! The one `Sizing::fill` distributor, shared by stack Fill children and grid Fill tracks: each
//! weighted item gets a proportional share of a budget, clamped to `[floor, cap]`, and freed room is
//! re-divided. Freezing follows CSS Flexbox §9.7: each pass clamps unfrozen items, sums the
//! adjustments, and freezes only violators whose adjustment has the total's sign. Freezing all
//! violators pins an item at its floor before a later cap frees room; freezing one per pass makes
//! the answer depend on push order.

/// One participant in a Fill distribution; `key` is the caller's handle, carried through untouched.
#[derive(Clone, Copy, Debug)]
pub(super) struct FillItem<K> {
    pub(super) key: K,
    pub(super) size: f32,
    weight: f32,
    /// Least extent this item accepts; a floor above `cap` resolves to `cap`.
    floor: f32,
    cap: f32,
    /// How far the last clamp moved `size` off the share: positive at the floor, negative at the cap.
    violation: f32,
    frozen: bool,
}

impl<K> FillItem<K> {
    pub(super) const fn new(key: K, weight: f32, floor: f32, cap: f32) -> Self {
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

    pub(super) const fn floor(&self) -> f32 {
        self.floor.min(self.cap)
    }

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
                // f64 because `active_weight` sheds a term per freeze and an f32 total drifts. Zero total weight
                // gives a zero share, which clamps every item onto its own floor.
                let share = if active_weight > 0.0 {
                    (f64::from(remaining) * f64::from(item.weight) / active_weight) as f32
                } else {
                    0.0
                };
                item.size = share.clamp(item.floor.min(item.cap), item.cap);
                // An infinite budget gives an uncapped item an infinite share; `INF - INF` would stall the sign test.
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
            // A zero or NaN total (mixed infinities) freezes everything and ends the solve.
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
