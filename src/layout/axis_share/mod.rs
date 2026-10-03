//! One axis shared among siblings: the Fill items' floors are set aside,
//! the Hug items share what is left ([`HugItem::share`]), and the Fill
//! items divide the rest ([`FillItem::distribute`]).
//!
//! The order is CSS Grid's: a `1fr` track's base is its min-content
//! before auto tracks grow, so a Fill item is never squeezed below its
//! floor by a Hug sibling. Every container that shares an axis between
//! its children solves it here — the grid's tracks, the stack's children
//! — so a child's `Sizing` reads the same in each.

use crate::layout::fill_item::FillItem;
use crate::layout::hug_item::HugItem;

/// Share `budget` — the axis less its gaps and fixed extents — between
/// `hugs` and `fills`, writing each one's `size`.
///
/// Returns the least finite budget from which the Hug shares hold, as
/// [`HugItem::share`] states it, moved out by the Fill floors set aside
/// before them. Fill shares read every budget; a caller whose answer
/// depends on them answers for that.
pub(super) fn solve<K>(hugs: &mut [HugItem<K>], fills: &mut [FillItem<K>], budget: f32) -> f32 {
    let fill_floors: f32 = fills.iter().map(FillItem::floor).sum();
    let shares_from = HugItem::share(hugs, (budget - fill_floors).max(0.0));
    let hugs_total: f32 = hugs.iter().map(|item| item.size).sum();
    FillItem::distribute(fills, (budget - hugs_total).max(0.0));
    if shares_from > 0.0 {
        shares_from + fill_floors
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests;
