//! The lowering every shape and chrome share: a [`Brush`] or curve ramp interned into the window's [`RecordStore`] gradient pool, and [`Chrome`] chrome lowered to its [`ChromeRow`]. Each shape lowers itself in its [`Lower`] impl and calls in here for what it interns.
//!
//! Entry points: [`Shapes::add`](crate::shape::shapes::Shapes::add) for shapes; `Tree::open_node` calls [`chrome`].
//!
//! [`Lower`]: crate::shape::Lower

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::domain::is_invisible;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::brush::gradient::{Gradient, GradientGeometry};
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::line_style::LineCap;
use crate::primitives::paint::paint::Paint;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::paint::arc_geometry::ArcGeometry;
use crate::shape::paint::chrome_row::ChromeRow;
use crate::shape::paint::curve_ramp::CurveRamp;
use crate::shape::paint::lowered_border::LoweredBorder;
use crate::shape::paint::lowered_brush::LoweredBrush;
use crate::shape::paint::lowered_material::{LoweredMaterial, MaterialRead};
use crate::shape::paint::lowered_shadow::LoweredShadow;
use crate::shape::paint::lowered_stroke::LoweredStroke;
use crate::shape::paint::quad_shape::{BoxGeometry, QuadShape};
use crate::shape::record::ShapeRecord;
use crate::shape::record_store::RecordStore;
use crate::shape::record_store::recorded_gradient::RecordedGradient;
use crate::shape::sealed::Resolve;
use std::hash;
use std::hash::{Hash as _, Hasher as _};

/// Stable content hash for a gradient kind or curve ramp: the fill kind's word, which tells the kinds apart, then the value's `Hash`, captured at lowering so downstream cache keys stay context-free.
#[inline]
fn gradient_hash<G: hash::Hash>(kind: FillKind, g: &G) -> ContentHash {
    ContentHash::of((kind.0, g))
}

fn gradient_brush<G: GradientGeometry>(
    store: &mut RecordStore,
    kind: FillKind,
    gradient: &Gradient<G>,
) -> LoweredBrush {
    let hash = gradient_hash(kind, gradient);
    let id = store.gradients.intern(
        hash,
        RecordedGradient {
            axis: gradient.axis(),
            kind,
            ramp: gradient.ramp,
        },
    );
    LoweredBrush::Gradient { id, hash }
}

/// Lower a `Brush`: `Solid` stays inline; gradients go into the store and return a `LoweredBrush::Gradient` with index and content hash.
pub(crate) fn brush(store: &mut RecordStore, b: &Brush) -> LoweredBrush {
    // A gradient's geometry vanishes behind a `GradientId`, so the no-op decision is made before interning, by both callers.
    debug_assert!(
        !b.has_nan(),
        "NaN gradient geometry reached lowering: {b:?}"
    );
    match b {
        Brush::Solid(color) => LoweredBrush::Solid(color.to_f16()),
        Brush::Linear(g) => gradient_brush(store, FillKind::linear(g.spread), g),
        Brush::Radial(g) => gradient_brush(store, FillKind::radial(g.spread), g),
        Brush::Conic(g) => gradient_brush(store, FillKind::conic(g.spread), g),
    }
}

/// Intern a curve's color ramp into the gradient pool [`brush`] fills, as a row with no axis.
pub(crate) fn ramp(store: &mut RecordStore, ramp: &ColorRamp) -> CurveRamp {
    let hash = gradient_hash(FillKind::RAMP, ramp);
    let id = store.gradients.intern(
        hash,
        RecordedGradient {
            axis: FillAxis::ZERO,
            kind: FillKind::RAMP,
            ramp: *ramp,
        },
    );
    CurveRamp::Interned { id, hash }
}

/// A [`Paint`] lowered for a box, a node's, a rect's or a circle's: its fill interned, its border
/// at its inset, its shadow and its material.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LoweredPaint {
    pub(crate) fill: LoweredBrush,
    pub(crate) border: LoweredBorder,
    pub(crate) shadow: LoweredShadow,
    pub(crate) material: Option<LoweredMaterial>,
}

