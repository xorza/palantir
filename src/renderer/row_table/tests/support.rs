//! Tables, ramps that differ only where intended, and what a flush owes, as the tests read them.

use crate::primitives::paint::brush::gradient::Gradient;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::renderer::row_table::*;

/// Row 0's texel: linear (1, 0, 1, 1), which the sRGB framebuffer encodes to `#ff00ff`.
pub(crate) fn magenta() -> RgbaF16 {
    RgbaF16::new(1.0, 0.0, 1.0, 1.0)
}

/// A gradient whose stops are distinct for every `i` below 2^24 (`i`'s three low bytes are the
/// first stop's color). Only the ramp keys a row, so varying geometry would silently reuse one.
pub(crate) fn distinct_grad(i: u32) -> Gradient {
    assert!(i < 1 << 24, "{i} does not fit three color bytes");
    let [r, g, b, _] = i.to_le_bytes();
    Gradient::horizontal([
        SrgbaU8::rgb(r, g, b).into(),
        RgbaF32::new(0.0, 1.0, 0.0, 1.0),
    ])
}

/// [`distinct_grad`]'s ramp.
pub(crate) fn ramp(i: u32) -> ColorRamp {
    distinct_grad(i).ramp
}

/// `ramp`'s row, as a flush bakes it.
pub(crate) fn baked(ramp: &ColorRamp) -> LutRowTexels {
    let mut row = [RgbaF16::TRANSPARENT; bake::LUT_ROW_TEXELS];
    bake::row(ramp, &mut row);
    row
}

/// A table of `first`, `soft` and `max` rows.
pub(crate) fn table(first: u32, soft: u32, max: u32) -> RowTable {
    RowTable::new(RowTableConfig {
        first_rows: first,
        soft_rows: soft,
        max_rows: max,
    })
}

/// Register `ramp(0)` through `ramp(n - 1)`, in order.
pub(crate) fn fill_rows(table: &mut RowTable, n: u32) -> Vec<LutRow> {
    (0..n).map(|i| table.register(&ramp(i))).collect()
}

/// The runs `table`'s flush owes, as `(start, end)`, which clears them.
pub(crate) fn runs(table: &mut RowTable) -> Vec<(u32, u32)> {
    table
        .flush()
        .runs()
        .iter()
        .map(|run| (run.start, run.end))
        .collect()
}

pub(crate) fn assert_real_row(table: &RowTable, row: LutRow) {
    assert!(
        (1..table.capacity()).contains(&row.0),
        "row {} must be in 1..{}",
        row.0,
        table.capacity(),
    );
}
