//! [`CoverageRaster`]: a glyph's outline filled to a coverage mask, each pixel the area the outline
//! covers in it, as font-rs and swash's zeno compute it: every flattened edge adds its signed
//! area to the cells it crosses, and a running sum over each row turns those into coverage
//! ([Levien, "The Rust rasterizer"](https://medium.com/@raphlinus/inside-the-fastest-font-renderer-in-the-world-75ae5270c445)).
//! Exact area, not samples: a 4×4 supersampler, as tiny-skia's, gives 17 levels, too few for the
//! edge of small text.

use crate::primitives::math::num::unit_to_u8;
use crate::text::cosmic::glyph_outline::{GlyphOutline, Verb};
use glam::Vec2;
use std::ops::Range;

/// The most a chord of a flattened curve strays from the curve, in pixels. A curve whose second
/// derivative is at most `B''` over its parameter strays by at most `B'' h² / 8` over a chord of
/// parameter length `h`, so `n = ⌈√(B'' / 8 TOL)⌉` uniform chords hold to it. A thirty-second of
/// a pixel draws a quarter circle of radius 8 in ten chords, which lose about a fifth of a pixel
/// of its area; font-rs's count of `1 + ⌊(3 |Δ²|²)^¼⌋` drew it in three and lost 2.2 px.
const FLATTEN_TOL: f32 = 1.0 / 32.0;

/// The most segments one curve flattens to, so a curve of absurd size from a malformed font stays
/// bounded; a curve that wants more draws coarser.
const MAX_SEGMENTS: usize = 256;

/// The longest edge of a mask in pixels; a glyph past it draws nothing, as no atlas page holds it.
pub(super) const MAX_MASK_EDGE: u32 = 8192;

/// Where a mask lies against the pen and how big it is: `left` and `top` from the pen to its
/// top-left pixel, `x` right and `y` up, as the atlas's bearing reads them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Placement {
    pub(super) left: i32,
    pub(super) top: i32,
    pub(super) width: u32,
    pub(super) height: u32,
}

impl Placement {
    pub(super) const EMPTY: Self = Self {
        left: 0,
        top: 0,
        width: 0,
        height: 0,
    };

    pub(super) const fn pixels(self) -> usize {
        self.width as usize * self.height as usize
    }
}

/// One straight edge of a flattened outline.
#[derive(Clone, Copy, Debug)]
struct Line {
    from: Vec2,
    to: Vec2,
}

/// The flattened edges of the outlines added since [`Self::begin`] and the area buffer they fill,
/// kept between glyphs so a glyph allocates nothing once both have grown.
#[derive(Debug, Default)]
pub(super) struct CoverageRaster {
    lines: Vec<Line>,
    min: Vec2,
    max: Vec2,
    /// Whether every point added was finite; a malformed font can draw one that is not.
    finite: bool,
    acc: Vec<f32>,
}

impl CoverageRaster {
    /// Start a glyph: no edges, no bounds.
    pub(super) fn begin(&mut self) {
        self.lines.clear();
        self.min = Vec2::INFINITY;
        self.max = Vec2::NEG_INFINITY;
        self.finite = true;
    }

    /// Flatten `outline` moved by `offset` and keep its edges, each contour closed; the range of
    /// [`Self::fill`]'s edges it added.
    pub(super) fn add(&mut self, outline: &GlyphOutline, offset: Vec2) -> Range<usize> {
        let first = self.lines.len();
        let mut points = outline.points().iter().map(|&p| p + offset);
        let mut point = || points.next().expect("each verb has its points");
        let mut start = Vec2::ZERO;
        let mut cur = Vec2::ZERO;
        for &verb in outline.verbs() {
            match verb {
                Verb::MoveTo => {
                    self.line(cur, start);
                    start = point();
                    cur = start;
                }
                Verb::LineTo => {
                    let to = point();
                    self.line(cur, to);
                    cur = to;
                }
                Verb::QuadTo => {
                    let (c, to) = (point(), point());
                    self.quad(cur, c, to);
                    cur = to;
                }
                Verb::CurveTo => {
                    let (c0, c1, to) = (point(), point(), point());
                    self.cubic([cur, c0, c1, to]);
                    cur = to;
                }
                Verb::Close => {
                    self.line(cur, start);
                    cur = start;
                }
            }
        }
        self.line(cur, start);
        first..self.lines.len()
    }

