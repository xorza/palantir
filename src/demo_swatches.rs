//! The categorical accent swatches the two bundled demo surfaces share:
//! the benchmark fixture ([`FrameFixture`](crate::FrameFixture)) and the
//! `showcase` binary.
//!
//! **Colours only, and that boundary is load-bearing.** A font size feeds
//! measurement, so a shared `caption_style` would let a restyle of the
//! showcase move every number the frame bench reports — silently, and in
//! a way no diff of the fixture would explain. A colour cannot: nothing
//! in measure or arrange reads one, so retheming is free. That asymmetry
//! is the whole reason the sharing stops here, and why each surface keeps
//! its own text styles, surface ladder, and scaffolding.
//!
//! Named for the ink rather than the job, because the two sites disagree
//! about the job: the fixture reads them semantically (`WARN`, `OK`), the
//! showcase categorically (`B`, `C`, "two distinct things"). Each aliases
//! these under its own vocabulary — same ink, different words.
//!
//! Not part of the supported surface; it exists only because both demo
//! surfaces ship in-tree.

use crate::primitives::color::Color;

/// Teal-blue. The default when one colour is enough.
pub const TEAL: Color = Color::hex(0x4cd3ff);
/// Orange. Pairs with [`TEAL`] for "two distinct things".
pub const ORANGE: Color = Color::hex(0xffa63d);
/// Green-yellow.
pub const LIME: Color = Color::hex(0xd9ff57);
/// Purple.
pub const VIOLET: Color = Color::hex(0xd897ff);
/// Red — the "wrong / danger" swatch.
pub const RED: Color = Color::hex(0xff5e44);
