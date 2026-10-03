use crate::layout::axis_share;
use crate::layout::fill_item::FillItem;
use crate::layout::hug_item::HugItem;
use crate::layout::measured::Measured;

/// The three phases in their order, with exact shares.
///
/// - A 30 px rigid item and a scroll wanting 400 beside a Fill item with
///   a 20 px floor, in 100: the floor is set aside, the Hug items share
///   80 — the rigid 30 and the scroll's 50 — and the Fill item takes the
///   20 left.
/// - The same items in 500: the Hug items take the 430 they want, and the
///   Fill item the 70 left; the shares hold from 430 + 20 = 450.
#[test]
fn fill_floors_come_first_then_hug_then_fill() {
    // (label, budget, hug shares, fill share, stable_from)
    let cases = [
        (
            "cramped",
            100.0,
            [30.0, 50.0],
            20.0,
            Measured::AT_OFFER_ONLY,
        ),
        ("roomy", 500.0, [30.0, 400.0], 70.0, 450.0),
    ];
    for (label, budget, hug_shares, fill_share, stable_from) in cases {
        let mut hugs = [HugItem::new(0, 30.0, 30.0), HugItem::new(1, 0.0, 400.0)];
        let mut fills = [FillItem::new(2, 1.0, 20.0, f32::INFINITY)];
        assert_eq!(
            axis_share::solve(&mut hugs, &mut fills, budget),
            stable_from,
            "{label}"
        );
        assert_eq!(hugs.map(|item| item.size), hug_shares, "{label}");
        assert_eq!(fills[0].size, fill_share, "{label}");
    }
}
