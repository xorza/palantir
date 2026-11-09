//! The polyline builder and its per-vertex or per-segment color source.

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::primitives::geometry::fit::Fit;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::rect::aabb::Aabb;
use crate::primitives::math::domain;
use crate::primitives::math::float_hash::{self, FloatHash};
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::paint::point_placement::PointPlacement;
use crate::shape::record::{ColorMode, ShapeRecord};
use crate::shape::record_store::RecordStore;
use crate::shape::sealed::{self, Resolve};
use glam::Vec2;
use std::hash::Hasher as _;
use std::slice;

/// A polyline through its points, logical px from its rect's top-left: the owner's arranged rect
/// or [`Self::at`] a rect of its own, its points fit into it by [`Self::fit`]. One color for every
/// vertex or one per point or segment multiplies its [`Stroke`]'s color as vertex data, as a
/// mesh's colors are. A `Stroke` draws it, centered on its points
/// ([`Ui::add_shape`](crate::Ui::add_shape)).
#[derive(Clone, Debug)]
#[must_use]
pub struct PolylineShape<'a> {
    pub(crate) points: &'a [Vec2],
    pub(crate) colors: PolylineColors<'a>,
    /// The points' AABB, folded once at construction so [`Shape::polyline`](crate::widget::Shape::polyline) checks every point in one test.
    pub(crate) bbox: Rect,
    pub(crate) placement: PointPlacement,
}

impl<'a> PolylineShape<'a> {
    pub(super) fn new(points: &'a [Vec2]) -> Self {
        Self {
            points,
            colors: PolylineColors::Single,
            // Under the AABB NaN contract, so a NaN point lands in the bbox.
            bbox: Aabb::of(points),
            placement: PointPlacement::NATURAL,
        }
    }

    /// Lay out in `rect`, in owner-relative coords, instead of the owner's arranged rect.
    ///
    /// # Panics
    ///
    /// Panics unless every component of `rect` is an [offset](crate::widget::domain::offset).
    #[track_caller]
    pub const fn at(mut self, rect: Rect) -> Self {
        rect.validate();
        self.placement.local_rect = Some(rect);
        self
    }

    /// Fit the points into the rect by `fit`; [`Fit::Natural`], as given, by default.
    pub const fn fit(mut self, fit: Fit) -> Self {
        self.placement.fit = fit;
        self
    }

    /// One color per point, lerped between points; multiplies the stroke color per channel.
    ///
    /// # Panics
    ///
    /// Panics unless `colors` has one entry per point and every color is a
    /// [color](crate::widget::domain::color); its product with the stroke's color is checked
    /// where [`Ui::add_shape`](crate::Ui::add_shape) joins them.
    #[track_caller]
    pub const fn per_point(mut self, colors: &'a [RgbaF32]) -> Self {
        assert!(
            colors.len() == self.points.len(),
            "per-point polyline colors must number the points",
        );
        check_colors(colors);
        self.colors = PolylineColors::PerPoint(colors);
        self
    }

    /// One color per segment, each a solid block; multiplies the stroke color.
    ///
    /// # Panics
    ///
    /// Panics unless `colors` has one entry per segment, one fewer than the points, and every
    /// color is a [color](crate::widget::domain::color); its product with the stroke's color is
    /// checked where [`Ui::add_shape`](crate::Ui::add_shape) joins them.
    #[track_caller]
    pub const fn per_segment(mut self, colors: &'a [RgbaF32]) -> Self {
        assert!(
            colors.len() == self.points.len().saturating_sub(1),
            "per-segment polyline colors must number the segments, one fewer than the points",
        );
        check_colors(colors);
        self.colors = PolylineColors::PerSegment(colors);
        self
    }
}