/// Lower `paint`, whose material reads `material` this record (`None` where it paints none): the
/// one lowering every box shares, sharing gradient lowering with every shape, so all share one
/// pool.
pub(crate) fn lower_paint(
    store: &mut RecordStore,
    paint: &Paint,
    material: Option<MaterialRead>,
) -> LoweredPaint {
    debug_assert!(
        !paint.has_nan(),
        "a Paint reaches lowering validated, by `Widget::record` or `Ui::add_shape`: {paint:?}",
    );
    let material = paint
        .material
        .as_ref()
        .zip(material)
        .map(|(authored, read)| LoweredMaterial::lower(store, authored, read));
    let fill = if paint.is_noop() {
        brush(store, &Brush::TRANSPARENT)
    } else {
        brush(store, &paint.fill)
    };
    LoweredPaint {
        fill,
        border: LoweredBorder {
            width: paint.border_inset(),
            ..LoweredBorder::lower(paint.border)
        },
        shadow: LoweredShadow::lower(paint.shadow),
        material,
    }
}

/// The record of a box laid out in `local_rect`, the owner's arranged rect where `None`, with
/// `outline`, covering `coverage`, in `paint`, its material resolved in `resolve`; `None` where it
/// paints nothing.
pub(crate) fn paint_box(
    resolve: Resolve<'_>,
    store: &mut RecordStore,
    geometry: BoxGeometry,
    paint: &Paint,
) -> Option<ShapeRecord> {
    let read = paint
        .material
        .as_ref()
        .and_then(|material| MaterialRead::resolve(material, resolve.effects, resolve.reductions));
    if geometry.local_rect.is_some_and(Rect::is_paint_empty)
        || (paint.fill.is_noop()
            && paint.border.is_noop()
            && paint.shadow.is_noop()
            && read.is_none())
    {
        return None;
    }
    let LoweredPaint {
        fill,
        border,
        shadow,
        material,
    } = lower_paint(store, paint, read);
    Some(ShapeRecord::Quad(QuadShape::Box {
        geometry,
        fill,
        border,
        shadow,
        material,
    }))
}

/// The record of `arc` drawn with `stroke`; `None` where it paints nothing: an invisible stroke,
/// or no sweep under a butt cap, which has no extent. An arc in a rect of no size keeps its record,
/// which paints its cap as a dot where a round or square cap gives it one ([`ArcGeometry::place`]).
pub(crate) fn arc(
    store: &mut RecordStore,
    arc: ArcGeometry,
    stroke: Stroke,
) -> Option<ShapeRecord> {
    if stroke.is_noop() || (stroke.cap == LineCap::Butt && is_invisible(arc.sweep.abs())) {
        return None;
    }
    Some(ShapeRecord::Arc {
        cap: stroke.cap,
        geometry: arc,
        stroke: LoweredStroke::lower(stroke),
        ramp: stroke
            .ramp
            .map_or(CurveRamp::None, |stroke_ramp| ramp(store, &stroke_ramp)),
    })
}

/// Lower a node's `Chrome` to its `ChromeRow`, with the focus `ring` over it.
pub(crate) fn chrome(
    store: &mut RecordStore,
    chrome: &Chrome,
    ring: Border,
    material: Option<MaterialRead>,
) -> ChromeRow {
    let LoweredPaint {
        fill,
        border,
        shadow,
        material,
    } = lower_paint(store, &chrome.paint, material);
    let corners = chrome.corners;
    let ring = LoweredBorder::lower(ring);
    let has_ring = !ring.is_noop();
    // Fed as whole words from registers: a struct packed for one `write` is stored field by field and read back wider, a load the CPU can't forward.
    let brush = fill.hash_parts();
    let mut h = Hasher::new();
    h.write_u64(brush.payload);
    h.write_u64(corners.as_u64());
    border.hash_into(&mut h);
    ring.hash_into(&mut h);
    shadow.hash(&mut h);
    material.hash(&mut h);
    h.write_u8(brush.tag);
    let hash = ContentHash::finish(&h);
    ChromeRow {
        fill,
        border,
        corners,
        shadow,
        material,
        hash,
        ring: has_ring,
    }
}

#[cfg(test)]
mod tests;
