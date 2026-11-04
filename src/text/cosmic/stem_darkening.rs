//! [`StemDarkening`]: how far a glyph's outline grows before it is rasterized.

use glam::Vec2;
use std::iter;
use std::ops::Range;
use swash::scale::outline::Outline;
use swash::zeno::{Point, Verb};

/// The dilation of a glyph's outline on each side, in the outline's own units, as macOS dilates
/// text: more at small sizes, capped, and none past a size.
///
/// Coverage blends in linear light, which is correct but thins dark text on a light ground. A
/// dilation of the outline fixes that, as FreeType's CFF driver, macOS, Pathfinder and Linebender's
/// renderers do; the constants are Pathfinder's, which it states match macOS 10.13
/// ([FreeType stem darkening](https://freetype.org/freetype2/docs/hinting/text-rendering-general.html),
/// [Pathfinder `effects.rs`](https://github.com/servo/pathfinder/blob/main/content/src/effects.rs),
/// [Linebender gamma](https://linebender.org/wiki/text/gamma/)). Refused: the DirectWrite and Skia
/// contrast and gamma corrections, which assume coverage blends in gamma space.
#[derive(Clone, Copy, Debug)]
pub(super) struct StemDarkening {
    /// Per side, x then y.
    amount: Vec2,
}

impl StemDarkening {
    /// The dilation per pixel per em, x then y.
    const FACTOR: Vec2 = Vec2::new(0.0121, 0.0121 * 1.25);
    /// The most a side grows, in pixels.
    const MAX_PX: f32 = 0.3;
    /// Above this size no outline grows.
    const MAX_PPEM: f32 = 72.0;

    /// No growth, for an outline drawn as its artwork stands.
    pub(super) const NONE: Self = Self { amount: Vec2::ZERO };

    /// The dilation at `ppem` pixels per em, in pixels.
    pub(super) fn at_ppem(ppem: f32) -> Self {
        let amount = if ppem > Self::MAX_PPEM {
            Vec2::ZERO
        } else {
            (Self::FACTOR * ppem).min(Vec2::splat(Self::MAX_PX))
        };
        Self { amount }
    }

    /// The most any size and scale dilates by, in an outline of `units_per_em` units:
    /// [`Self::FACTOR`] ems. Drawn at scale `s`, a glyph of `size` px grows by
    /// `at_ppem(size * s) / s` of its own pixels, `FACTOR * size` below the cap and less above.
    pub(super) fn bound(units_per_em: f32) -> Self {
        Self {
            amount: Self::FACTOR * units_per_em,
        }
    }

    /// Grow `outline` by the amount on each side, about where it stands: FreeType's
    /// `FT_Outline_EmboldenXY` at twice the amount, without its shift of every point by the
    /// amount toward `+x` and `+y`, so a straight edge moves out by the amount. A layer whose
    /// orientation reads neither way is left as it is, as FreeType leaves it.
    ///
    /// Refused: swash's `Outline::embolden`, a port of the same function that reads a layer's
    /// orientation from all its points as one polygon. FreeType sums each contour's area; the two
    /// overlapping clockwise contours of Inter's `f` read counterclockwise as one polygon, and
    /// swash thins that glyph where it should grow it.
    pub(super) fn dilate(self, outline: &mut Outline) {
        if self.amount == Vec2::ZERO {
            return;
        }
        for idx in 0..outline.len() {
            let mut layer = outline.get_mut(idx).expect("a layer below the count");
            let verbs = layer.verbs();
            let points = layer.points_mut();
            let Some(orientation) = Orientation::of(points, verbs) else {
                continue;
            };
            for contour in contours(verbs) {
                dilate_contour(&mut points[contour], orientation, self.amount);
            }
        }
    }
}

/// Which way an outline's filled contours turn, in y-up units, as FreeType's
/// `FT_Outline_Get_Orientation` reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Orientation {
    /// Clockwise, as TrueType draws a filled contour.
    Clockwise,
    /// Counterclockwise, as PostScript and CFF draw one.
    Counterclockwise,
}

impl Orientation {
    /// The sign of the sum of each contour's area, `None` when it is zero. The terms are
    /// FreeType's, twice the area with counterclockwise positive, summed in `f64`, as a glyph in
    /// font units sums to millions.
    fn of(points: &[Point], verbs: &[Verb]) -> Option<Self> {
        let area: f64 = contours(verbs)
            .map(|contour| {
                let contour = &points[contour];
                let mut prev = contour[contour.len() - 1];
                contour
                    .iter()
                    .map(|&cur| {
                        let term = f64::from(cur.y - prev.y) * f64::from(cur.x + prev.x);
                        prev = cur;
                        term
                    })
                    .sum::<f64>()
            })
            .sum();
        if area > 0.0 {
            Some(Self::Counterclockwise)
        } else if area < 0.0 {
            Some(Self::Clockwise)
        } else {
            None
        }
    }
}

