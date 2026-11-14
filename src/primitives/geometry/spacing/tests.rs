use crate::internals::panic_probe;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::*;
use crate::primitives::math::lerp::Lerp;
use crate::primitives::packed::serde::LaneCodec;
use crate::primitives::packed::serde::internals::{from_ron, ron_text};
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;

/// Every constructor refuses an edge that does not pack to a finite f16, on any lane: NaN, an
/// infinity, and 65520, the first magnitude f16 rounds to infinity. 65519 rounds down to 65504
/// and is taken.
#[test]
fn an_edge_past_f16_is_refused_where_it_is_built() {
    for bad in [f32::NAN, f32::INFINITY, 65520.0, -65520.0] {
        for lane in 0..4 {
            let mut lanes = [1.0, 2.0, 3.0, 4.0];
            lanes[lane] = bad;
            let [left, top, right, bottom] = lanes;
            panic_probe::assert_panics_with(Spacing::LANE_RULE, || {
                Spacing::new(left, top, right, bottom)
            });
        }
        panic_probe::assert_panics_with(Spacing::LANE_RULE, || Spacing::all(bad));
        panic_probe::assert_panics_with(Spacing::LANE_RULE, || Spacing::xy(1.0, bad));
        panic_probe::assert_panics_with(Spacing::LANE_RULE, || Spacing::new(1.0, 2.0, 3.0, bad));
    }
    assert_eq!(
        Spacing::new(65519.0, -65519.0, 0.0, 1.0).to_array(),
        [65504.0, -65504.0, 0.0, 1.0]
    );
}

/// A padding or margin past its lanes panics at the caller's line, through the number and both
/// tuple conversions `Configure::padding` and `margin` take, as AGENTS.md's input validation asks.
#[test]
fn a_bad_spacing_panics_at_the_call_site() {
    type Case = (u32, fn());
    let cases: [Case; 5] = [
        (line!(), || drop(Widget::leaf().padding(f32::NAN))),
        (line!(), || drop(Widget::leaf().padding((1.0, f32::NAN)))),
        (line!(), || drop(Widget::leaf().padding((1, 2, 3, 65520)))),
        (line!(), || drop(Widget::leaf().margin(f32::INFINITY))),
        (line!(), || _ = Spacing::from((f32::NAN, 1.0))),
    ];
    for (n, (line, record)) in cases.into_iter().enumerate() {
        let site = panic_probe::panic_site(Spacing::LANE_RULE, record);
        assert_eq!(site, format!("{}:{line}", file!()), "case {n}");
    }
}

/// A derived spacing saturates where an authored one is refused: ±70000 and ±infinity land on
/// ±65504, and a NaN still panics. A sum is derived: 60000 + 60000 lands on 65504 and
/// −60000 + −60000 on −65504, each lane alone, as 1 + 2 is still 3 beside them.
#[test]
fn a_derived_spacing_saturates_at_the_f16_range() {
    assert_eq!(
        Spacing::saturating([70000.0, -70000.0, f32::INFINITY, 1.0]).to_array(),
        [65504.0, -65504.0, 65504.0, 1.0]
    );
    let wide = Spacing::new(60000.0, -60000.0, 1.0, 0.0);
    assert_eq!(
        (wide + Spacing::new(60000.0, -60000.0, 2.0, 0.0)).to_array(),
        [65504.0, -65504.0, 3.0, 0.0]
    );
    panic_probe::assert_panics_with(Spacing::LANE_RULE, || {
        Spacing::saturating([0.0, f32::NAN, 0.0, 0.0])
    });
}

/// `to_array` reads the edges back in `new`'s order, left, top, right, bottom.
#[test]
fn to_array_reads_the_edges_in_new_order() {
    let original = Spacing::new(1.0, 2.0, 3.0, 4.0);
    assert_eq!(original.to_array(), [1.0, 2.0, 3.0, 4.0]);
    assert_eq!(original.sums(), Size::new(4.0, 6.0));
}

#[test]
fn xy_ctor_repeats_axes() {
    let s = Spacing::xy(3.0, 7.0);
    assert_eq!(s.to_array(), [3.0, 7.0, 3.0, 7.0]);
    assert_eq!(s.sums(), Size::new(6.0, 14.0));
}