    /// The pixel grid that holds every edge added since [`Self::begin`]: from the floor of their
    /// least corner to the ceiling of their greatest. [`Placement::EMPTY`] for none; `None` for
    /// edges past [`MAX_MASK_EDGE`] or at no finite place, which draw nothing.
    #[expect(
        clippy::cast_sign_loss,
        reason = "a size is the greatest corner's ceiling less the least corner's floor"
    )]
    pub(super) fn placement(&self) -> Option<Placement> {
        if !self.finite {
            return None;
        }
        if self.lines.is_empty() {
            return Some(Placement::EMPTY);
        }
        let min = self.min.floor();
        let max = self.max.ceil();
        let size = max - min;
        if size.max_element() > MAX_MASK_EDGE as f32 {
            return None;
        }
        Some(Placement {
            left: min.x as i32,
            top: max.y as i32,
            width: size.x as u32,
            height: size.y as u32,
        })
    }

    /// Fill the edges `lines` into `mask` over `placement`'s grid, a byte a pixel, rows top
    /// down: each pixel's covered area, its winding counted as nonzero and capped at full.
    pub(super) fn fill(&mut self, lines: Range<usize>, placement: Placement, mask: &mut Vec<u8>) {
        let width = placement.width as usize;
        let height = placement.height as usize;
        mask.clear();
        mask.resize(width * height, 0);
        if width == 0 || height == 0 {
            return;
        }
        // Two spare cells: an edge on the right border spills its remainder past the row's last
        // cell, into the next row's first or two, which the running sum then carries back to
        // zero; the last row spills past the grid.
        self.acc.clear();
        self.acc.resize(width * height + 2, 0.0);
        let origin = Vec2::new(placement.left as f32, placement.top as f32);
        let grid = |p: Vec2| Vec2::new(p.x - origin.x, origin.y - p.y);
        for line in &self.lines[lines] {
            accumulate(&mut self.acc, width, height, grid(line.from), grid(line.to));
        }
        let mut sum = 0.0;
        for (coverage, &area) in mask.iter_mut().zip(&self.acc) {
            sum += area;
            *coverage = unit_to_u8(sum.abs().min(1.0));
        }
    }

    fn line(&mut self, from: Vec2, to: Vec2) {
        if !(from.is_finite() && to.is_finite()) {
            self.finite = false;
            return;
        }
        if from == to {
            return;
        }
        self.min = self.min.min(from).min(to);
        self.max = self.max.max(from).max(to);
        self.lines.push(Line { from, to });
    }

    fn quad(&mut self, p0: Vec2, p1: Vec2, p2: Vec2) {
        // A quadratic's second derivative is `2 (p0 − 2 p1 + p2)` throughout.
        let segments = segments(2.0 * (p0 - 2.0 * p1 + p2).length());
        let mut prev = p0;
        for i in 1..segments {
            let t = i as f32 / segments as f32;
            let at = p0.lerp(p1, t).lerp(p1.lerp(p2, t), t);
            self.line(prev, at);
            prev = at;
        }
        self.line(prev, p2);
    }

    fn cubic(&mut self, [p0, p1, p2, p3]: [Vec2; 4]) {
        // A cubic's second derivative runs between `6 (p0 − 2 p1 + p2)` and `6 (p1 − 2 p2 + p3)`.
        let bend = f32::max((p0 - 2.0 * p1 + p2).length(), (p1 - 2.0 * p2 + p3).length());
        let segments = segments(6.0 * bend);
        let mut prev = p0;
        for i in 1..segments {
            let t = i as f32 / segments as f32;
            let (a, b, c) = (p0.lerp(p1, t), p1.lerp(p2, t), p2.lerp(p3, t));
            let at = a.lerp(b, t).lerp(b.lerp(c, t), t);
            self.line(prev, at);
            prev = at;
        }
        self.line(prev, p3);
    }
}

/// How many chords a curve whose second derivative reaches `second_derivative` takes to hold to
/// [`FLATTEN_TOL`], from one to [`MAX_SEGMENTS`].
#[expect(
    clippy::cast_sign_loss,
    reason = "the root of a non-negative ratio, or NaN, which the saturating cast takes to zero"
)]
fn segments(second_derivative: f32) -> usize {
    let count = (second_derivative / (8.0 * FLATTEN_TOL)).sqrt().ceil();
    if count.is_finite() {
        (count as usize).clamp(1, MAX_SEGMENTS)
    } else {
        MAX_SEGMENTS
    }
}