/// The point range of each contour `verbs` draws, none empty: a contour starts at a `MoveTo` and
/// ends at its `Close`, the next `MoveTo` or the end.
fn contours(verbs: &[Verb]) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut verbs = verbs.iter();
    let mut start = 0;
    let mut end = 0;
    iter::from_fn(move || {
        for verb in verbs.by_ref() {
            let open = end > start;
            match verb {
                Verb::MoveTo if open => {
                    let contour = start..end;
                    start = end;
                    end += 1;
                    return Some(contour);
                }
                Verb::Close if open => {
                    let contour = start..end;
                    start = end;
                    return Some(contour);
                }
                Verb::Close => {}
                Verb::MoveTo | Verb::LineTo => end += 1,
                Verb::QuadTo => end += 2,
                Verb::CurveTo => end += 3,
            }
        }
        (end > start).then(|| {
            let contour = start..end;
            start = end;
            contour
        })
    })
}

/// FreeType's `FT_Outline_EmboldenXY` over one contour at `strength`, its halved strength, without
/// the shift by `strength`: each run of points at one place moves along the bisector of the edges
/// around it, `strength` per axis on a straight edge, less where an edge is too short to take it.
/// A turn sharper than about 160 degrees does not move. Counter `j` walks the points; `i` follows
/// it as points move, and the anchor `k` marks the first point moved, so the walk ends within two
/// passes of the contour.
fn dilate_contour(points: &mut [Point], orientation: Orientation, strength: Vec2) {
    let last = points.len() - 1;
    let next = |idx: usize| if idx < last { idx + 1 } else { 0 };
    let at = |point: Point| Vec2::new(point.x, point.y);
    let mut dir_in = Vec2::ZERO;
    let mut len_in = 0.0;
    let mut anchor = Vec2::ZERO;
    let mut len_anchor = 0.0;
    let mut i = last;
    let mut j = 0;
    let mut k = None;
    while j != i && k != Some(i) {
        let (dir_out, len_out) = if k == Some(j) {
            (anchor, len_anchor)
        } else {
            let out = at(points[j]) - at(points[i]);
            let len = out.length();
            if len == 0.0 {
                j = next(j);
                continue;
            }
            (out / len, len)
        };
        if len_in == 0.0 {
            i = j;
        } else {
            if k.is_none() {
                k = Some(i);
                anchor = dir_in;
                len_anchor = len_in;
            }
            let shift = bisector_shift(Edges {
                dir_in,
                dir_out,
                len_min: f32::min(len_in, len_out),
                orientation,
                strength,
            });
            while i != j {
                points[i].x += shift.x;
                points[i].y += shift.y;
                i = next(i);
            }
        }
        dir_in = dir_out;
        len_in = len_out;
        j = next(j);
    }
}

/// The two edges at a point of a contour, as [`bisector_shift`] reads them.
#[derive(Clone, Copy, Debug)]
struct Edges {
    /// Unit direction of the edge into the point.
    dir_in: Vec2,
    /// Unit direction of the edge out of it.
    dir_out: Vec2,
    /// The shorter edge's length, which caps the shift so a short edge does not fold.
    len_min: f32,
    orientation: Orientation,
    strength: Vec2,
}

/// How far a point moves, FreeType's shift along the lateral bisector of its two edges.
fn bisector_shift(edges: Edges) -> Vec2 {
    let Edges {
        dir_in,
        dir_out,
        len_min,
        orientation,
        strength,
    } = edges;
    let cos = dir_in.dot(dir_out);
    // FreeType's `-0xF000` in 16.16: a turn of about 160 degrees.
    if cos <= -0.9375 {
        return Vec2::ZERO;
    }
    let d = cos + 1.0;
    let mut shift = Vec2::new(dir_in.y + dir_out.y, dir_in.x + dir_out.x);
    let mut q = dir_out.x * dir_in.y - dir_out.y * dir_in.x;
    match orientation {
        Orientation::Clockwise => {
            shift.x = -shift.x;
            q = -q;
        }
        Orientation::Counterclockwise => shift.y = -shift.y,
    }
    let axis = |component: f32, strength: f32| {
        if strength * q <= len_min * d {
            component * strength / d
        } else {
            component * len_min / q
        }
    };
    Vec2::new(axis(shift.x, strength.x), axis(shift.y, strength.y))
}

