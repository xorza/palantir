//! IEC 61966-2-1's transfer in `f64`, written out as a reference apart from palantir's own, which
//! tests hold palantir's encoding and a GPU's stored bytes to.

use crate::primitives::math::libm_ext::Libm;

/// The encoded value of the linear `c`, unclamped.
pub(crate) fn encode(c: f64) -> f64 {
    if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.libm_powf(1.0 / 2.4) - 0.055
    }
}

/// The linear value of the encoded `e`, clamped to `0..=1` first.
pub(crate) fn decode(e: f64) -> f64 {
    let e = e.clamp(0.0, 1.0);
    if e <= 0.040_45 {
        e / 12.92
    } else {
        ((e + 0.055) / 1.055).libm_powf(2.4)
    }
}
