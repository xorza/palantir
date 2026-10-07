use crate::layout::hug_item::HugItem;
use crate::layout::measured::Measured;

/// `(label, (lo, hi) items, budget, shares, stable_from)`.
type Case = (
    &'static str,
    &'static [(f32, f32)],
    f32,
    &'static [f32],
    f32,
);

/// Each branch of the share, with exact shares: rigid 30 + 400 in 100 gives 30/70; 100 + 300 in 200 with floors 0 and 100 splits 100 of slack as 100/3 and 200/3; floors 60 + 60 in 100 overflow; 50 + 50 get their wants in 100 or unbounded; nothing wanted holds anywhere.
#[test]
fn hug_items_give_way_in_proportion_to_their_range() {
    const AT: f32 = Measured::AT_OFFER_ONLY;
    let cases: &[Case] = &[
        (
            "rigid beside a scroll",
            &[(30.0, 30.0), (0.0, 400.0)],
            100.0,
            &[30.0, 70.0],
            AT,
        ),
        (
            "slack by range",
            &[(0.0, 100.0), (100.0, 300.0)],
            200.0,
            &[100.0 / 3.0, 100.0 + 200.0 / 3.0],
            AT,
        ),
        (
            "floors overflow",
            &[(60.0, 80.0), (60.0, 90.0)],
            100.0,
            &[60.0, 60.0],
            AT,
        ),
        (
            "all fit",
            &[(10.0, 50.0), (0.0, 50.0)],
            100.0,
            &[50.0, 50.0],
            100.0,
        ),
        (
            "unbounded",
            &[(10.0, 50.0), (0.0, 50.0)],
            f32::INFINITY,
            &[50.0, 50.0],
            100.0,
        ),
        ("nothing wanted", &[(0.0, 0.0)], 0.0, &[0.0], 0.0),
    ];
    for &(label, items, budget, shares, stable_from) in cases {
        let mut pool: Vec<HugItem<usize>> = items
            .iter()
            .enumerate()
            .map(|(index, &(lo, hi))| HugItem::new(index, lo, hi))
            .collect();
        assert_eq!(HugItem::share(&mut pool, budget), stable_from, "{label}");
        let got: Vec<f32> = pool.iter().map(|item| item.size).collect();
        assert_eq!(got, shares, "{label}");
    }
}
