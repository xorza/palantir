//! [`Draw`]: a shape and the paint that covers it as one value, its types erased, as
//! [`RecordStore::record`](crate::shape::record_store::RecordStore::record) records it.
//!
//! # Decisions
//!
//! - **One record path, in order.** `Ui::add_shape` has the shape check its paint, makes one
//!   `Draw` of the two, and the record store records it: names resolved, a draw that paints
//!   nothing dropped, variable-length payloads copied into the frame's arena, and the fixed-size
//!   record returned for the tree to push. The pair impls only check and convert; lowering is one
//!   match, compiled once. Refused: a lowering that returns its payloads for a later write, which
//!   either owns them, an allocation per shape per frame, or borrows them and needs a second
//!   record type for the store to patch with indexes.

use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::paint::Paint;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::curve::CurveShape;
use crate::shape::effect_paint::ErasedEffectPaint;
use crate::shape::geometry::arc_geometry::ArcGeometry;
use crate::shape::geometry::closed_geometry::ClosedGeometry;
use crate::shape::icon_paint::IconPaint;
use crate::shape::image_paint::ImagePaint;
use crate::shape::mesh::MeshShape;
use crate::shape::polyline::PolylineShape;
use crate::shape::text::TextShape;

/// One pair [`Ui::add_shape`](crate::Ui::add_shape) takes, borrowed from its caller: the pipeline
/// that draws it, its geometry and its paint. Each pair's `DrawWith::draw` makes one, so the
/// record store lowers every pair through one match, compiled once.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Draw<'a> {
    /// A rect, a circle or a triangle, and the paint that covers it.
    Closed {
        geometry: ClosedGeometry,
        paint: ClosedPaintRef<'a>,
    },
    /// A line or a Bézier.
    Curve {
        shape: &'a CurveShape,
        stroke: &'a Stroke,
    },
    /// An arc, or a circle drawn as a closed one.
    Arc {
        geometry: ArcGeometry,
        stroke: &'a Stroke,
    },
    Polyline {
        shape: &'a PolylineShape<'a>,
        stroke: &'a Stroke,
    },
    Text {
        shape: &'a TextShape,
        color: RgbaF32,
    },
    Mesh {
        shape: &'a MeshShape<'a>,
        tint: RgbaF32,
    },
}

/// The paint of a [`Draw::Closed`], borrowed from its caller.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ClosedPaintRef<'a> {
    /// A box's or a triangle's fill, border and shadow, over the material it wears, if any.
    Fill {
        paint: &'a Paint,
        material: Option<&'a Material>,
    },
    Image(&'a ImagePaint),
    /// An icon, of a square-cornered rect alone.
    Icon(&'a IconPaint),
    /// An app's effect.
    Effect(ErasedEffectPaint<'a>),
}

impl Draw<'_> {
    /// Whether the draw reads a paint animation's time, so an animation that drives no channel
    /// still moves it: only an effect does.
    pub(crate) const fn reads_time(&self) -> bool {
        matches!(
            self,
            Self::Closed {
                paint: ClosedPaintRef::Effect(_),
                ..
            }
        )
    }
}
