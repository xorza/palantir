use crate::internals::panic_probe;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::limits::assert_valid_bounds;

/// The pair screen runs in every build, so a node rejects the triple a
/// `Track` already rejects rather than carrying an inverted pair down to
/// `f32::clamp` in the arrange pass.
///
/// An infinite *maximum* is the unbounded axis and stays legal; every
/// other way the pair can go wrong is listed, because the point of one
/// screen is that each case is answered here.
#[test]
fn bounds_pairs_are_rejected_in_every_build() {
    assert_valid_bounds(Size::new(10.0, 10.0), Size::new(10.0, f32::INFINITY));
    assert_valid_bounds(Size::ZERO, Size::INF);

    let cases: &[(Size, Size, &str)] = &[
        (
            Size::new(20.0, 0.0),
            Size::new(10.0, 10.0),
            "inverted width",
        ),
        (
            Size::new(0.0, 20.0),
            Size::new(10.0, 10.0),
            "inverted height",
        ),
        (Size::new(f32::NAN, 0.0), Size::INF, "NaN minimum"),
        (Size::ZERO, Size::new(f32::NAN, 10.0), "NaN maximum"),
        (Size::new(-1.0, 0.0), Size::INF, "negative minimum"),
        (Size::ZERO, Size::new(-1.0, 10.0), "negative maximum"),
        (Size::INF, Size::INF, "infinite minimum"),
    ];
    for &(min_size, max_size, _label) in cases {
        panic_probe::assert_panics_with("node minimums must be finite", || {
            assert_valid_bounds(min_size, max_size);
        });
    }
}
