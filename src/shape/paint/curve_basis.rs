//! The geometry of a lowered curve.

use glam::Vec2;

/// Which parametric basis a stroke traces — the half that actually
/// differs between a Bézier and an arc. Named for the shader's own
/// vocabulary: both lower to a `CurveInstance` on the one curve
/// pipeline, selected by its `kind` lane, so they are two bases of one
/// draw rather than two draws.
///
/// Lowered once, in [`crate::shape::lower`], and then carried
/// verbatim from [`ShapeRecord::Curve`] through `DrawCurvePayload` to
/// the composer — the tiers in between share the stroke's width, cap,
/// fill, and bbox handling and never re-split the two forms.
///
/// Both forms are owner-local. The composer folds in the owner origin
/// and the active transform before scaling to physical px.
///
/// [`ShapeRecord::Curve`]: crate::shape::record::ShapeRecord::Curve
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CurveBasis {
    /// Cubic Bézier control points. Quadratics promote to cubic and
    /// straight lines degenerate to one at lowering, so this is every
    /// non-circular stroke.
    Cubic {
        p0: Vec2,
        p1: Vec2,
        p2: Vec2,
        p3: Vec2,
    },
    /// Exact circle — no cubic approximation error, and ramp `t`
    /// tracks the sweep linearly. `a0`/`a1` are radians in the screen
    /// convention (0 = +x, y-down ⇒ increasing = clockwise), so
    /// `a1 < a0` is a negative sweep.
    Arc {
        center: Vec2,
        radius: f32,
        a0: f32,
        a1: f32,
    },
}

impl Default for CurveBasis {
    /// A degenerate cubic at the origin — the `Default` a
    /// `DrawCurvePayload` literal falls back to, never a real draw.
    fn default() -> Self {
        Self::Cubic {
            p0: Vec2::ZERO,
            p1: Vec2::ZERO,
            p2: Vec2::ZERO,
            p3: Vec2::ZERO,
        }
    }
}