/// Add the signed area the edge `p0 → p1` covers to each cell of a `width × height` grid, y down,
/// as font-rs's `draw_line` does: each row the edge crosses splits its height `dy` between the
/// cells under it by how much of each it leaves to its right; the sign is the edge's direction.
#[expect(
    clippy::cast_sign_loss,
    reason = "every coordinate is clamped into the grid, at zero or more, before the cast"
)]
fn accumulate(acc: &mut [f32], width: usize, height: usize, p0: Vec2, p1: Vec2) {
    if p0.y == p1.y {
        return;
    }
    let (dir, p0, p1) = if p0.y < p1.y {
        (1.0, p0, p1)
    } else {
        (-1.0, p1, p0)
    };
    debug_assert!(p0.y >= 0.0, "an edge above the grid: {p0}");
    let right = width as f32;
    let dxdy = (p1.x - p0.x) / (p1.y - p0.y);
    let mut x = p0.x.clamp(0.0, right);
    let first_row = p0.y as usize;
    let end_row = (p1.y.ceil() as usize).min(height);
    for row in first_row..end_row {
        let start = row * width;
        let dy = ((row + 1) as f32).min(p1.y) - (row as f32).max(p0.y);
        // Clamped: a step's rounding may leave the grid by an ulp at its border.
        let x_next = (x + dxdy * dy).clamp(0.0, right);
        let d = dy * dir;
        let (x0, x1) = if x < x_next { (x, x_next) } else { (x_next, x) };
        let x0_floor = x0.floor();
        let x0_cell = x0_floor as usize;
        let x1_ceil = x1.ceil();
        let x1_cell = x1_ceil as usize;
        if x1_cell <= x0_cell + 1 {
            // Within one cell: the part right of the edge's midpoint stays, the rest spills on.
            let x_mid = x.midpoint(x_next) - x0_floor;
            acc[start + x0_cell] += d - d * x_mid;
            acc[start + x0_cell + 1] += d * x_mid;
        } else {
            let s = (x1 - x0).recip();
            let x0_frac = x0 - x0_floor;
            let a0 = 0.5 * s * (1.0 - x0_frac) * (1.0 - x0_frac);
            let x1_frac = x1 - x1_ceil + 1.0;
            let a_last = 0.5 * s * x1_frac * x1_frac;
            acc[start + x0_cell] += d * a0;
            if x1_cell == x0_cell + 2 {
                acc[start + x0_cell + 1] += d * (1.0 - a0 - a_last);
            } else {
                let a1 = s * (1.5 - x0_frac);
                acc[start + x0_cell + 1] += d * (a1 - a0);
                for cell in x0_cell + 2..x1_cell - 1 {
                    acc[start + cell] += d * s;
                }
                let a2 = a1 + (x1_cell - x0_cell - 3) as f32 * s;
                acc[start + x1_cell - 1] += d * (1.0 - a2 - a_last);
            }
            acc[start + x1_cell] += d * a_last;
        }
        x = x_next;
    }
}

#[cfg(test)]
mod tests {
    use crate::text::cosmic::coverage_raster::{CoverageRaster, MAX_MASK_EDGE, Placement};
    use crate::text::cosmic::glyph_outline::GlyphOutline;
    use glam::Vec2;
    use skrifa::outline::OutlinePen as _;

    /// A closed polygon through `points`, y up.
    fn polygon(points: &[[f32; 2]]) -> GlyphOutline {
        let mut outline = GlyphOutline::default();
        let [x, y] = points[0];
        outline.move_to(x, y);
        for &[x, y] in &points[1..] {
            outline.line_to(x, y);
        }
        outline.close();
        outline
    }

    /// The mask of `outline` moved by `offset`, with its placement.
    fn fill(outline: &GlyphOutline, offset: Vec2) -> (Placement, Vec<u8>) {
        let mut raster = CoverageRaster::default();
        raster.begin();
        let lines = raster.add(outline, offset);
        let placement = raster.placement().expect("a finite outline");
        let mut mask = Vec::new();
        raster.fill(lines, placement, &mut mask);
        (placement, mask)
    }

    /// A square on whole pixels covers each of them fully and nothing else; its placement puts the
    /// top-left pixel at its left edge and its top, y up.
    #[test]
    fn a_square_on_whole_pixels_covers_them_fully() {
        let (placement, mask) = fill(
            &polygon(&[[1.0, 1.0], [3.0, 1.0], [3.0, 4.0], [1.0, 4.0]]),
            Vec2::ZERO,
        );
        assert_eq!(
            placement,
            Placement {
                left: 1,
                top: 4,
                width: 2,
                height: 3,
            }
        );
        assert_eq!(mask, [255; 6]);
    }

    /// A half-pixel offset spreads a one-pixel square over four pixels, a quarter each: 0.25 · 255
    /// rounds to 64. Either winding fills alike.
    #[test]
    fn a_half_pixel_offset_shares_a_pixel_four_ways_in_either_winding() {
        let square = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let (placement, mask) = fill(&polygon(&square), Vec2::splat(0.5));
        assert_eq!(
            placement,
            Placement {
                left: 0,
                top: 2,
                width: 2,
                height: 2,
            }
        );
        assert_eq!(mask, [64; 4]);
        let mut reversed = square;
        reversed.reverse();
        assert_eq!(fill(&polygon(&reversed), Vec2::splat(0.5)).1, [64; 4]);
    }

