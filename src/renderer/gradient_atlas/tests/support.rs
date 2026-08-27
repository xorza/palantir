//! Reading a baked row back, and minting gradients that differ only where
//! intended.

use crate::common::hash::Hasher;
use crate::primitives::brush::gradient::linear_geometry::LinearGradient;
use crate::primitives::color::ColorU8;
use crate::renderer::gradient_atlas::*;
use std::hash::Hasher as _;

/// Fresh f16 LUT row, all texels transparent before bake.
pub(super) fn fresh_row() -> LutRowTexels {
    [ColorF16::TRANSPARENT; LUT_ROW_TEXELS]
}

/// Vary the *stops* (the only thing the row key now depends on)
/// across calls. Geometry (angle/centre/etc.) is now atlas-key
/// irrelevant — varying angle would silently produce row reuse
/// under the (stops, interp) keying.
pub(super) fn distinct_grad(seed: f32) -> LinearGradient {
    // FxHash on the seed bits gives well-distributed 32-bit chunks
    // for the (r, g, b) bytes, so different seeds produce visibly
    // different stop colours and the (stops, interp) hash lands in
    // distinct atlas rows.
    let mut h = Hasher::new();
    h.write_u32(seed.to_bits());
    let v = h.finish();
    let r = v as u8;
    let g = (v >> 8) as u8;
    let b = (v >> 16) as u8;
    LinearGradient::two_stop(0.0, ColorU8::rgb(r, g, b), ColorU8::rgb(0, 0xff, 0))
}

pub(super) fn register_for(atlas: &mut CpuGradientAtlas, g: LinearGradient) -> LutRow {
    atlas.register_stops(&g.stops, g.interp)
}

pub(super) fn assert_real_row(atlas: &CpuGradientAtlas, row: LutRow) {
    assert!(
        (1..atlas.capacity()).contains(&row.0),
        "row {} must be in 1..{}",
        row.0,
        atlas.capacity(),
    );
}
