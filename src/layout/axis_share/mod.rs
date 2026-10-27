//! One axis shared among siblings: Fill floors are set aside, Hug items share the rest ([`HugItem::share`]), Fill items divide what remains ([`FillItem::distribute`]). CSS Grid's order, so a Hug sibling never squeezes Fill below its floor; grid tracks and stack children both solve here.
//!
//! - **Hug shrink is the auto table-column interpolation** (CSS 2.1 §17.5.2.2): short of max-content, each Hug item gets its min-content plus a share of the space past the mins in proportion to its max − min. Refused: CSS Flexbox §9.7's scaled shrink, which weighs by the base size and lets a wide item give up more than its slack.
//! - **Fill grow is WPF star sizing**: what remains splits by weight, above each Fill floor.
//! - **Overflow aligns to the leading edge**: content past the slot starts at the start edge, as WPF and CSS block layout place it.

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