#[cfg(test)]
mod tests {
    use crate::text::cosmic::stem_darkening::{StemDarkening, contours};
    use crate::text::cosmic::tests::mono;
    use crate::text::cosmic::{Hinting, ScalerSpec};
    use cosmic_text::Weight;
    use glam::Vec2;
    use swash::scale::ScaleContext;
    use swash::scale::outline::Outline;
    use swash::zeno::Verb;

    /// Each contour's points, from its `MoveTo` to its `Close`, the next `MoveTo` or the end: a
    /// line pair closed (points 0 to 2), a curve left open by a `MoveTo` (3 to 5), and a cubic
    /// open at the end (6 to 9). A `Close` with nothing open adds none, and no verbs give none.
    #[test]
    fn contours_split_at_each_close_and_move() {
        use Verb::{Close, CurveTo, LineTo, MoveTo, QuadTo};
        let verbs = [
            MoveTo, LineTo, LineTo, Close, Close, MoveTo, QuadTo, MoveTo, CurveTo,
        ];
        assert_eq!(contours(&verbs).collect::<Vec<_>>(), [0..3, 3..6, 6..10]);
        assert_eq!(contours(&[]).count(), 0);
        assert_eq!(contours(&[Close]).count(), 0);
    }

    /// The box of JetBrains Mono's `I`, read in its 1000-unit em, then dilated by `amount`.
    fn i_bounds(amount: Vec2) -> [f32; 4] {
        let font = mono();
        let mut ctx = ScaleContext::new();
        let mut scaler = ScalerSpec {
            size: 0.0,
            hinting: Hinting::Off,
            weight: Weight::NORMAL,
        }
        .scaler(&mut ctx, &font);
        let mut outline = Outline::new();
        let glyph = font.as_swash().charmap().map('I');
        assert!(scaler.scale_outline_into(glyph, &mut outline));
        StemDarkening { amount }.dilate(&mut outline);
        let b = outline.bounds();
        [b.min.x, b.min.y, b.max.x, b.max.y]
    }

    /// `I` is bounded by straight edges that meet at right angles, so the dilation moves each
    /// edge out by the amount: 20 units across and 30 up, on both sides, the box centered where
    /// it was. FreeType's own shift would leave the left and bottom edges in place and move the
    /// right and top by 40 and 60.
    #[test]
    fn a_straight_outline_grows_by_the_amount_on_each_side() {
        let [x0, y0, x1, y1] = i_bounds(Vec2::ZERO);
        let grown = i_bounds(Vec2::new(20.0, 30.0));
        // Swash normalizes each edge's direction, one rounding off an exact unit vector.
        let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
        for (side, want) in [x0 - 20.0, y0 - 30.0, x1 + 20.0, y1 + 30.0]
            .into_iter()
            .enumerate()
        {
            assert!(
                near(grown[side], want),
                "side {side}: {grown:?}, want {want}"
            );
        }
    }

    /// The amount is `0.0121 * ppem` across and `0.015125 * ppem` up, each capped at 0.3 px, and
    /// none past 72 ppem. At 10 ppem: 0.121 and 0.15125. At 24 ppem the vertical caps
    /// (0.363 > 0.3) and the horizontal does not (0.2904). At 30 ppem both cap. 72 ppem still
    /// darkens; just past it, nothing does. Zero ppem is zero.
    #[test]
    fn the_amount_grows_with_size_caps_and_stops() {
        let at = |ppem: f32| StemDarkening::at_ppem(ppem).amount;
        // One f32 rounding of the product apart from the decimal.
        let near = |a: Vec2, b: Vec2| (a - b).abs().max_element() < 1e-6;
        assert!(near(at(10.0), Vec2::new(0.121, 0.15125)), "{}", at(10.0));
        assert!(near(at(24.0), Vec2::new(0.2904, 0.3)), "{}", at(24.0));
        assert_eq!(at(24.0).y, 0.3);
        assert_eq!(at(30.0), Vec2::splat(0.3));
        assert_eq!(at(72.0), Vec2::splat(0.3));
        assert_eq!(at(72.01), Vec2::ZERO);
        assert_eq!(at(0.0), Vec2::ZERO);
    }

    /// The bound is the factor in ems: 0.0121 and 0.015125 em, so 24.2 and 30.25 units in a
    /// 2000-unit em. It equals the amount below the cap, scaled to the em: at 10 ppem, 0.121 px
    /// of a 10 px em is 0.0121 em.
    #[test]
    fn the_bound_is_the_factor_in_ems() {
        let bound = StemDarkening::bound(2000.0).amount;
        assert!((bound - Vec2::new(24.2, 30.25)).abs().max_element() < 1e-4);
        let per_em = StemDarkening::at_ppem(10.0).amount / 10.0;
        assert!(
            (per_em - StemDarkening::bound(1.0).amount)
                .abs()
                .max_element()
                < 1e-7
        );
    }
}
