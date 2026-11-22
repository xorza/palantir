//! [`GlyphOutline`]: one glyph's outline as skrifa draws it, y up, in pixels or font units.

use glam::Vec2;
use skrifa::outline::OutlinePen;

/// One drawing command of an outline; each takes its points in order from
/// [`GlyphOutline::points`]: a move or a line one, a quadratic two, a cubic three, a close none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Verb {
    MoveTo,
    LineTo,
    QuadTo,
    CurveTo,
    Close,
}

/// A glyph's contours as points and the verbs that draw through them, kept between glyphs so one
/// outline allocates nothing once its buffers have grown.
#[derive(Debug, Default)]
pub(super) struct GlyphOutline {
    points: Vec<Vec2>,
    verbs: Vec<Verb>,
}

/// An outline's verbs beside its points borrowed to move, which [`GlyphOutline::contours_mut`]
/// lends.
#[derive(Debug)]
pub(super) struct OutlineMut<'a> {
    pub(super) verbs: &'a [Verb],
    pub(super) points: &'a mut [Vec2],
}

/// The smallest box holding every point of an outline, control points included.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PointBounds {
    pub(super) min: Vec2,
    pub(super) max: Vec2,
}

impl GlyphOutline {
    pub(super) fn clear(&mut self) {
        self.points.clear();
        self.verbs.clear();
    }

    pub(super) fn points(&self) -> &[Vec2] {
        &self.points
    }

    pub(super) fn verbs(&self) -> &[Verb] {
        &self.verbs
    }

    /// The verbs to read and the points to move in place at once, as a dilation does.
    pub(super) fn contours_mut(&mut self) -> OutlineMut<'_> {
        OutlineMut {
            verbs: &self.verbs,
            points: &mut self.points,
        }
    }

    /// Lean the outline right by `tan` of its angle per unit up, as a synthetic italic leans:
    /// `x += tan * y`, the baseline fixed.
    pub(super) fn skew(&mut self, tan: f32) {
        for point in &mut self.points {
            point.x += tan * point.y;
        }
    }

    /// The box of every point, `None` for an outline with none.
    pub(super) fn bounds(&self) -> Option<PointBounds> {
        let (&first, rest) = self.points.split_first()?;
        let (min, max) = rest
            .iter()
            .fold((first, first), |(min, max), &p| (min.min(p), max.max(p)));
        Some(PointBounds { min, max })
    }

    fn push(&mut self, verb: Verb, points: &[Vec2]) {
        self.verbs.push(verb);
        self.points.extend_from_slice(points);
    }
}

impl OutlinePen for GlyphOutline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.push(Verb::MoveTo, &[Vec2::new(x, y)]);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.push(Verb::LineTo, &[Vec2::new(x, y)]);
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.push(Verb::QuadTo, &[Vec2::new(cx0, cy0), Vec2::new(x, y)]);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.push(
            Verb::CurveTo,
            &[Vec2::new(cx0, cy0), Vec2::new(cx1, cy1), Vec2::new(x, y)],
        );
    }

    fn close(&mut self) {
        self.verbs.push(Verb::Close);
    }
}

#[cfg(test)]
mod tests {
    use crate::text::cosmic::glyph_outline::{GlyphOutline, PointBounds, Verb};
    use glam::Vec2;
    use skrifa::outline::OutlinePen as _;

    /// The pen records each command with its points in order; the bounds take every point, the
    /// control points included; a skew moves each point by `tan · y`, the baseline still; a
    /// cleared outline has no bounds.
    #[test]
    fn the_pen_records_each_command_and_the_outline_skews_about_the_baseline() {
        let mut outline = GlyphOutline::default();
        outline.move_to(0.0, 0.0);
        outline.line_to(4.0, 0.0);
        outline.quad_to(6.0, 2.0, 4.0, 4.0);
        outline.curve_to(3.0, 5.0, 1.0, 5.0, 0.0, 4.0);
        outline.close();
        assert_eq!(
            outline.verbs(),
            [
                Verb::MoveTo,
                Verb::LineTo,
                Verb::QuadTo,
                Verb::CurveTo,
                Verb::Close
            ]
        );
        assert_eq!(outline.points().len(), 1 + 1 + 2 + 3);
        assert_eq!(
            outline.bounds(),
            Some(PointBounds {
                min: Vec2::new(0.0, 0.0),
                max: Vec2::new(6.0, 5.0),
            })
        );

        // tan = 0.5: (4, 0) stays, (4, 4) moves to (6, 4), (1, 5) to (3.5, 5).
        outline.skew(0.5);
        assert_eq!(outline.points()[1], Vec2::new(4.0, 0.0));
        assert_eq!(outline.points()[3], Vec2::new(6.0, 4.0));
        assert_eq!(outline.points()[5], Vec2::new(3.5, 5.0));

        outline.clear();
        assert_eq!(outline.bounds(), None);
        assert!(outline.verbs().is_empty());
    }
}
