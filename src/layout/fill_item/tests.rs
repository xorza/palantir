use crate::layout::fill_item::FillItem;

fn distribute(items: &[(f32, f32, f32)], budget: f32) -> Vec<f32> {
    let mut pool: Vec<FillItem<usize>> = items
        .iter()
        .enumerate()
        .map(|(index, &(weight, floor, cap))| FillItem::new(index, weight, floor, cap))
        .collect();
    FillItem::distribute(&mut pool, budget);
    pool.iter().map(|item| item.size).collect()
}

/// `(label, items, budget, expected)`. Expected values are exact: the committing pass divides evenly.
type Case = (
    &'static str,
    &'static [(f32, f32, f32)],
    f32,
    &'static [f32],
);

const CASES: &[Case] = &[
    (
        "unconstrained_even_split",
        &[(1.0, 0.0, f32::INFINITY); 3],
        300.0,
        &[100.0, 100.0, 100.0],
    ),
    (
        "weighted_split",
        &[(1.0, 0.0, f32::INFINITY), (3.0, 0.0, f32::INFINITY)],
        400.0,
        &[100.0, 300.0],
    ),
    (
        "single_cap_violator",
        &[(1.0, 0.0, 10.0), (1.0, 0.0, f32::INFINITY)],
        100.0,
        &[10.0, 90.0],
    ),
    (
        "single_floor_violator",
        &[(1.0, 80.0, f32::INFINITY), (1.0, 0.0, f32::INFINITY)],
        100.0,
        &[80.0, 20.0],
    ),
    // 33.33 each unclamped: violations -23.33 and +6.67 total negative, so only the cap freezes and
    // the 90 left splits evenly, which clears item 1's floor.
    (
        "cap_outweighs_floor",
        &[
            (1.0, 0.0, 10.0),
            (1.0, 40.0, f32::INFINITY),
            (1.0, 0.0, f32::INFINITY),
        ],
        100.0,
        &[10.0, 45.0, 45.0],
    ),
    (
        "cap_outweighs_floor_reordered",
        &[
            (1.0, 40.0, f32::INFINITY),
            (1.0, 0.0, 10.0),
            (1.0, 0.0, f32::INFINITY),
        ],
        100.0,
        &[45.0, 10.0, 45.0],
    ),
    // A floor above its own cap: the hard max wins, so item 0 takes 20.
    (
        "floor_above_cap",
        &[(1.0, 90.0, 20.0), (1.0, 0.0, f32::INFINITY)],
        100.0,
        &[20.0, 80.0],
    ),
    (
        "zero_budget",
        &[(1.0, 0.0, 50.0), (2.0, 0.0, 50.0)],
        0.0,
        &[0.0, 0.0],
    ),
    (
        "zero_total_weight",
        &[(0.0, 40.0, f32::INFINITY), (0.0, 0.0, f32::INFINITY)],
        100.0,
        &[40.0, 0.0],
    ),
    // A Hug parent's unbounded budget: the capped item takes its cap, the uncapped one infinity.
    (
        "infinite_budget",
        &[(1.0, 0.0, 50.0), (1.0, 0.0, f32::INFINITY)],
        f32::INFINITY,
        &[50.0, f32::INFINITY],
    ),
];

#[test]
fn distribution_matches_the_hand_computed_shares() {
    for &(label, items, budget, expected) in CASES {
        assert_eq!(distribute(items, budget), expected, "case `{label}`");
    }
}

/// Items 0 and 2 have floors above their caps, so both pin at the cap; the total violation is `-100`,
/// so only they freeze and item 1 gets all 273 left (over its 173 floor). The budget is spent to the pixel.
#[test]
fn a_freed_cap_clears_a_later_floor() {
    const ITEMS: &[(f32, f32, f32)] = &[
        (2.0, 174.0, 28.0),
        (2.0, 173.0, f32::INFINITY),
        (3.0, 96.0, 78.0),
    ];
    const BUDGET: f32 = 379.0;

    let sizes = distribute(ITEMS, BUDGET);
    assert_eq!(sizes, vec![28.0, 273.0, 78.0]);
    assert_eq!(sizes.iter().sum::<f32>(), BUDGET);
}

/// An item's allocation depends on the item, not its list position. A plain LCG makes failures reproducible.
#[test]
fn allocations_do_not_depend_on_item_order() {
    fn next(state: &mut u64) -> u64 {
        *state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        *state >> 33
    }

    let mut state = 0x5DEE_CE66_D1CE_u64;
    let mut mixed_sign_cases = 0u32;
    for _ in 0..4_000u32 {
        let count = 1 + (next(&mut state) % 5) as usize;
        let budget = (next(&mut state) % 500) as f32;
        let share = budget / count as f32;
        let mut items = Vec::with_capacity(count);
        let (mut saw_floor, mut saw_cap) = (false, false);
        for _ in 0..count {
            let weight = 1.0 + (next(&mut state) % 4) as f32;
            let floor = (next(&mut state) % 200) as f32;
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

        let forward = distribute(&items, budget);
        let mut reversed_items = items.clone();
        reversed_items.reverse();
        let mut reversed = distribute(&reversed_items, budget);
        reversed.reverse();
        for (index, (a, b)) in forward.iter().zip(&reversed).enumerate() {
            // The runs subtract frozen sizes in opposite orders, so they agree to f32 rounding.
            assert_eq!(
                a, b,
                "item {index} got {a} in list order and {b} reversed\n\
                 items: {items:?}\n forward: {forward:?}\n reversed: {reversed:?}"
            );
        }
    }

    assert!(
        mixed_sign_cases > 200,
        "sweep produced only {mixed_sign_cases} mixed floor+cap cases; \
         it isn't exercising the shape the freeze rule turns on",
    );
}

/// Every allocation lands inside its own bounds, whatever the budget.
#[test]
fn every_allocation_respects_its_own_bounds() {
    const ITEMS: &[(f32, f32, f32)] = &[
        (1.0, 30.0, 40.0),
        (2.0, 0.0, f32::INFINITY),
        (1.0, 90.0, 20.0),
    ];
    for budget in [0.0, 25.0, 140.0, 1_000.0] {
        let sizes = distribute(ITEMS, budget);
        for (index, (&size, &(_, floor, cap))) in sizes.iter().zip(ITEMS).enumerate() {
            assert!(
                size >= floor.min(cap) && size <= cap,
                "budget {budget}: item {index} got {size}, outside [{}, {cap}]",
                floor.min(cap),
            );
        }
    }
}
