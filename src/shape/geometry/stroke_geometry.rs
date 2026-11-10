//! [`StrokeGeometry`]: the path a stroke record traces, a cubic, an arc or a polyline, which
//! places where it is drawn.

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::common::span::Span;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::nan::NanCheck;
use crate::shape::geometry::arc_geometry::ArcGeometry;
use crate::shape::geometry::bezier_geometry::BezierGeometry;
use crate::shape::geometry::point_placement::PointPlacement;
use std::hash::{Hash, Hasher as _};

/// The path a stroke traces, owner-relative; the stroke that paints it rides beside it in the
/// record. All three draw on the one curve pipeline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum StrokeGeometry {
    Bezier(BezierGeometry),
    Arc(ArcGeometry),
    Polyline(PolylineGeometry),
}

/// A polyline's points and their colors, in the record store, as a mesh's vertices carry theirs:
/// the colors multiply the stroke's color where the polyline is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PolylineGeometry {
    /// The points as authored, two or more, in the store's polyline points.
    pub(crate) points: Span,
    pub(crate) colors: PolylineColorSpan,
    /// The points' AABB as authored, the extent `placement` fits into its rect.
    pub(crate) bbox: Rect,
    pub(crate) placement: PointPlacement,
    /// The points and colors hashed where they were stored, as the record's hash cannot read the
    /// store.
    pub(crate) content_hash: ContentHash,
}

/// A polyline's colors in the store's polyline colors: none, the stroke's color alone; one a
/// point; or one a segment, one fewer than the points.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PolylineColorSpan {
    #[default]
    Single,
    PerPoint(Span),
    PerSegment(Span),
}

impl StrokeGeometry {
    /// Feed the geometry into a record's hash.
    pub(crate) fn hash_into(self, h: &mut Hasher) {
        match self {
            Self::Bezier(geometry) => {
                h.write_u8(0);
                geometry.hash_into(h);
            }
            Self::Arc(geometry) => {
                h.write_u8(1);
                geometry.hash_into(h);
            }
            // The spans and the bbox are frame-local, and the hash covers what they hold.
            Self::Polyline(PolylineGeometry {
                content_hash,
                placement,
                points: _,
                colors: _,
                bbox: _,
            }) => {
                h.write_u8(2);
                content_hash.hash(h);
                placement.hash_into(h);
            }
        }
    }
}

impl NanCheck for StrokeGeometry {
    fn has_nan(&self) -> bool {
        match self {
            Self::Bezier(geometry) => geometry.has_nan(),
            Self::Arc(geometry) => geometry.has_nan(),
            Self::Polyline(geometry) => geometry.bbox.has_nan() || geometry.placement.has_nan(),
        }
    }
}
