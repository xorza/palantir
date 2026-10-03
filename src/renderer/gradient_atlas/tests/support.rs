//! Reading a baked row back, and minting gradients that differ only where
//! intended.

use crate::primitives::paint::brush::gradient::linear_geometry::LinearGradient;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::renderer::gradient_atlas::*;

/// Fresh f16 LUT row, all texels transparent before bake.
pub(super) fn fresh_row() -> LutRowTexels {
    [RgbaF16::TRANSPARENT; LUT_ROW_TEXELS]
}

/// A gradient whose stops are distinct for every `i` below 2^24: `i`'s
/// three low bytes are the first stop's colour. Only the stops key a row,
/// so varying the geometry instead would silently reuse one.
pub(super) fn distinct_grad(i: u32) -> LinearGradient {
    assert!(i < 1 << 24, "{i} does not fit three colour bytes");
    let [r, g, b, _] = i.to_le_bytes();
    LinearGradient::two_stop(
        0.0,
        SrgbaU8::rgb(r, g, b).into(),
        RgbaF32::new(0.0, 1.0, 0.0, 1.0),
    )
}

/// Register `distinct_grad(0)` through `distinct_grad(n - 1)`, in order.
pub(super) fn fill_rows(atlas: &mut CpuGradientAtlas, n: u32) -> Vec<LutRow> {
    (0..n)
        .map(|i| atlas.register(&distinct_grad(i).ramp))
        .collect()
}

pub(super) fn assert_real_row(atlas: &CpuGradientAtlas, row: LutRow) {
    assert!(
        (1..atlas.capacity()).contains(&row.0),
        "row {} must be in 1..{}",
        row.0,
        atlas.capacity(),
    );
}