#[test]
fn from_tuple_preserves_component_order() {
    let xy: Spacing = (3, 7).into();
    assert_eq!(xy.to_array(), [3.0, 7.0, 3.0, 7.0]);
    let ltrb: Spacing = (1, 2, 3, 4).into();
    assert_eq!(ltrb.to_array(), [1.0, 2.0, 3.0, 4.0]);
}

/// Spacing's own choices on the shared wire format: matched axes write as
/// `[horizontal, vertical]`, a diagonal match stays four lanes, and a table names `left`, `top`,
/// `right`, `bottom`. Each text reads back as the spacing it was written from.
#[test]
fn spacing_pairs_axes_and_names_each_edge() {
    for (spacing, text) in [
        (Spacing::xy(4.0, 8.0), "[4.0,8.0]"),
        (Spacing::new(1.0, 1.0, 2.0, 2.0), "[1.0,1.0,2.0,2.0]"),
    ] {
        assert_eq!(ron_text(&spacing), text);
        assert_eq!(from_ron::<Spacing>(text), spacing, "{text}");
    }
    assert_eq!(
        from_ron::<Spacing>("(left: 1.0, top: 2.0, right: 3.0, bottom: 4.0)"),
        Spacing::new(1.0, 2.0, 3.0, 4.0)
    );
}

/// A file lane is any f32 that rounds to a finite f16, either sign: ±65519.99 rounds to
/// ±65504, the largest, and ±65520 packs to infinity.
#[test]
fn deserialize_rejects_a_lane_past_f16() {
    for (input, valid) in [
        ("65504.0", true),
        ("-65504.0", true),
        ("65519.99", true),
        ("-65519.99", true),
        ("65520.0", false),
        ("-65520.0", false),
        ("inf", false),
        ("NaN", false),
    ] {
        let parsed = ron::from_str::<Spacing>(input);
        assert_eq!(parsed.is_ok(), valid, "{input}: {parsed:?}");
        if let Err(error) = parsed {
            assert!(
                error.to_string().contains(Spacing::LANE_RULE),
                "{input}: {error}"
            );
        }
    }
}

#[test]
fn add_op() {
    let a = Spacing::new(1.0, 2.0, 3.0, 4.0);
    let b = Spacing::new(10.0, 20.0, 30.0, 40.0);
    let c = a + b;
    assert_eq!(c.to_array(), [11.0, 22.0, 33.0, 44.0]);
}

/// Both lane types blend lane by lane in f32, ending exactly, and their arithmetic saturates at
/// the largest f16, 65504, rather than packing to infinity.
#[test]
fn lanes_blend_and_add_one_by_one() {
    use crate::primitives::math::animatable::Animatable;

    let (a, b) = (
        Spacing::new(0.0, 2.0, 4.0, 8.0),
        Spacing::new(4.0, 6.0, 8.0, 16.0),
    );
    // 0 + 4·¼ = 1, 2 + 4·¼ = 3, 4 + 4·¼ = 5, 8 + 8·¼ = 10.
    assert_eq!(
        Spacing::lerp(&a, &b, 0.25),
        Spacing::new(1.0, 3.0, 5.0, 10.0)
    );
    assert_eq!(Spacing::lerp(&a, &b, 0.0), a);
    assert_eq!(Spacing::lerp(&a, &b, 1.0), b);
    assert_eq!(b.sub(&a), Spacing::new(4.0, 4.0, 4.0, 8.0));
    assert_eq!(a.add(&b.sub(&a)), b);
    assert_eq!(b.scale(0.5), Spacing::new(2.0, 3.0, 4.0, 8.0));
    // 0² + 2² + 4² + 8² = 84.
    assert_eq!(a.magnitude_squared(), 84.0);
    assert_eq!(
        Spacing::all(60000.0).add(&Spacing::all(60000.0)),
        Spacing::all(65504.0)
    );

    let (c, d) = (Corners::all(2.0), Corners::new(6.0, 10.0, 2.0, 0.0));
    // 2 + 4·½ = 4, 2 + 8·½ = 6, 2, 2 − 2·½ = 1.
    assert_eq!(Corners::lerp(&c, &d, 0.5), Corners::new(4.0, 6.0, 2.0, 1.0));
    assert_eq!(Corners::lerp(&c, &d, 1.0), d);
}
