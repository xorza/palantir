//! The lowering every shape and chrome share: a [`Brush`] or curve ramp interned into the window's [`RecordStore`] gradient pool, and [`Chrome`] chrome lowered to its [`ChromeRow`]. Each shape lowers itself in its [`Lower`] impl and calls in here for what it interns.
//!
//! Entry points: [`Shapes::add`](crate::shape::shapes::Shapes::add) for shapes; `Tree::open_node` calls [`chrome`].
//!
//! [`Lower`]: crate::shape::Lower

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::backdrop_filter::BackdropFilter;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::brush::gradient::{Gradient, GradientGeometry};
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::stroke::Stroke;
use crate::primitives::reduction::Reduction;
use crate::shape::paint::chrome_row::ChromeRow;
use crate::shape::paint::curve_ramp::CurveRamp;
use crate::shape::paint::lowered_backdrop::LoweredBackdrop;
use crate::shape::paint::lowered_shadow::LoweredShadow;
use crate::shape::paint::shape_brush::ShapeBrush;
use crate::shape::paint::shape_stroke::ShapeStroke;
use crate::shape::record_store::RecordStore;
use crate::shape::record_store::recorded_gradient::RecordedGradient;
use std::hash;
use std::hash::{Hash as _, Hasher as _};

/// What a pane of fill `fill` paints for its `filter` under `transparency`.
#[derive(Debug)]
pub(crate) enum PaneFill {
    /// The pixels behind, shown through `filter` under the fill; `None` for a filter that changes
    /// nothing, which makes no pane.
    Through(Option<LoweredBackdrop>),
    /// The solid infill the fill becomes, with no filter.
    Solid(Brush),
}

/// The one statement of the reduced-transparency rule, for a chrome and a rect alike: a pane under
/// reduced transparency paints its solid infill and no filter.
pub(crate) fn pane_fill(filter: BackdropFilter, fill: &Brush, transparency: Reduction) -> PaneFill {
    if transparency.is_reduced() && !filter.is_noop() {
        PaneFill::Solid(filter.solid_infill(fill))
    } else {
        PaneFill::Through(LoweredBackdrop::of(filter))
    }
}

/// Stable content hash for a gradient kind or curve ramp: the fill kind's word, which tells the kinds apart, then the value's `Hash`, captured at lowering so downstream cache keys stay context-free.
#[inline]
fn gradient_hash<G: hash::Hash>(kind: FillKind, g: &G) -> ContentHash {
    let mut h = Hasher::new();
    h.write_u32(kind.0);
    g.hash(&mut h);
    ContentHash::finish(&h)
}

fn gradient_brush<G: GradientGeometry>(
    store: &mut RecordStore,
    kind: FillKind,
    gradient: &Gradient<G>,
) -> ShapeBrush {
    let hash = gradient_hash(kind, gradient);
    let id = store.gradients.intern(
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
        Brush::Solid(color) => ShapeBrush::Solid(color.to_f16()),
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

/// Lower a `Chrome` to a `ChromeRow`, sharing gradient lowering with shapes so chrome and shapes share one pool. Takes `chrome` by reference: [`Chrome`] is deliberately not `Copy`.
pub(crate) fn chrome(store: &mut RecordStore, chrome: &Chrome, ring: Stroke) -> ChromeRow {
    debug_assert!(
        !chrome.has_nan(),
        "a Chrome reaches lowering validated, by `Widget::record`: {chrome:?}",
    );
    let pane = pane_fill(chrome.backdrop_filter, &chrome.fill, store.transparency);
    let (fill_brush, backdrop) = match &pane {
        PaneFill::Through(backdrop) => (&chrome.fill, *backdrop),
        PaneFill::Solid(solid) => (solid, None),
    };
    let fill_brush = if chrome.is_noop() {
        &Brush::TRANSPARENT
    } else {
        fill_brush
    };
    let fill = brush(store, fill_brush);
    let border = ShapeStroke {
        width: chrome.border_inset(),
        ..ShapeStroke::lower(chrome.border)
    };
    let corners = chrome.corners;
    let shadow = LoweredShadow::lower(chrome.shadow);
    let ring = ShapeStroke::lower(ring);
    let has_ring = !ring.is_noop();
    // Fed as whole words from registers: a struct packed for one `write` is stored field by field and read back wider, a load the CPU can't forward.
    let brush = fill.hash_parts();
    let mut h = Hasher::new();
    h.write_u64(brush.payload);
    h.write_u64(corners.as_u64());
    border.hash_into(&mut h);
    ring.hash_into(&mut h);
    shadow.hash(&mut h);
    backdrop.hash(&mut h);
    h.write_u8(brush.tag);
    let hash = ContentHash::finish(&h);
    ChromeRow {
        fill,
        border,
        corners,
        shadow,
        backdrop,
        hash,
        ring: has_ring,
    }
}

#[cfg(test)]
mod tests;
