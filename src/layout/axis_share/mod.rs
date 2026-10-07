//! One axis shared among siblings: Fill floors are set aside, Hug items share the rest ([`HugItem::share`]), Fill items divide what remains ([`FillItem::distribute`]). CSS Grid's order, so a Hug sibling never squeezes Fill below its floor; grid tracks and stack children both solve here.

use crate::layout::fill_item::FillItem;
use crate::layout::hug_item::HugItem;

/// Share `budget` (the axis less gaps and fixed extents) between `hugs` and `fills`, writing each `size`; returns the least finite budget from which the Hug shares hold ([`HugItem::share`]), moved out by the Fill floors.
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
