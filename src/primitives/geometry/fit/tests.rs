use super::*;

/// The extent (10, 20)–(30, 30), 20 × 10 about (20, 25), into the rect (0, 0)–(100, 100) about
/// (50, 50): `Fill` scales by (100 / 20, 100 / 10) = (5, 10), offset `(0, 0) − (10, 20) · scale`
/// so the ends meet; `Contain` by min = 5 and `Cover` by max = 10, each offset
/// `(50, 50) − (20, 25) · scale`; `Natural` moves it to the rect's corner.
#[test]
fn each_fit_maps_the_extent_by_hand() {
    let extent = Rect::new(10.0, 20.0, 20.0, 10.0);
    let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
    let corners = |fit: Fit| {
        let map = fit.map(extent, rect);
        [
            map.apply(Vec2::new(10.0, 20.0)),
            map.apply(Vec2::new(30.0, 30.0)),
        ]
    };
    assert_eq!(
        corners(Fit::Natural),
        [Vec2::new(10.0, 20.0), Vec2::new(30.0, 30.0)]
    );
    assert_eq!(
        corners(Fit::Fill),
        [Vec2::new(0.0, 0.0), Vec2::new(100.0, 100.0)]
    );
    assert_eq!(
        corners(Fit::Contain),
        [Vec2::new(0.0, 25.0), Vec2::new(100.0, 75.0)]
    );
    assert_eq!(
        corners(Fit::Cover),
        [Vec2::new(-50.0, 0.0), Vec2::new(150.0, 100.0)]
    );
    assert_eq!(
        Fit::Natural.map(extent, Rect::new(4.0, 6.0, 1.0, 1.0)),
        FitMap {
            offset: Vec2::new(4.0, 6.0),
            scale: Vec2::ONE,
        },
        "`Natural` takes the rect's corner alone",
    );
}

/// An axis of no extent takes no part in the scale and centers: the line (0, 5)–(20, 5) into
/// (0, 0)–(40, 10) scales by 40 / 20 = 2 and keeps y = 5, the rect's middle, under every fit but
/// `Natural`. A point takes scale 1 and lands on the rect's center.
#[test]
fn an_axis_of_no_extent_centers() {
    let line = Rect::new(0.0, 5.0, 20.0, 0.0);
    let rect = Rect::new(0.0, 0.0, 40.0, 10.0);
    for fit in [Fit::Fill, Fit::Contain, Fit::Cover] {
        let map = fit.map(line, rect);
        assert_eq!(map.scale.x, 2.0, "{fit:?}");
        assert_eq!(
            [
                map.apply(Vec2::new(0.0, 5.0)),
                map.apply(Vec2::new(20.0, 5.0))
            ],
            [Vec2::new(0.0, 5.0), Vec2::new(40.0, 5.0)],
            "{fit:?}"
        );
    }
    assert_eq!(Fit::Fill.map(line, rect).scale.y, 1.0);
    let point = Rect::new(3.0, 3.0, 0.0, 0.0);
    let map = Fit::Cover.map(point, Rect::new(0.0, 0.0, 10.0, 20.0));
    assert_eq!(map.scale, Vec2::ONE);
    assert_eq!(map.apply(Vec2::splat(3.0)), Vec2::new(5.0, 10.0));
}

/// Artwork 24 × 12 in the rect (10, 20) 100 × 100: `Contain` scales by 100 / 24 to 100 × 50,
/// centered 25 down; `Fill` takes the rect; `Natural` keeps 24 × 12 at the rect's corner; `Cover`
/// scales by 100 / 12 to 200 × 100, centered 50 left. A degenerate artwork centers.
#[test]
fn artwork_places_by_hand() {
    let rect = Rect::new(10.0, 20.0, 100.0, 100.0);
    let art = Size::new(24.0, 12.0);
    assert_eq!(
        Fit::Contain.place_artwork(rect, art),
        Rect::new(10.0, 45.0, 100.0, 50.0)
    );
    assert_eq!(Fit::Fill.place_artwork(rect, art), rect);
    assert_eq!(
        Fit::Natural.place_artwork(rect, art),
        Rect::new(10.0, 20.0, 24.0, 12.0)
    );
    let cover = Fit::Cover.place_artwork(rect, art);
    assert!(
        cover.min.abs_diff_eq(Vec2::new(-40.0, 20.0), 1e-4),
        "{cover:?}"
    );
    assert!((cover.size.w - 200.0).abs() < 1e-4, "{cover:?}");
    assert_eq!(cover.size.h, 100.0);
    assert_eq!(
        Fit::Contain.place_artwork(rect, Size::ZERO),
        Rect::new(60.0, 70.0, 0.0, 0.0)
    );
}
