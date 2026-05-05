//! Shared "layout swatch" palette for showcase demos. These are demo
//! content — colored rectangles that visualize where layout puts each
//! child. Not framework theme. Picked from the Ayu syntax-color block
//! so they harmonize with the default palette.

use palantir::Color;

/// Teal-blue. Default swatch when one color is enough.
pub const A: Color = Color::hex(0x4cd3ff);
/// Orange. Pair with `A` for "two distinct things".
pub const B: Color = Color::hex(0xffa63d);
/// Green.
pub const C: Color = Color::hex(0xd9ff57);
/// Purple.
pub const D: Color = Color::hex(0xd897ff);
