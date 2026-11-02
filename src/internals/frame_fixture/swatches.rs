//! The fixture's accent swatches, named for the ink, not the job: [`tokens`](super::tokens) reads
//! them semantically (`WARN`, `OK`), the type specimen and the chrome by color.

use crate::primitives::paint::color::RgbaF32;

/// Teal-blue. The default when one color is enough.
pub(super) const TEAL: RgbaF32 = RgbaF32::hex(0x4cd3ff);
/// Orange. Pairs with [`TEAL`] for "two distinct things".
pub(super) const ORANGE: RgbaF32 = RgbaF32::hex(0xffa63d);
/// Green-yellow.
pub(super) const LIME: RgbaF32 = RgbaF32::hex(0xd9ff57);
/// Purple.
pub(super) const VIOLET: RgbaF32 = RgbaF32::hex(0xd897ff);
/// Red — the "wrong / danger" swatch.
pub(super) const RED: RgbaF32 = RgbaF32::hex(0xff5e44);
