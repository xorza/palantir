//! The lowering every shape and chrome share: a [`Brush`] or curve ramp interned into the window's [`RecordStore`] gradient pool, and [`Background`] chrome lowered to its [`ChromeRow`]. Each shape lowers itself in its [`Lower`] impl and calls in here for what it interns.
//!
//! Entry points: [`Shapes::add`](crate::shape::shapes::Shapes::add) for shapes; `Tree::open_node` calls [`background`].
//!
//! [`Lower`]: crate::shape::Lower

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::brush::gradient::{Gradient, GradientGeometry};
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::paint::stroke::Stroke;
use crate::scene::record_store::RecordStore;
use crate::scene::record_store::recorded_gradient::RecordedGradient;
use crate::shape::paint::chrome_row::ChromeRow;
use crate::shape::paint::curve_ramp::CurveRamp;
use crate::shape::paint::lowered_shadow::LoweredShadow;
use crate::shape::paint::shape_brush::ShapeBrush;
use crate::shape::paint::shape_stroke::ShapeStroke;
use std::hash;
use std::hash::{Hash as _, Hasher as _};

/// Stable content hash for a gradient kind or curve ramp: the fill kind's word, which tells the kinds apart, then the value's `Hash`, captured at lowering so downstream cache keys stay context-free.
#[inline]
fn gradient_hash<G: hash::Hash>(kind: FillKind, g: &G) -> u64 {
    let mut h = Hasher::new();
    h.write_u32(kind.0);
    g.hash(&mut h);
    h.finish()
}

fn gradient_brush<G: GradientGeometry>(
    store: &mut RecordStore,
    kind: FillKind,
    gradient: &Gradient<G>,
) -> ShapeBrush {
    let hash = gradient_hash(kind, gradient);
    let id = store.intern_gradient(
        hash,
        RecordedGradient {
            axis: gradient.axis(),
            kind,
            ramp: gradient.ramp,
        },
    );
    ShapeBrush::Gradient { id, hash }
}

/// Lower a `Brush`: `Solid` stays inline; gradients go into the store and return a `ShapeBrush::Gradient` with index and content hash.
pub(crate) fn brush(store: &mut RecordStore, b: &Brush) -> ShapeBrush {
    // A gradient's geometry vanishes behind a `GradientId`, so the no-op decision is made before interning, by both callers.
    debug_assert!(
        !b.has_nan(),
        "NaN gradient geometry reached lowering: {b:?}"
    );
    match b {
        Brush::Solid(color) => ShapeBrush::Solid((*color).into()),
        Brush::Linear(g) => gradient_brush(store, FillKind::linear(g.spread), g),
        Brush::Radial(g) => gradient_brush(store, FillKind::radial(g.spread), g),
        Brush::Conic(g) => gradient_brush(store, FillKind::conic(g.spread), g),
    }
}

/// Intern a curve's color ramp into the gradient pool [`brush`] fills, as a row with no axis.
pub(crate) fn ramp(store: &mut RecordStore, ramp: &ColorRamp) -> CurveRamp {
    let hash = gradient_hash(FillKind::RAMP, ramp);
    let id = store.intern_gradient(
        hash,
        RecordedGradient {
            axis: FillAxis::ZERO,
            kind: FillKind::RAMP,
            ramp: *ramp,
        },
    );
    CurveRamp::Interned { id, hash }
}

/// Lower a `Background` to a `ChromeRow`, sharing gradient lowering with shapes so chrome and shapes share one pool. Takes `bg` by reference: [`Background`] is deliberately not `Copy`.
pub(crate) fn background(store: &mut RecordStore, bg: &Background, ring: Stroke) -> ChromeRow {
    // Chrome's NaN gate, the second of two (the shape path's is `Shapes::add`), before lowering because `fill` interns its gradient.
    //
    // It sanitizes where the shape path drops: `chrome_table` keeps a row for `ClipMode::Rounded` even when the paint is no-op, so the stencil mask can read `corners`, and dropping it would leave the mask reading the NaN. Each field falls back to what its NaN meant; sanitizing before the hash keeps `ChromeRow.hash` honest.
    debug_assert!(
        !bg.has_nan(),
        "NaN in a Background — it degrades to no rounding and no paint: {bg:?}",
    );
    let fill_brush = if bg.fill.has_nan() || bg.is_noop() {
        &Brush::TRANSPARENT
    } else {
        &bg.fill
    };
    let fill = brush(store, fill_brush);
    let border = ShapeStroke {
        width: bg.border_inset(),
        ..ShapeStroke::from(if bg.border.has_nan() {
            Stroke::NONE
        } else {
            bg.border
        })
    };
    let corners = if bg.corners.has_nan() {
        Corners::ZERO
    } else {
        bg.corners
    };
    let shadow: LoweredShadow = if bg.shadow.has_nan() {
        Shadow::NONE.into()
    } else {
        bg.shadow.into()
    };
    let ring = ShapeStroke::from(ring);
    let has_ring = !ring.is_noop();
    // Fed as whole words from registers: a struct packed for one `write` is stored field by field and read back wider, a load the CPU can't forward.
    let brush = fill.hash_parts();
    let mut h = Hasher::new();
    h.write_u64(brush.payload);
    h.write_u64(corners.as_u64());
    border.hash_into(&mut h);
    ring.hash_into(&mut h);
    shadow.hash(&mut h);
    h.write_u8(brush.tag);
    let hash = ContentHash(h.finish());
    ChromeRow {
        fill,
        border,
        corners,
        shadow,
        hash,
        ring: has_ring,
    }
}

#[cfg(test)]
mod tests;