    /// A right triangle over a 2×2 grid, its hypotenuse the diagonal from bottom-left to top-right:
    /// the pixels it cuts hold half each (128), the one under it all (255), the one over it none.
    /// Rows run top down.
    #[test]
    fn a_diagonal_edge_covers_half_of_each_pixel_it_cuts() {
        let (_, mask) = fill(&polygon(&[[0.0, 0.0], [2.0, 0.0], [2.0, 2.0]]), Vec2::ZERO);
        assert_eq!(mask, [0, 128, 128, 255]);
    }

    /// Two overlapping squares of one winding count as one fill, capped at full; a hole wound the
    /// other way inside a square clears it.
    #[test]
    fn overlap_caps_at_full_and_a_reversed_contour_cuts_a_hole() {
        let mut outline = polygon(&[[0.0, 0.0], [2.0, 0.0], [2.0, 1.0], [0.0, 1.0]]);
        outline.move_to(1.0, 0.0);
        outline.line_to(3.0, 0.0);
        outline.line_to(3.0, 1.0);
        outline.line_to(1.0, 1.0);
        outline.close();
        assert_eq!(fill(&outline, Vec2::ZERO).1, [255; 3]);

        let mut ring = polygon(&[[0.0, 0.0], [3.0, 0.0], [3.0, 3.0], [0.0, 3.0]]);
        ring.move_to(1.0, 1.0);
        ring.line_to(1.0, 2.0);
        ring.line_to(2.0, 2.0);
        ring.line_to(2.0, 1.0);
        ring.close();
        let (_, mask) = fill(&ring, Vec2::ZERO);
        assert_eq!(mask, [255, 255, 255, 255, 0, 255, 255, 255, 255]);
    }

    /// A quarter disc of radius 8 drawn by a cubic: it flattens close to the circle, so the
    /// coverage sums to its area, π · 64 / 4 ≈ 50.27 px, within a quarter pixel; a parabola's
    /// segment to its own; an open contour closes itself.
    #[test]
    fn curves_flatten_to_their_area_and_an_open_contour_closes() {
        // The cubic's control points at k = 0.5523 · r trace a quarter circle within 0.03 %.
        let k = 0.552_284_8 * 8.0;
        let mut outline = GlyphOutline::default();
        outline.move_to(0.0, 0.0);
        outline.line_to(8.0, 0.0);
        outline.curve_to(8.0, k, k, 8.0, 0.0, 8.0);
        let (_, mask) = fill(&outline, Vec2::ZERO);
        let area: f32 = mask.iter().map(|&c| f32::from(c) / 255.0).sum();
        let quarter_disc = std::f32::consts::PI * 64.0 / 4.0;
        assert!(
            (area - quarter_disc).abs() < 0.25,
            "{area} against {quarter_disc}"
        );

        let mut quad = GlyphOutline::default();
        quad.move_to(0.0, 0.0);
        quad.quad_to(4.0, 8.0, 8.0, 0.0);
        quad.close();
        // A parabola's segment holds 2/3 of its bounding triangle's base · height: 2/3 · 8 · 4.
        let area: f32 = fill(&quad, Vec2::ZERO)
            .1
            .iter()
            .map(|&c| f32::from(c) / 255.0)
            .sum();
        assert!((area - 64.0 / 3.0).abs() < 0.25, "{area}");
    }

    /// No edges give an empty placement and mask; edges past the largest mask, or at a place no
    /// float names, give none.
    #[test]
    fn no_edges_place_nothing_and_unplaceable_edges_draw_nothing() {
        let (placement, mask) = fill(&GlyphOutline::default(), Vec2::ZERO);
        assert_eq!(placement, Placement::EMPTY);
        assert!(mask.is_empty());

        let edge = MAX_MASK_EDGE as f32 + 1.0;
        let mut raster = CoverageRaster::default();
        raster.begin();
        raster.add(
            &polygon(&[[0.0, 0.0], [edge, 0.0], [edge, 1.0]]),
            Vec2::ZERO,
        );
        assert_eq!(raster.placement(), None);
        raster.begin();
        raster.add(
            &polygon(&[[0.0, 0.0], [f32::NAN, 0.0], [1.0, 1.0]]),
            Vec2::ZERO,
        );
        assert_eq!(raster.placement(), None);
    }
}