/// Where a polyline's color varies, on top of its stroke color.
#[derive(Clone, Copy, Debug)]
pub(crate) enum PolylineColors<'a> {
    Single,
    /// One multiplier per input point; the GPU lerps between them.
    PerPoint(&'a [RgbaF32]),
    /// One multiplier per segment; no color bleeds across a joint.
    PerSegment(&'a [RgbaF32]),
}

pub(crate) const TINT_RULE: &str =
    "a polyline color times its stroke color must have channels that round to within ±65504 in f16";

/// Each of `colors` a color, and its product with `stroke`, which lowering packs into f16 lanes,
/// one too: two colors each in range can multiply past the largest f16. Refused rather than
/// saturated, as the drawn color would then differ from the product asked for.
#[track_caller]
const fn check_colors(colors: &[RgbaF32]) {
    let mut i = 0;
    while i < colors.len() {
        let _ = domain::color(colors[i]);
        i += 1;
    }
}

/// Panic unless each of `colors` times `stroke` is a color, as the record stores the product.
#[track_caller]
const fn check_tints(colors: &[RgbaF32], stroke: RgbaF32) {
    let mut i = 0;
    while i < colors.len() {
        assert!(domain::is_color(colors[i].tinted(stroke)), "{}", TINT_RULE);
        i += 1;
    }
}

impl PolylineColors<'_> {
    /// Whether the colors fit `points_len` points, as [`PolylineShape::per_point`] and
    /// [`PolylineShape::per_segment`] assert.
    const fn matches(&self, points_len: usize) -> bool {
        match self {
            PolylineColors::Single => true,
            PolylineColors::PerPoint(colors) => colors.len() == points_len,
            PolylineColors::PerSegment(colors) => colors.len() == points_len.saturating_sub(1),
        }
    }
}

/// A polyline drawn with a [`Stroke`], its colors per point or per segment times the stroke's.
impl sealed::LowerShape for (PolylineShape<'_>, Stroke) {
    /// # Panics
    ///
    /// Unless the stroke holds its kinds and draws no ramp, and each vertex color times the
    /// stroke's color is a color.
    #[track_caller]
    fn validate(&self) {
        let (shape, stroke) = self;
        stroke.validate();
        assert!(stroke.ramp.is_none(), "a polyline draws no ramp");
        match shape.colors {
            PolylineColors::Single => {}
            PolylineColors::PerPoint(colors) | PolylineColors::PerSegment(colors) => {
                check_tints(colors, stroke.color);
            }
        }
    }

    fn lower(self, _resolve: Resolve<'_>, store: &mut RecordStore) -> Option<ShapeRecord> {
        let (
            PolylineShape {
                points,
                colors,
                bbox,
                placement,
            },
            stroke,
        ) = self;
        let Stroke {
            width,
            color,
            ramp: _,
            cap,
            join,
        } = stroke;
        let invisible = match colors {
            PolylineColors::Single => false,
            PolylineColors::PerPoint(colors) | PolylineColors::PerSegment(colors) => {
                colors.iter().all(|color| color.is_noop())
            }
        };
        if stroke.is_noop() || points.len() < 2 || invisible {
            return None;
        }
        let (mode, color_slice): (ColorMode, &[RgbaF32]) = match &colors {
            PolylineColors::Single => (ColorMode::Single, slice::from_ref(&RgbaF32::WHITE)),
            PolylineColors::PerPoint(cs) => (ColorMode::PerPoint, cs),
            PolylineColors::PerSegment(cs) => (ColorMode::PerSegment, cs),
        };

        // Dropped above under two points, and the color setters checked the count.
        debug_assert!(
            points.len() >= 2,
            "polyline with < 2 points reached lowering"
        );
        debug_assert!(colors.matches(points.len()));
        let staged = store.stage_polyline(points, color_slice, color);
        let lowered_colors = &store.polyline_colors[staged.colors.range()];

        let mut h = Hasher::new();
        for &point in points {
            point.hash_visual(&mut h);
        }
        h.pod_slice(lowered_colors);
        let style = (u64::from(float_hash::canon_bits(width)) << 24)
            | ((mode as u64) << 16)
            | ((cap as u64) << 8)
            | (join as u64);
        h.write_u64(style);
        let content_hash = ContentHash::finish(&h);

        Some(ShapeRecord::Polyline {
            width,
            color_mode: mode,
            cap,
            join,
            points: staged.points,
            colors: staged.colors,
            bbox,
            placement,
            content_hash,
        })
    }
}
