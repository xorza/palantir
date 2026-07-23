/// Libm-free `f32` helpers for the hot snap/quantize paths.
pub(crate) trait F32Ext {
    /// Exact `f32::round` (round half away from zero) without the libm
    /// call: baseline x86-64 has no `roundss` (SSE4.1), so `.round()`
    /// compiles to an out-of-line `roundf` call in the per-quad snap
    /// and pixel-alignment paths. Integer-pipeline trick from Go 1.10's
    /// `math.Round`: add a half-ulp at the fraction position (the
    /// mantissa carry performs the round-up), then clear the fraction.
    /// Bit-identical to `f32::round` for every f32 bit pattern —
    /// including NaN payloads, ±inf, and `(-0.5, -0.0]` → `-0.0` —
    /// at ~3.5× the speed of the libm call.
    fn fast_round(self) -> f32;

    /// `self` has no fractional part — equivalent to `x == x.round()`
    /// minus the libm call. NaN reports `false` like the equality it
    /// replaces; magnitudes ≥ 2^63 (unreachable for pixel coordinates)
    /// report `false`, which only forgoes a fast path.
    fn is_integral(&self) -> bool;
}

impl F32Ext for f32 {
    #[inline]
    fn fast_round(self) -> f32 {
        const SHIFT: u32 = 23;
        const BIAS: u32 = 127;
        const SIGN_MASK: u32 = 0x8000_0000;
        const FRAC_MASK: u32 = (1 << SHIFT) - 1;
        const HALF: u32 = 1 << (SHIFT - 1);
        const ONE: u32 = BIAS << SHIFT;
        let mut bits = self.to_bits();
        let e = (bits >> SHIFT) & 0xff;
        if e < BIAS {
            // |x| < 1: ±0, or ±1 once |x| ≥ 0.5 (e == BIAS - 1).
            bits &= SIGN_MASK;
            if e == BIAS - 1 {
                bits |= ONE;
            }
        } else if e < BIAS + SHIFT {
            // Fraction bits exist: the half-ulp add carries through the
            // mantissa (into the exponent at a .5 crossing — that IS the
            // round-up), the mask clears what's left of the fraction.
            let e = e - BIAS;
            bits += HALF >> e;
            bits &= !(FRAC_MASK >> e);
        }
        // e ≥ BIAS + SHIFT: already integral, or inf/NaN — unchanged.
        f32::from_bits(bits)
    }

    #[inline]
    fn is_integral(&self) -> bool {
        *self == (*self as i64 as f32)
    }
}

/// Marker trait for primitive numeric types accepted by `From` impls on
/// `Sizing`, `Size`, `Corners`, `Spacing`, etc.
pub(crate) trait Num: Copy {
    fn as_f32(self) -> f32;
}

macro_rules! impl_num {
    ($($t:ty),*) => {
        $(
            impl Num for $t {
                fn as_f32(self) -> f32 { self as f32 }
            }
        )*
    };
}

impl_num!(f32, f64, i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

#[cfg(test)]
mod tests {
    use crate::primitives::num::F32Ext;

    #[test]
    fn fast_round_matches_std_round() {
        // Hand-picked halves pin the away-from-zero contract:
        // 0.5 → 1, 2.5 → 3 (not banker's 2), -0.5 → -1, -2.5 → -3.
        let cases: &[(f32, f32)] = &[
            (0.5, 1.0),
            (1.5, 2.0),
            (2.5, 3.0),
            (-0.5, -1.0),
            (-1.5, -2.0),
            (-2.5, -3.0),
            (0.49999997, 0.0),      // largest f32 below 0.5
            (0.50000006, 1.0),      // smallest f32 above 0.5
            (8388607.5, 8388608.0), // last representable half-step
            (-8388607.5, -8388608.0),
            (8388608.0, 8388608.0), // 2^23: fraction-free path
            (3.4e38, 3.4e38),
            (f32::INFINITY, f32::INFINITY),
            (f32::NEG_INFINITY, f32::NEG_INFINITY),
        ];
        for &(x, want) in cases {
            assert_eq!(x.fast_round().to_bits(), want.to_bits(), "x = {x}");
            assert_eq!(
                want.to_bits(),
                x.round().to_bits(),
                "case out of sync with std: {x}"
            );
        }
        assert!(f32::NAN.fast_round().is_nan());

        // Dense sweep: bit-identical to `f32::round` across mixed
        // magnitudes and signs (0.0173 step avoids hitting only halves).
        for i in -60_000..60_000i32 {
            let x = i as f32 * 0.0173;
            assert_eq!(x.fast_round().to_bits(), x.round().to_bits(), "x = {x}");
        }
        // Exact half-integers across the i16 range.
        for i in -32_768..32_768i32 {
            let x = i as f32 + 0.5;
            assert_eq!(x.fast_round().to_bits(), x.round().to_bits(), "x = {x}");
        }
        // Signed-zero cases stay bit-exact: -0.0 and (-0.5, -0.0) → -0.0.
        assert_eq!((-0.0f32).fast_round().to_bits(), (-0.0f32).to_bits());
        assert_eq!((-0.25f32).fast_round().to_bits(), (-0.0f32).to_bits());
    }

    #[test]
    fn is_integral_matches_round_equality() {
        let integral = [0.0, -0.0, 1.0, -7.0, 8388608.0, 1e18];
        let fractional = [0.1, -0.5, 1.5, 8388607.5, f32::NAN];
        for x in integral {
            assert!(x.is_integral(), "x = {x}");
            assert!(x == x.round(), "case out of sync with std: {x}");
        }
        for x in fractional {
            assert!(!x.is_integral(), "x = {x}");
            assert!(
                x != x.round() || x.is_nan(),
                "case out of sync with std: {x}"
            );
        }
        // Beyond i64 range the check conservatively reports false (the
        // equality it replaces said true); only forgoes a fast path.
        assert!(!1e30.is_integral());
        assert!(!f32::INFINITY.is_integral());
    }
}
