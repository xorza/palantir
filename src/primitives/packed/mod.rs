//! Compact encodings: four f16 lanes in eight bytes, their wire format, and
//! the fill words a quad reads.

pub(crate) mod fill_axis;
pub(crate) mod fill_kind;
pub(crate) mod half_simd;
pub(crate) mod serde;
