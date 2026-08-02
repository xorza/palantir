//! Differential tests over the two Fill distributors.
//!
//! `stack::freeze_distribute` and `grid::resolve_axis`'s Phase 3 solve
//! the same problem — hand each weighted item its proportional share of
//! a budget, clamped to `[floor, cap]`, and re-divide whatever the
//! clamps free. They are deliberately *not* merged, and the comment on
//! `freeze_distribute` says why: they converge differently, so a shared
//! solver would silently change one driver's edge-case results.
//!
//! Nothing held them to that claim. These tests do, from both sides:
//! they agree on the ordinary shapes, and they provably do *not* agree
//! in general.

use crate::layout::{grid, stack};

/// `(label, items as (weight, floor, cap), budget)`.
type Case = (&'static str, &'static [(f32, f32, f32)], f32);

/// Shapes both solvers reach the same answer on — the ones real layouts
/// hit. Pinned so a change to either solver has to justify itself
/// against the common cases before the divergence case below.
const AGREEING: &[Case] = &[
    (
        "unconstrained_even_split",
        &[(1.0, 0.0, f32::INFINITY); 3],
        300.0,
    ),
    (
        "weighted_split",
        &[(1.0, 0.0, f32::INFINITY), (3.0, 0.0, f32::INFINITY)],
        400.0,
    ),
    // One item's cap frees space the rest re-divide.
    (
        "single_cap_violator",
        &[(1.0, 0.0, 10.0), (1.0, 0.0, f32::INFINITY)],
        100.0,
    ),
    // One item's floor eats space the rest give up.
    (
        "single_floor_violator",
        &[(1.0, 80.0, f32::INFINITY), (1.0, 0.0, f32::INFINITY)],
        100.0,
    ),
    // Both signs at once, but one violator of each — not enough to
    // separate them, whichever order they appear in.
    (
        "cap_then_floor",
        &[
            (1.0, 0.0, 10.0),
            (1.0, 40.0, f32::INFINITY),
            (1.0, 0.0, f32::INFINITY),
        ],
        100.0,
    ),
    (
        "floor_then_cap",
        &[
            (1.0, 40.0, f32::INFINITY),
            (1.0, 0.0, 10.0),
            (1.0, 0.0, f32::INFINITY),
        ],
        100.0,
    ),
    // A floor above its own cap: the hard max has to win in both.
    (
        "floor_above_cap",
        &[(1.0, 90.0, 20.0), (1.0, 0.0, f32::INFINITY)],
        100.0,
    ),
    ("zero_budget", &[(1.0, 0.0, 50.0), (2.0, 0.0, 50.0)], 0.0),
];

#[test]
fn the_two_fill_distributors_agree_on_the_ordinary_shapes() {
    for &(label, items, budget) in AGREEING {
        let from_stack = stack::internals::distribute_fill(items, budget);
        let from_grid = grid::internals::distribute_fill(items, budget);
        for (i, (s, g)) in from_stack.iter().zip(&from_grid).enumerate() {
            assert!(
                (s - g).abs() < 1e-3,
                "case `{label}` item {i}: stack gave {s}, grid gave {g}\n\
                 stack: {from_stack:?}\n grid:  {from_grid:?}",
            );
        }
    }
}

/// The case that separates them, pinned exactly.
///
/// Three items, two with a floor above their own cap, so both solvers
/// pin those at the cap. What differs is *when* the middle item's floor
/// is tested:
///
/// - `stack` sweeps entries in list order, so it reaches item 1 while
///   item 2 is still unfrozen. Item 1's share is `351 × 2/5 = 140.4`,
///   below its 173 floor, so it freezes there — and the room item 2's
///   cap frees a moment later arrives too late to help.
/// - `grid` clamps one violator per iteration and `swap_remove`s it,
///   which reorders the pool. It reaches item 2 first, caps it, and by
///   the time item 1 is tested its share has risen to 273 — above the
///   floor, so it never freezes at all.
///
/// Neither is wrong. Stack leaves 100 px unallocated, which its arrange
/// hands to `justify` as slack; grid has no such downstream step and
/// fills the budget. That is exactly why the two are kept apart, and
/// merging them silently picks one driver's answer for both.
#[test]
fn the_two_fill_distributors_diverge_on_freeze_order() {
    const ITEMS: &[(f32, f32, f32)] = &[
        (2.0, 174.0, 28.0),
        (2.0, 173.0, f32::INFINITY),
        (3.0, 96.0, 78.0),
    ];
    const BUDGET: f32 = 379.0;

    assert_eq!(
        stack::internals::distribute_fill(ITEMS, BUDGET),
        vec![28.0, 173.0, 78.0],
        "stack freezes item 1 at its floor before item 2's cap frees room",
    );
    assert_eq!(
        grid::internals::distribute_fill(ITEMS, BUDGET),
        vec![28.0, 273.0, 78.0],
        "grid caps item 2 first, so item 1's share clears its floor",
    );
}

/// The guard against merging them: a deterministic sweep that must keep
/// finding disagreements.
///
/// Eight hand-built cases all agreed — the divergence only shows up once
/// two items violate on *opposite* sides and a third shifts the freeze
/// order, which is more adversarial than anyone writes by hand. If a
/// future change makes the two interchangeable this fails, and whoever
/// sees it gets to delete one solver deliberately rather than by
/// accident.
///
/// The generator is a plain LCG so a failure reproduces from its seed
/// alone — no `rand` dependency, no per-run variation.
#[test]
fn the_two_fill_distributors_are_not_interchangeable() {
    fn next(state: &mut u64) -> u64 {
        *state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        *state >> 33
    }

    let mut state = 0x5DEE_CE66_D1CE_u64;
    let mut mixed_sign_cases = 0u32;
    let mut divergences = 0u32;
    for _ in 0..4_000u32 {
        let count = 1 + (next(&mut state) % 5) as usize;
        let budget = (next(&mut state) % 500) as f32;
        let share = budget / count as f32;
        let mut items = Vec::with_capacity(count);
        let (mut saw_floor, mut saw_cap) = (false, false);
        for _ in 0..count {
            let weight = 1.0 + (next(&mut state) % 4) as f32;
            let floor = (next(&mut state) % 200) as f32;
            // Half the caps are unbounded, so the mix isn't all-clamped.
            let cap = if next(&mut state).is_multiple_of(2) {
                f32::INFINITY
            } else {
                (next(&mut state) % 200) as f32
            };
            saw_floor |= floor > share;
            saw_cap |= cap < share;
            items.push((weight, floor, cap));
        }
        if saw_floor && saw_cap {
            mixed_sign_cases += 1;
        }

        let from_stack = stack::internals::distribute_fill(&items, budget);
        let from_grid = grid::internals::distribute_fill(&items, budget);
        if from_stack
            .iter()
            .zip(&from_grid)
            .any(|(s, g)| (s - g).abs() >= 1e-3)
        {
            divergences += 1;
        }
    }

    // The sweep is only worth anything if it reached the shape the split
    // exists for.
    assert!(
        mixed_sign_cases > 200,
        "sweep produced only {mixed_sign_cases} mixed floor+cap cases; \
         it isn't exercising the shape the split claims",
    );
    assert!(
        divergences > 0,
        "the two Fill distributors agreed on all 4000 sampled cases — if that \
         is now genuinely true, one of them should be deleted rather than \
         left as a hand-synced copy",
    );
}
