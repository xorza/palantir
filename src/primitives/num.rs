/// Libm-free `f32` helpers for the hot snap/quantize paths.
pub(crate) trait F32Ext {
    /// Exact `f32::round` (round-half-away-from-zero) without the libm
    /// `roundf` call: baseline x86-64 has no `roundss` (SSE4.1), so
    /// `.round()` compiles to an out-of-line call in the per-quad snap
    /// and pixel-alignment paths. Truncate through the integer pipeline
    /// and fix the half-step instead — bit-identical to `f32::round`
    /// for every input (`|x| ≥ 2^23` is already integral and returns
    /// unchanged, as do NaN/±inf; the sign-bit copy keeps `(-0.5, -0.0]`
    /// rounding to `-0.0` like libm).
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
        if !(-8_388_608.0 < self && self < 8_388_608.0) {
            return self;
        }
        // Work on |x| so one compare covers both signs and LLVM keeps it
        // branchless (a two-sided `d >= 0.5 / d <= -0.5` chain lowers to
        // real branches that mispredict on arbitrary fractions — ~4x
        // slower measured). Exact: |x| < 2^23 fits i32, and `ax - t` is
        // a multiple of `ulp(ax)` below 1.0, so the subtraction is
        // error-free. The final sign OR restores `-0.0` for inputs in
        // `(-0.5, -0.0]`.
        let ax = f32::from_bits(self.to_bits() & 0x7fff_ffff);
        let t = ax as i32 as f32;
        let inc = if ax - t >= 0.5 { 1.0 } else { 0.0 };
        f32::from_bits((t + inc).to_bits() | (self.to_bits() & 0x8000_0000))
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
            (8388607.5, 8388608.0), // last half-step under 2^23
            (-8388607.5, -8388608.0),
            (8388608.0, 8388608.0), // 2^23: guard path, already integral
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
