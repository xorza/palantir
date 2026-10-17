use super::*;

/// In range stands, out of range clamps, and only a non-number is refused,
/// infinities included: clamping one would pass a corrupt preference off as a
/// choice.
#[test]
fn new_clamps_to_the_ladder_ends_and_refuses_non_numbers() {
    for (factor, want) in [
        (1.37, Some(1.37)),
        (12.0, Some(UserScale::MAX)),
        (0.01, Some(UserScale::MIN)),
        (-1.0, Some(UserScale::MIN)),
        (f32::NAN, None),
        (f32::INFINITY, None),
        (f32::NEG_INFINITY, None),
    ] {
        assert_eq!(UserScale::new(factor).map(UserScale::get), want, "{factor}");
    }
    assert_eq!(UserScale::default(), UserScale::ONE);
}

/// The ladder must be sorted for the step searches to find the nearest rung.
#[test]
fn the_ladder_ascends_and_holds_one() {
    assert!(UserScale::LADDER.is_sorted_by(|a, b| a < b));
    assert!(UserScale::LADDER.contains(&1.0));
}

#[test]
fn stepping_walks_the_rungs_and_stops_at_the_ends() {
    // 1.0 is rung 5, so up is 1.1 and down is 0.9.
    let one = UserScale::ONE;
    assert_eq!(one.stepped_up().get(), 1.1);
    assert_eq!(one.stepped_down().get(), 0.9);
    // Two steps up from 1.0 is 1.25, and one back down is 1.1.
    assert_eq!(one.stepped_up().stepped_up().get(), 1.25);
    assert_eq!(one.stepped_up().stepped_up().stepped_down().get(), 1.1);

    let top = UserScale::new(UserScale::MAX).unwrap();
    assert_eq!(top.stepped_up(), top, "the top rung has nothing above it");
    let bottom = UserScale::new(UserScale::MIN).unwrap();
    assert_eq!(
        bottom.stepped_down(),
        bottom,
        "the bottom rung has nothing below it",
    );
}

/// A value between two rungs moves on the first step in both directions
/// (1.37 sits between 1.25 and 1.5). Within `EPS` of a rung it counts as on
/// it; past `EPS` above, a step down lands on it.
#[test]
fn stepping_from_between_rungs_lands_on_the_neighbors() {
    for (factor, up, down) in [
        (1.37, 1.5, 1.25),
        (1.0 + EPS * 0.5, 1.1, 0.9),
        (1.0 - EPS * 0.5, 1.1, 0.9),
        (1.0 + EPS * 2.0, 1.1, 1.0),
        (1.0 - EPS * 2.0, 1.0, 0.9),
    ] {
        let scale = UserScale::new(factor).unwrap();
        assert_eq!(scale.stepped_up().get(), up, "{factor} up");
        assert_eq!(scale.stepped_down().get(), down, "{factor} down");
    }
}

#[test]
fn percent_reads_as_a_label() {
    assert_eq!(UserScale::ONE.percent(), 100);
    assert_eq!(UserScale::new(1.25).unwrap().percent(), 125);
    assert_eq!(UserScale::new(0.67).unwrap().percent(), 67);
}
