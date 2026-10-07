//! The categorical accent swatches shared by the benchmark fixture ([`FrameFixture`](crate::internals::frame_fixture::FrameFixture)) and the `showcase` example.
//!
//! **Colours only, and that boundary is load-bearing.** A font size feeds measurement, so a shared style would let a showcase restyle move every frame-bench number; no measure or arrange pass reads a colour. Each surface keeps its own text styles and scaffolding.
//!
//! Named for the ink, not the job: the fixture reads them semantically (`WARN`, `OK`), the showcase categorically (`B`, `C`). Not part of the supported surface.

use crate::primitives::paint::color::RgbaF32;

/// Teal-blue. The default when one colour is enough.
pub const TEAL: RgbaF32 = RgbaF32::hex(0x4cd3ff);
/// Orange. Pairs with [`TEAL`] for "two distinct things".
pub const ORANGE: RgbaF32 = RgbaF32::hex(0xffa63d);
/// Green-yellow.
pub const LIME: RgbaF32 = RgbaF32::hex(0xd9ff57);
/// Purple.
pub const VIOLET: RgbaF32 = RgbaF32::hex(0xd897ff);
/// Red — the "wrong / danger" swatch.
pub const RED: RgbaF32 = RgbaF32::hex(0xff5e44);
