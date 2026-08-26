use glam::Vec2;

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

    /// Snap to the whole-pixel grid that cache identities key on, as an
    /// integer so the result can be compared and hashed exactly.
    ///
    /// One definition on purpose: a measure-cache `available_q` and a text
    /// run's wrap width both quantize through here, and were they to land
    /// on different grids a cached subtree could be blitted against a shape
    /// measured at another width. Non-finite (an unbounded axis) saturates
    /// rather than wrapping through the `as` cast.
    fn quantize_px(self) -> i32;
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

    #[inline]
    fn quantize_px(self) -> i32 {
        if self.is_finite() {
            self.fast_round() as i32
        } else {
            i32::MAX
        }
    }
}

/// [`F32Ext`] applied per component, for the paint paths that snap a point.
pub(crate) trait Vec2Ext {
    /// Componentwise [`F32Ext::fast_round`]. `Vec2::round` is two
    /// out-of-line `roundf` calls on baseline x86-64, which is what this
    /// exists to keep off the per-icon and per-quad snap paths.
    fn fast_round(self) -> Vec2;
}

impl Vec2Ext for Vec2 {
    #[inline]
    fn fast_round(self) -> Vec2 {
        Vec2::new(self.x.fast_round(), self.y.fast_round())
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
mod tests;
